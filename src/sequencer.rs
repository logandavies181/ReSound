use std::path::Path;

use crate::{
    luaapi::{Chord, InstrumentType, LuaExecution, LuaState},
    renderer_input::{EffectSpan, RendererInput, SampleInstrument, ScoreEvent},
};

const VELOCITY: u8 = 100;
type SequencedScore = (Vec<ScoreEvent>, f64, Vec<EffectSpan>);

pub fn sequence(
    execution: LuaExecution,
    project_dir: &Path,
    output_path: impl Into<std::path::PathBuf>,
) -> Result<RendererInput, Box<dyn std::error::Error>> {
    let LuaExecution {
        state,
        initial_parameter_values,
        parameter_values,
        lua,
    } = execution;
    let (score, score_duration, effects) = sequence_score(&state)?;
    let vst = state
        .instruments
        .iter()
        .filter(|instrument| instrument.r#type == InstrumentType::Vst)
        .map(|instrument| crate::vst_loader::resolve(instrument, project_dir))
        .collect::<Result<Vec<_>, _>>()?;
    if vst.is_empty() {
        return Err("no VST instrument configured".into());
    }
    let samples = collect_samples(&state.instruments, project_dir);

    Ok(RendererInput {
        output_path: output_path.into(),
        vst,
        samples,
        config: state,
        render_duration_seconds: score_duration,
        score,
        effects,
        initial_parameter_values,
        parameter_values,
        _lua: lua,
    })
}

fn collect_samples(
    instruments: &[crate::luaapi::InstrumentConfig],
    project_dir: &Path,
) -> Vec<SampleInstrument> {
    instruments
        .iter()
        .filter(|instrument| instrument.r#type == InstrumentType::Wav)
        .map(|instrument| SampleInstrument {
            path: project_dir.join(&instrument.path),
        })
        .collect()
}

fn sequence_score(state: &LuaState) -> Result<SequencedScore, Box<dyn std::error::Error>> {
    if !state.default_bpm.is_finite() || state.default_bpm <= 0.0 {
        return Err("default BPM must be finite and greater than zero".into());
    }
    if !state.default_meter.is_finite() || state.default_meter <= 0.0 {
        return Err("default meter must be finite and greater than zero".into());
    }

    let seconds_per_quarter = 60.0 / state.default_bpm;
    let seconds_per_bar = state.default_meter * seconds_per_quarter;
    let mut score = Vec::new();
    let mut effects = Vec::new();
    let mut score_duration: f64 = 0.0;

    for chord_set in &state.chord_sets {
        let instrument_index = state
            .instruments
            .iter()
            .position(|instrument| instrument.name == chord_set.instrument_name)
            .ok_or_else(|| {
                format!(
                    "chord set references unknown instrument '{}'",
                    chord_set.instrument_name
                )
            })?;
        let instrument = &state.instruments[instrument_index];
        let vst_index = (instrument.r#type == InstrumentType::Vst).then(|| {
            state.instruments[..instrument_index]
                .iter()
                .filter(|candidate| candidate.r#type == InstrumentType::Vst)
                .count()
        });
        let sample_index = if instrument.r#type == InstrumentType::Wav {
            Some(
                state.instruments[..instrument_index]
                    .iter()
                    .filter(|candidate| candidate.r#type == InstrumentType::Wav)
                    .count(),
            )
        } else {
            None
        };
        if !chord_set.at.is_finite() || chord_set.at < 1.0 || chord_set.at.fract() != 0.0 {
            return Err(format!(
                "chord set bar must be a positive whole number, got {}",
                chord_set.at
            )
            .into());
        }
        let repetitions = match chord_set.times {
            Some(times)
                if times.is_finite()
                    && times >= 1.0
                    && times.fract() == 0.0
                    && times < usize::MAX as f64 =>
            {
                times as usize
            }
            Some(times) => {
                return Err(format!(
                    "chord repeat count must be a positive whole number, got {times}"
                )
                .into());
            }
            None => 1,
        };

        let mut sequence_beats = 0.0;
        for chord in &chord_set.chords {
            validate_chord(chord)?;
            sequence_beats += chord.duration.length;
        }

        let start_seconds = (chord_set.at - 1.0) * seconds_per_bar;
        let sequence_seconds = sequence_beats * seconds_per_quarter;
        let end_seconds = start_seconds + sequence_seconds * repetitions as f64;
        if !end_seconds.is_finite() {
            return Err("chord set extends beyond the supported timeline".into());
        }
        score_duration = score_duration.max(end_seconds);

        for repetition in 0..repetitions {
            let mut beat_offset = repetition as f64 * sequence_beats;
            for chord in &chord_set.chords {
                let at_seconds = start_seconds + beat_offset * seconds_per_quarter;
                let note_off_seconds =
                    at_seconds + chord.duration.length * chord.duration.ratio * seconds_per_quarter;
                if !note_off_seconds.is_finite() {
                    return Err("chord gate extends beyond the supported timeline".into());
                }
                if let Some(effect_fn) = &chord.effect_fn {
                    effects.push(EffectSpan {
                        start_seconds: at_seconds,
                        playing_end_seconds: note_off_seconds,
                        effect_fn: effect_fn.clone(),
                    });
                }
                if chord.duration.ratio > 0.0 {
                    for pitch in &chord.pitches {
                        match (instrument.r#type, pitch.as_str()) {
                            (InstrumentType::Wav, "x") => {
                                score.push(ScoreEvent::PlaySample {
                                    at_seconds,
                                    sample_index: sample_index
                                        .ok_or("WAV instrument missing sample index")?,
                                });
                            }
                            (InstrumentType::Wav, _) => {
                                return Err(format!(
                                    "WAV instrument '{}' only accepts the sample trigger pitch 'x'",
                                    instrument.name
                                )
                                .into());
                            }
                            (InstrumentType::Vst, "x") => {
                                return Err(format!(
                                    "VST instrument '{}' does not accept the sample trigger pitch 'x'",
                                    instrument.name
                                )
                                .into());
                            }
                            (InstrumentType::Vst, _) => {
                                let note = pitch_to_midi(pitch)?;
                                score.push(ScoreEvent::NoteOn {
                                    at_seconds,
                                    vst_index: vst_index.ok_or("VST instrument missing index")?,
                                    note,
                                    velocity: VELOCITY,
                                });
                                score.push(ScoreEvent::NoteOff {
                                    at_seconds: note_off_seconds,
                                    vst_index: vst_index.ok_or("VST instrument missing index")?,
                                    note,
                                });
                            }
                        }
                    }
                }
                beat_offset += chord.duration.length;
            }
        }
    }

    score.sort_by(|left, right| {
        let (left_time, left_priority) = event_order(left);
        let (right_time, right_priority) = event_order(right);
        left_time
            .total_cmp(&right_time)
            .then_with(|| left_priority.cmp(&right_priority))
    });

    Ok((score, score_duration, effects))
}

fn validate_chord(chord: &Chord) -> Result<(), Box<dyn std::error::Error>> {
    let duration = chord.duration;
    if !duration.length.is_finite() || duration.length <= 0.0 {
        return Err(format!(
            "chord duration must be finite and greater than zero, got {}",
            duration.length
        )
        .into());
    }
    if !duration.ratio.is_finite() || duration.ratio < 0.0 {
        return Err(format!(
            "chord duration ratio must be finite and non-negative, got {}",
            duration.ratio
        )
        .into());
    }
    Ok(())
}

fn pitch_to_midi(pitch: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let split = pitch
        .find(|character: char| character.is_ascii_digit())
        .ok_or_else(|| format!("invalid pitch name: {pitch}"))?;
    let (pitch_class, octave) = pitch.split_at(split);
    if octave.len() != 1 {
        return Err(format!("invalid pitch name: {pitch}").into());
    }
    let octave: i16 = octave.parse()?;
    let pitch_class_offset = match pitch_class {
        "cb" => -1,
        "c" => 0,
        "cs" | "db" => 1,
        "d" => 2,
        "ds" | "eb" => 3,
        "e" | "fb" => 4,
        "es" | "f" => 5,
        "fs" | "gb" => 6,
        "g" => 7,
        "gs" | "ab" => 8,
        "a" => 9,
        "as" | "bb" => 10,
        "b" => 11,
        "bs" => 12,
        _ => return Err(format!("invalid pitch name: {pitch}").into()),
    };
    let midi_note = octave * 12 + 12 + pitch_class_offset;
    if !(0..=127).contains(&midi_note) {
        return Err(format!("pitch {pitch} is outside the MIDI note range 0-127").into());
    }
    Ok(midi_note as u8)
}

fn event_order(event: &ScoreEvent) -> (f64, u8) {
    match event {
        ScoreEvent::NoteOff { at_seconds, .. } => (*at_seconds, 0),
        ScoreEvent::NoteOn { at_seconds, .. } => (*at_seconds, 1),
        ScoreEvent::PlaySample { at_seconds, .. } => (*at_seconds, 2),
    }
}

#[cfg(test)]
mod tests {
    use crate::luaapi::{Chord, ChordSet, Duration, InstrumentConfig, InstrumentType, LuaState};

    use super::{collect_samples, pitch_to_midi, sequence_score};
    use crate::renderer_input::ScoreEvent;

    #[test]
    fn collects_configured_wav_paths_and_ignores_vst_paths() {
        let instruments = vec![
            InstrumentConfig {
                name: "kick".into(),
                path: "samples/kick.wav".into(),
                r#type: InstrumentType::Wav,
            },
            InstrumentConfig {
                name: "synth".into(),
                path: "plugins/synth.vst3".into(),
                r#type: InstrumentType::Vst,
            },
            InstrumentConfig {
                name: "snare".into(),
                path: "drums/snare.wav".into(),
                r#type: InstrumentType::Wav,
            },
        ];

        assert_eq!(
            collect_samples(&instruments, std::path::Path::new("/project"))
                .into_iter()
                .map(|sample| sample.path)
                .collect::<Vec<_>>(),
            vec![
                std::path::PathBuf::from("/project/samples/kick.wav"),
                std::path::PathBuf::from("/project/drums/snare.wav")
            ]
        );
    }

    #[test]
    fn maps_enharmonic_pitch_names_to_midi_notes() {
        assert_eq!(pitch_to_midi("c4").unwrap(), 60);
        assert_eq!(pitch_to_midi("bs3").unwrap(), 60);
        assert_eq!(pitch_to_midi("db4").unwrap(), 61);
        assert_eq!(pitch_to_midi("cb4").unwrap(), 59);
        assert_eq!(
            pitch_to_midi("bs9").unwrap_err().to_string(),
            "pitch bs9 is outside the MIDI note range 0-127"
        );
    }

    #[test]
    fn sequences_chords_with_bar_offsets_durations_and_repetitions() {
        let mut state = LuaState {
            default_bpm: 120.0,
            default_meter: 4.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                path: "synth.vst3".into(),
                r#type: InstrumentType::Vst,
            }],
            ..LuaState::default()
        };
        state.chord_sets.push(ChordSet {
            instrument_name: "synth".into(),
            at: 2.0,
            times: Some(2.0),
            chords: vec![
                Chord {
                    pitches: vec!["c4".into(), "e4".into()],
                    duration: Duration {
                        length: 1.0,
                        ratio: 0.5,
                    },
                    effect_fn: None,
                },
                Chord {
                    pitches: Vec::new(),
                    duration: Duration {
                        length: 1.0,
                        ratio: 1.0,
                    },
                    effect_fn: None,
                },
                Chord {
                    pitches: vec!["g4".into()],
                    duration: Duration {
                        length: 2.0,
                        ratio: 1.0,
                    },
                    effect_fn: None,
                },
            ],
        });

        let (score, duration, effects) = sequence_score(&state).unwrap();
        assert!(effects.is_empty());
        assert_eq!(duration, 6.0);
        let midi_events = score
            .iter()
            .filter_map(|event| match event {
                ScoreEvent::NoteOn {
                    at_seconds, note, ..
                } => Some((*at_seconds, *note)),
                ScoreEvent::NoteOff { .. } | ScoreEvent::PlaySample { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            midi_events,
            vec![
                (2.0, 60),
                (2.0, 64),
                (3.0, 67),
                (4.0, 60),
                (4.0, 64),
                (5.0, 67)
            ]
        );
        assert!(score.iter().any(|event| matches!(
            event,
            ScoreEvent::NoteOff {
                at_seconds: 2.25,
                note: 60,
                ..
            }
        )));
    }

    #[test]
    fn gate_ratio_can_exceed_one_without_delaying_the_next_chord() {
        let state = LuaState {
            default_bpm: 120.0,
            default_meter: 4.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                path: "synth.vital".into(),
                r#type: InstrumentType::Vst,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 1.0,
                times: None,
                chords: vec![
                    Chord {
                        pitches: vec!["c4".into()],
                        duration: Duration {
                            length: 0.5,
                            ratio: 3.0,
                        },
                        effect_fn: None,
                    },
                    Chord {
                        pitches: vec!["d4".into()],
                        duration: Duration {
                            length: 0.5,
                            ratio: 1.0,
                        },
                        effect_fn: None,
                    },
                ],
            }],
        };

        let (score, _, _) = sequence_score(&state).unwrap();
        let note_ons = score
            .iter()
            .filter_map(|event| match event {
                ScoreEvent::NoteOn {
                    at_seconds, note, ..
                } => Some((*at_seconds, *note)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let note_offs = score
            .iter()
            .filter_map(|event| match event {
                ScoreEvent::NoteOff {
                    at_seconds, note, ..
                } => Some((*at_seconds, *note)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(note_ons, vec![(0.0, 60), (0.25, 62)]);
        assert_eq!(note_offs, vec![(0.5, 62), (0.75, 60)]);
    }

    #[test]
    fn schedules_effects_for_each_chord_repetition() {
        let lua = mlua::Lua::new();
        let effect_fn = lua.load("return function() end").eval().unwrap();
        let state = LuaState {
            default_bpm: 120.0,
            default_meter: 4.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                path: "synth.vital".into(),
                r#type: InstrumentType::Vst,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 2.0,
                times: Some(2.0),
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        ratio: 0.5,
                    },
                    effect_fn: Some(effect_fn),
                }],
            }],
        };

        let (_, _, effects) = sequence_score(&state).unwrap();
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].start_seconds, 2.0);
        assert_eq!(effects[0].playing_end_seconds, 2.25);
        assert_eq!(effects[1].start_seconds, 2.5);
        assert_eq!(effects[1].playing_end_seconds, 2.75);
    }

    #[test]
    fn routes_vst_notes_to_their_configured_instrument() {
        let state = LuaState {
            instruments: vec![
                InstrumentConfig {
                    name: "vital".into(),
                    path: "vital.vital".into(),
                    r#type: InstrumentType::Vst,
                },
                InstrumentConfig {
                    name: "kick".into(),
                    path: "kick.wav".into(),
                    r#type: InstrumentType::Wav,
                },
                InstrumentConfig {
                    name: "dexed".into(),
                    path: "dexed.dexed".into(),
                    r#type: InstrumentType::Vst,
                },
            ],
            chord_sets: vec![
                ChordSet {
                    instrument_name: "vital".into(),
                    at: 1.0,
                    times: None,
                    chords: vec![Chord {
                        pitches: vec!["c4".into()],
                        duration: Duration {
                            length: 1.0,
                            ratio: 1.0,
                        },
                        effect_fn: None,
                    }],
                },
                ChordSet {
                    instrument_name: "dexed".into(),
                    at: 1.0,
                    times: None,
                    chords: vec![Chord {
                        pitches: vec!["d4".into()],
                        duration: Duration {
                            length: 1.0,
                            ratio: 1.0,
                        },
                        effect_fn: None,
                    }],
                },
            ],
            ..LuaState::default()
        };

        let (score, _, _) = sequence_score(&state).unwrap();
        let note_ons = score
            .iter()
            .filter_map(|event| match event {
                ScoreEvent::NoteOn {
                    vst_index, note, ..
                } => Some((*vst_index, *note)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(note_ons, vec![(0, 60), (1, 62)]);
    }

    #[test]
    fn rejects_invalid_meter_and_out_of_range_pitches() {
        let mut state = LuaState::default();
        state.default_bpm = 0.0;
        assert!(sequence_score(&state).is_err());

        state.default_bpm = 120.0;
        state.default_meter = 0.0;
        assert!(sequence_score(&state).is_err());

        state.default_meter = 4.0;
        state.instruments.push(InstrumentConfig {
            name: "synth".into(),
            path: "synth.vst3".into(),
            r#type: InstrumentType::Vst,
        });
        state.chord_sets.push(ChordSet {
            instrument_name: "synth".into(),
            at: 1.0,
            times: None,
            chords: vec![Chord {
                pitches: vec!["bs9".into()],
                duration: Duration {
                    length: 1.0,
                    ratio: 1.0,
                },
                effect_fn: None,
            }],
        });
        assert!(sequence_score(&state).is_err());
    }

    #[test]
    fn turns_x_pitches_into_events_for_the_named_wav_instrument() {
        let state = LuaState {
            instruments: vec![
                InstrumentConfig {
                    name: "kick".into(),
                    path: "kick.wav".into(),
                    r#type: InstrumentType::Wav,
                },
                InstrumentConfig {
                    name: "snare".into(),
                    path: "snare.wav".into(),
                    r#type: InstrumentType::Wav,
                },
            ],
            chord_sets: vec![ChordSet {
                instrument_name: "snare".into(),
                at: 2.0,
                times: None,
                chords: vec![Chord {
                    pitches: vec!["x".into()],
                    duration: Duration {
                        length: 1.0,
                        ratio: 1.0,
                    },
                    effect_fn: None,
                }],
            }],
            ..LuaState::default()
        };

        let (score, duration, _) = sequence_score(&state).unwrap();
        assert_eq!(duration, 2.5);
        assert!(matches!(
            score.as_slice(),
            [ScoreEvent::PlaySample {
                at_seconds: 2.0,
                sample_index: 1
            }]
        ));
    }

    #[test]
    fn rejects_chord_sets_with_unknown_instruments() {
        let state = LuaState {
            chord_sets: vec![ChordSet {
                instrument_name: "missing".into(),
                at: 1.0,
                times: None,
                chords: Vec::new(),
            }],
            ..LuaState::default()
        };

        assert_eq!(
            sequence_score(&state).unwrap_err().to_string(),
            "chord set references unknown instrument 'missing'"
        );
    }

    #[test]
    fn zero_gate_ratio_is_silent_but_advances_timeline() {
        let state = LuaState {
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                path: "synth.vst3".into(),
                r#type: InstrumentType::Vst,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 1.0,
                times: None,
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        ratio: 0.0,
                    },
                    effect_fn: None,
                }],
            }],
            ..LuaState::default()
        };
        let (score, duration, _) = sequence_score(&state).unwrap();
        assert_eq!(duration, 0.5);
        assert!(!score.iter().any(|event| matches!(
            event,
            ScoreEvent::NoteOn { .. } | ScoreEvent::NoteOff { .. }
        )));
    }
}
