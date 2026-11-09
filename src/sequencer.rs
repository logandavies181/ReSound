use std::path::Path;

use crate::{
    luaapi::{Chord, InstrumentType, LuaExecution, LuaState},
    renderer_input::{CachedPattern, EffectSpan, RendererInput, SampleInstrument, ScoreEvent},
};

const VELOCITY: u8 = 100;
type SequencedScore = (Vec<ScoreEvent>, f64, Vec<EffectSpan>, Vec<CachedPattern>);

pub fn sequence_from_bar(
    execution: LuaExecution,
    project_dir: &Path,
    output_path: impl Into<std::path::PathBuf>,
    from_bar: u64,
) -> Result<RendererInput, Box<dyn std::error::Error>> {
    let LuaExecution {
        state,
        initial_parameter_values,
        parameter_values,
        lua,
    } = execution;
    let (score, score_duration, effects, cached_patterns) =
        sequence_score_from_bar(&state, from_bar)?;
    let vst = state
        .instruments
        .iter()
        .filter(|instrument| instrument.r#type == InstrumentType::Vst)
        .map(|instrument| crate::vst_loader::resolve(instrument, project_dir))
        .collect::<Result<Vec<_>, _>>()?;
    if vst.is_empty() {
        return Err("no VST instrument configured".into());
    }
    let samples = collect_samples(&state.instruments, project_dir)?;

    Ok(RendererInput {
        output_path: output_path.into(),
        cache_dir: project_dir.join(".resound-cache"),
        vst,
        samples,
        config: state,
        render_duration_seconds: score_duration,
        score,
        cached_patterns,
        effects,
        initial_parameter_values,
        parameter_values,
        _lua: lua,
    })
}

fn collect_samples(
    instruments: &[crate::luaapi::InstrumentConfig],
    project_dir: &Path,
) -> Result<Vec<SampleInstrument>, Box<dyn std::error::Error>> {
    instruments
        .iter()
        .filter(|instrument| instrument.r#type == InstrumentType::Wav)
        .map(|instrument| {
            let path = instrument
                .path
                .as_ref()
                .ok_or_else(|| format!("sample instrument '{}' has no path", instrument.name))?;
            Ok(SampleInstrument {
                path: project_dir.join(path),
                volume: instrument.volume,
            })
        })
        .collect()
}

fn sequence_score_from_bar(
    state: &LuaState,
    from_bar: u64,
) -> Result<SequencedScore, Box<dyn std::error::Error>> {
    if from_bar == 0 {
        return Err("from-bar must be a positive whole number".into());
    }
    if !state.default_bpm.is_finite() || state.default_bpm <= 0.0 {
        return Err("default BPM must be finite and greater than zero".into());
    }
    if !state.default_meter.is_finite() || state.default_meter <= 0.0 {
        return Err("default meter must be finite and greater than zero".into());
    }

    let seconds_per_quarter = 60.0 / state.default_bpm;
    let seconds_per_bar = state.default_meter * seconds_per_quarter;
    let from_seconds = (from_bar as f64 - 1.0) * seconds_per_bar;
    if !from_seconds.is_finite() {
        return Err("from-bar exceeds the supported timeline".into());
    }
    let mut score = Vec::new();
    let mut effects = Vec::new();
    let mut cached_patterns = Vec::new();
    let mut score_duration: f64 = 0.0;
    let cacheable_sets = cacheable_chord_sets(state, seconds_per_quarter, seconds_per_bar);

    for (chord_set_index, chord_set) in state.chord_sets.iter().enumerate() {
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
        score_duration = score_duration.max(end_seconds - from_seconds);

        let is_cacheable = cacheable_sets[chord_set_index];
        let mut cached_starts = Vec::new();
        let mut cached_score = None;
        let mut cached_duration_seconds = sequence_seconds;
        for repetition in 0..repetitions {
            let repetition_start = start_seconds + repetition as f64 * sequence_seconds;
            let cache_this_repetition = is_cacheable && repetition_start >= from_seconds;
            let build_cached_pattern = cache_this_repetition && cached_score.is_none();
            if cache_this_repetition {
                cached_starts.push(repetition_start - from_seconds);
                if build_cached_pattern {
                    cached_score = Some(Vec::new());
                }
            }
            let mut beat_offset = repetition as f64 * sequence_beats;
            for chord in &chord_set.chords {
                let at_seconds = start_seconds + beat_offset * seconds_per_quarter;
                let play_length = chord.duration.play_length.unwrap_or(chord.duration.length);
                let note_off_seconds = at_seconds + play_length * seconds_per_quarter;
                if !note_off_seconds.is_finite() {
                    return Err("chord gate extends beyond the supported timeline".into());
                }
                if at_seconds >= from_seconds {
                    if let Some(effect_fn) = &chord.effect_fn {
                        effects.push(EffectSpan {
                            start_seconds: at_seconds - from_seconds,
                            playing_end_seconds: note_off_seconds - from_seconds,
                            effect_fn: effect_fn.clone(),
                        });
                    }
                    if play_length > 0.0 {
                        let event_at_seconds = if cache_this_repetition {
                            at_seconds - repetition_start
                        } else {
                            at_seconds - from_seconds
                        };
                        let event_note_off_seconds = if cache_this_repetition {
                            note_off_seconds - repetition_start
                        } else {
                            note_off_seconds - from_seconds
                        };
                        for pitch in &chord.pitches {
                            match (instrument.r#type, pitch.as_str()) {
                                (InstrumentType::Wav, "x") => {
                                    score.push(ScoreEvent::PlaySample {
                                        at_seconds: event_at_seconds,
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
                                    let target = if !cache_this_repetition {
                                        Some(&mut score)
                                    } else if build_cached_pattern {
                                        cached_score.as_mut()
                                    } else {
                                        None
                                    };
                                    if let Some(target) = target {
                                        target.push(ScoreEvent::NoteOn {
                                            at_seconds: event_at_seconds,
                                            vst_index: vst_index
                                                .ok_or("VST instrument missing index")?,
                                            note,
                                            velocity: VELOCITY,
                                        });
                                        target.push(ScoreEvent::NoteOff {
                                            at_seconds: event_note_off_seconds,
                                            vst_index: vst_index
                                                .ok_or("VST instrument missing index")?,
                                            note,
                                        });
                                    }
                                    cached_duration_seconds = cached_duration_seconds
                                        .max(note_off_seconds - repetition_start);
                                }
                            }
                        }
                    }
                }
                beat_offset += chord.duration.length;
            }
        }
        if let (Some(key), Some(mut cached_score)) = (chord_set.cache_key.as_ref(), cached_score)
            && !cached_starts.is_empty()
            && !cached_score.is_empty()
        {
            cached_score.sort_by(|left, right| {
                let (left_time, left_priority) = event_order(left);
                let (right_time, right_priority) = event_order(right);
                left_time
                    .total_cmp(&right_time)
                    .then_with(|| left_priority.cmp(&right_priority))
            });
            cached_patterns.push(CachedPattern {
                key: key.clone(),
                vst_index: vst_index.ok_or("cacheable chord set missing VST index")?,
                start_seconds: cached_starts,
                duration_seconds: cached_duration_seconds,
                score: cached_score,
            });
        }
    }

    score.sort_by(|left, right| {
        let (left_time, left_priority) = event_order(left);
        let (right_time, right_priority) = event_order(right);
        left_time
            .total_cmp(&right_time)
            .then_with(|| left_priority.cmp(&right_priority))
    });

    Ok((score, score_duration, effects, cached_patterns))
}

fn cacheable_chord_sets(
    state: &LuaState,
    seconds_per_quarter: f64,
    seconds_per_bar: f64,
) -> Vec<bool> {
    let mut candidates = vec![None; state.chord_sets.len()];
    for (index, chord_set) in state.chord_sets.iter().enumerate() {
        let is_vst = state.instruments.iter().any(|instrument| {
            instrument.name == chord_set.instrument_name && instrument.r#type == InstrumentType::Vst
        });
        if chord_set.cache_key.is_none()
            || !is_vst
            || state
                .chord_sets
                .iter()
                .any(|other| other.chords.iter().any(|chord| chord.effect_fn.is_some()))
        {
            continue;
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
            None => 1,
            Some(_) => continue,
        };
        if !chord_set.at.is_finite() || chord_set.at < 1.0 || chord_set.at.fract() != 0.0 {
            continue;
        }

        let mut sequence_beats = 0.0;
        let mut last_gate_beats: f64 = 0.0;
        let mut valid = true;
        for chord in &chord_set.chords {
            let play_length = chord.duration.play_length.unwrap_or(chord.duration.length);
            if !chord.duration.length.is_finite()
                || chord.duration.length <= 0.0
                || !play_length.is_finite()
                || play_length < 0.0
            {
                valid = false;
                break;
            }
            last_gate_beats = last_gate_beats.max(sequence_beats + play_length);
            sequence_beats += chord.duration.length;
        }
        if !valid || sequence_beats <= 0.0 || last_gate_beats > sequence_beats {
            continue;
        }

        let start = (chord_set.at - 1.0) * seconds_per_bar;
        let sequence_seconds = sequence_beats * seconds_per_quarter;
        let end = start
            + (repetitions.saturating_sub(1) as f64 * sequence_seconds)
            + last_gate_beats * seconds_per_quarter;
        if start.is_finite() && end.is_finite() && end > start {
            candidates[index] = Some((chord_set.instrument_name.clone(), start, end));
        }
    }

    let mut cacheable = candidates.iter().map(Option::is_some).collect::<Vec<_>>();
    for left in 0..candidates.len() {
        let Some((left_instrument, left_start, left_end)) = &candidates[left] else {
            continue;
        };
        for right in left + 1..candidates.len() {
            let Some((right_instrument, right_start, right_end)) = &candidates[right] else {
                continue;
            };
            if left_instrument == right_instrument
                && left_start < right_end
                && right_start < left_end
            {
                cacheable[left] = false;
                cacheable[right] = false;
            }
        }
    }

    cacheable
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
    if let Some(play_length) = duration.play_length
        && (!play_length.is_finite() || play_length < 0.0)
    {
        return Err(format!(
            "chord play length must be finite and non-negative, got {play_length}"
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

    use super::{collect_samples, pitch_to_midi, sequence_score_from_bar};
    use crate::renderer_input::ScoreEvent;

    #[test]
    fn collects_configured_wav_paths_and_ignores_vst_paths() {
        let instruments = vec![
            InstrumentConfig {
                name: "kick".into(),
                plugin: String::new(),
                path: Some("samples/kick.wav".into()),
                r#type: InstrumentType::Wav,
                volume: 1.0,
            },
            InstrumentConfig {
                name: "synth".into(),
                plugin: "Vital".into(),
                path: Some("plugins/synth.vst3".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            },
            InstrumentConfig {
                name: "snare".into(),
                plugin: String::new(),
                path: Some("drums/snare.wav".into()),
                r#type: InstrumentType::Wav,
                volume: 1.0,
            },
        ];

        assert_eq!(
            collect_samples(&instruments, std::path::Path::new("/project"))
                .unwrap()
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
                plugin: "Vital".into(),
                path: Some("synth.vst3".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            ..LuaState::default()
        };
        state.chord_sets.push(ChordSet {
            instrument_name: "synth".into(),
            at: 2.0,
            times: Some(2.0),
            cache_key: None,
            chords: vec![
                Chord {
                    pitches: vec!["c4".into(), "e4".into()],
                    duration: Duration {
                        length: 1.0,
                        play_length: Some(0.5),
                    },
                    effect_fn: None,
                },
                Chord {
                    pitches: Vec::new(),
                    duration: Duration {
                        length: 1.0,
                        play_length: Some(1.0),
                    },
                    effect_fn: None,
                },
                Chord {
                    pitches: vec!["g4".into()],
                    duration: Duration {
                        length: 2.0,
                        play_length: Some(2.0),
                    },
                    effect_fn: None,
                },
            ],
        });

        let (score, duration, effects, _) = sequence_score_from_bar(&state, 1).unwrap();
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
    fn groups_repetitions_into_reusable_patterns_from_later_bars() {
        let state = LuaState {
            default_bpm: 120.0,
            default_meter: 1.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                plugin: "Vital".into(),
                path: Some("synth.vst3".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 1.0,
                times: Some(3.0),
                cache_key: Some("pattern-key".into()),
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        play_length: Some(1.0),
                    },
                    effect_fn: None,
                }],
            }],
        };

        let (score, _, _, patterns) = sequence_score_from_bar(&state, 2).unwrap();
        assert!(score.is_empty());
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].key, "pattern-key");
        assert_eq!(patterns[0].start_seconds, vec![0.0, 0.5]);
        assert_eq!(patterns[0].duration_seconds, 0.5);
        assert!(matches!(
            patterns[0].score.as_slice(),
            [
                ScoreEvent::NoteOn {
                    at_seconds: 0.0,
                    note: 60,
                    ..
                },
                ScoreEvent::NoteOff {
                    at_seconds: 0.5,
                    note: 60,
                    ..
                }
            ]
        ));
    }

    #[test]
    fn starts_from_selected_bar_omitting_earlier_notes_and_shifting_timeline() {
        let lua = mlua::Lua::new();
        let effect_fn: mlua::Function = lua.load("return function() end").eval().unwrap();
        let state = LuaState {
            default_bpm: 120.0,
            default_meter: 4.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                plugin: "Vital".into(),
                path: Some("synth.vital".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            chord_sets: vec![
                ChordSet {
                    instrument_name: "synth".into(),
                    at: 1.0,
                    times: None,
                    cache_key: None,
                    chords: vec![Chord {
                        pitches: vec!["c4".into()],
                        duration: Duration {
                            length: 4.0,
                            play_length: Some(8.0),
                        },
                        effect_fn: Some(effect_fn.clone()),
                    }],
                },
                ChordSet {
                    instrument_name: "synth".into(),
                    at: 2.0,
                    times: None,
                    cache_key: None,
                    chords: vec![Chord {
                        pitches: vec!["d4".into()],
                        duration: Duration {
                            length: 1.0,
                            play_length: Some(1.0),
                        },
                        effect_fn: Some(effect_fn),
                    }],
                },
            ],
        };

        let (score, duration, effects, _) = sequence_score_from_bar(&state, 2).unwrap();
        assert_eq!(duration, 0.5);
        assert_eq!(score.len(), 2);
        assert!(matches!(
            score[0],
            ScoreEvent::NoteOn {
                at_seconds: 0.0,
                note: 62,
                ..
            }
        ));
        assert!(matches!(
            score[1],
            ScoreEvent::NoteOff {
                at_seconds: 0.5,
                note: 62,
                ..
            }
        ));
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].start_seconds, 0.0);
        assert_eq!(effects[0].playing_end_seconds, 0.5);
    }

    #[test]
    fn rejects_zero_from_bar() {
        assert_eq!(
            sequence_score_from_bar(&LuaState::default(), 0)
                .unwrap_err()
                .to_string(),
            "from-bar must be a positive whole number"
        );
    }

    #[test]
    fn play_length_can_exceed_rhythmic_length_without_delaying_the_next_chord() {
        let state = LuaState {
            default_bpm: 120.0,
            default_meter: 4.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                plugin: "Vital".into(),
                path: Some("synth.vital".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 1.0,
                times: None,
                cache_key: None,
                chords: vec![
                    Chord {
                        pitches: vec!["c4".into()],
                        duration: Duration {
                            length: 0.5,
                            play_length: Some(1.5),
                        },
                        effect_fn: None,
                    },
                    Chord {
                        pitches: vec!["d4".into()],
                        duration: Duration {
                            length: 0.5,
                            play_length: Some(0.5),
                        },
                        effect_fn: None,
                    },
                ],
            }],
        };

        let (score, _, _, _) = sequence_score_from_bar(&state, 1).unwrap();
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
                plugin: "Vital".into(),
                path: Some("synth.vital".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 2.0,
                times: Some(2.0),
                cache_key: None,
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        play_length: Some(0.5),
                    },
                    effect_fn: Some(effect_fn),
                }],
            }],
        };

        let (_, _, effects, _) = sequence_score_from_bar(&state, 1).unwrap();
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
                    plugin: "Vital".into(),
                    path: Some("vital.vital".into()),
                    r#type: InstrumentType::Vst,
                    volume: 1.0,
                },
                InstrumentConfig {
                    name: "kick".into(),
                    plugin: String::new(),
                    path: Some("kick.wav".into()),
                    r#type: InstrumentType::Wav,
                    volume: 1.0,
                },
                InstrumentConfig {
                    name: "dexed".into(),
                    plugin: "Dexed".into(),
                    path: Some("dexed.dexed".into()),
                    r#type: InstrumentType::Vst,
                    volume: 1.0,
                },
            ],
            chord_sets: vec![
                ChordSet {
                    instrument_name: "vital".into(),
                    at: 1.0,
                    times: None,
                    cache_key: None,
                    chords: vec![Chord {
                        pitches: vec!["c4".into()],
                        duration: Duration {
                            length: 1.0,
                            play_length: Some(1.0),
                        },
                        effect_fn: None,
                    }],
                },
                ChordSet {
                    instrument_name: "dexed".into(),
                    at: 1.0,
                    times: None,
                    cache_key: None,
                    chords: vec![Chord {
                        pitches: vec!["d4".into()],
                        duration: Duration {
                            length: 1.0,
                            play_length: Some(1.0),
                        },
                        effect_fn: None,
                    }],
                },
            ],
            ..LuaState::default()
        };

        let (score, _, _, _) = sequence_score_from_bar(&state, 1).unwrap();
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
        assert!(sequence_score_from_bar(&state, 1).is_err());

        state.default_bpm = 120.0;
        state.default_meter = 0.0;
        assert!(sequence_score_from_bar(&state, 1).is_err());

        state.default_meter = 4.0;
        state.instruments.push(InstrumentConfig {
            name: "synth".into(),
            plugin: "Vital".into(),
            path: Some("synth.vst3".into()),
            r#type: InstrumentType::Vst,
            volume: 1.0,
        });
        state.chord_sets.push(ChordSet {
            instrument_name: "synth".into(),
            at: 1.0,
            times: None,
            cache_key: None,
            chords: vec![Chord {
                pitches: vec!["bs9".into()],
                duration: Duration {
                    length: 1.0,
                    play_length: Some(1.0),
                },
                effect_fn: None,
            }],
        });
        assert!(sequence_score_from_bar(&state, 1).is_err());
    }

    #[test]
    fn turns_x_pitches_into_events_for_the_named_wav_instrument() {
        let state = LuaState {
            instruments: vec![
                InstrumentConfig {
                    name: "kick".into(),
                    plugin: String::new(),
                    path: Some("kick.wav".into()),
                    r#type: InstrumentType::Wav,
                    volume: 1.0,
                },
                InstrumentConfig {
                    name: "snare".into(),
                    plugin: String::new(),
                    path: Some("snare.wav".into()),
                    r#type: InstrumentType::Wav,
                    volume: 1.0,
                },
            ],
            chord_sets: vec![ChordSet {
                instrument_name: "snare".into(),
                at: 2.0,
                times: None,
                cache_key: None,
                chords: vec![Chord {
                    pitches: vec!["x".into()],
                    duration: Duration {
                        length: 1.0,
                        play_length: Some(1.0),
                    },
                    effect_fn: None,
                }],
            }],
            ..LuaState::default()
        };

        let (score, duration, _, _) = sequence_score_from_bar(&state, 1).unwrap();
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
                cache_key: None,
                chords: Vec::new(),
            }],
            ..LuaState::default()
        };

        assert_eq!(
            sequence_score_from_bar(&state, 1).unwrap_err().to_string(),
            "chord set references unknown instrument 'missing'"
        );
    }

    #[test]
    fn zero_play_length_is_silent_but_advances_timeline() {
        let state = LuaState {
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                plugin: "Vital".into(),
                path: Some("synth.vst3".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 1.0,
                times: None,
                cache_key: None,
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        play_length: Some(0.0),
                    },
                    effect_fn: None,
                }],
            }],
            ..LuaState::default()
        };
        let (score, duration, _, _) = sequence_score_from_bar(&state, 1).unwrap();
        assert_eq!(duration, 0.5);
        assert!(!score.iter().any(|event| matches!(
            event,
            ScoreEvent::NoteOn { .. } | ScoreEvent::NoteOff { .. }
        )));
    }

    #[test]
    fn omitted_play_length_defaults_to_rhythmic_length() {
        let state = LuaState {
            default_bpm: 120.0,
            instruments: vec![InstrumentConfig {
                name: "synth".into(),
                plugin: "Vital".into(),
                path: Some("synth.vst3".into()),
                r#type: InstrumentType::Vst,
                volume: 1.0,
            }],
            chord_sets: vec![ChordSet {
                instrument_name: "synth".into(),
                at: 1.0,
                times: None,
                cache_key: None,
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        play_length: None,
                    },
                    effect_fn: None,
                }],
            }],
            ..LuaState::default()
        };

        let (score, _, _, _) = sequence_score_from_bar(&state, 1).unwrap();
        assert!(score.iter().any(|event| matches!(
            event,
            ScoreEvent::NoteOff {
                at_seconds: 0.5,
                note: 60,
                ..
            }
        )));
    }
}
