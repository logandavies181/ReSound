use std::path::Path;

use crate::{
    luaapi::{Chord, LuaState},
    renderer_input::{RendererInput, SampleInstrument, ScoreEvent, VstInstrument},
};

const VST_PLUGIN_PATH: &str = "/usr/lib/vst3/Vital.vst3";
const VST_PRESET_PATH: &str = "test.vital";
const SAMPLE_PATH: &str = "snare.wav";
const VELOCITY: u8 = 100;

pub fn sequence(
    state: LuaState,
    project_dir: &Path,
    output_path: impl Into<std::path::PathBuf>,
) -> Result<RendererInput, Box<dyn std::error::Error>> {
    let (score, score_duration) = sequence_score(&state)?;

    Ok(RendererInput {
        output_path: output_path.into(),
        vst: VstInstrument {
            plugin_path: VST_PLUGIN_PATH.into(),
            preset_path: project_dir.join(VST_PRESET_PATH),
        },
        sample: SampleInstrument {
            path: project_dir.join(SAMPLE_PATH),
        },
        config: state,
        render_duration_seconds: score_duration,
        score,
    })
}

fn sequence_score(state: &LuaState) -> Result<(Vec<ScoreEvent>, f64), Box<dyn std::error::Error>> {
    if !state.default_bpm.is_finite() || state.default_bpm <= 0.0 {
        return Err("default BPM must be finite and greater than zero".into());
    }
    if !state.default_meter.is_finite() || state.default_meter <= 0.0 {
        return Err("default meter must be finite and greater than zero".into());
    }

    let seconds_per_quarter = 60.0 / state.default_bpm;
    let seconds_per_bar = state.default_meter * seconds_per_quarter;
    let mut score = vec![ScoreEvent::PlaySample { at_seconds: 0.0 }];
    let mut score_duration: f64 = 0.0;

    for chord_set in &state.chord_sets {
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
                if chord.duration.ratio > 0.0 {
                    for pitch in &chord.pitches {
                        let note = pitch_to_midi(pitch)?;
                        score.push(ScoreEvent::NoteOn {
                            at_seconds,
                            note,
                            velocity: VELOCITY,
                        });
                        score.push(ScoreEvent::NoteOff {
                            at_seconds: note_off_seconds,
                            note,
                        });
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

    Ok((score, score_duration))
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
    if !duration.ratio.is_finite() || !(0.0..=1.0).contains(&duration.ratio) {
        return Err(format!(
            "chord duration ratio must be between zero and one, got {}",
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
        ScoreEvent::PlaySample { at_seconds } => (*at_seconds, 2),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::luaapi::{Chord, ChordSet, Duration, LuaState};

    use super::{pitch_to_midi, sequence, sequence_score};
    use crate::renderer_input::ScoreEvent;

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
            ..LuaState::default()
        };
        state.chord_sets.push(ChordSet {
            at: 2.0,
            times: Some(2.0),
            chords: vec![
                Chord {
                    pitches: vec!["c4".into(), "e4".into()],
                    duration: Duration {
                        length: 1.0,
                        ratio: 0.5,
                    },
                },
                Chord {
                    pitches: Vec::new(),
                    duration: Duration {
                        length: 1.0,
                        ratio: 1.0,
                    },
                },
                Chord {
                    pitches: vec!["g4".into()],
                    duration: Duration {
                        length: 2.0,
                        ratio: 1.0,
                    },
                },
            ],
        });

        let (score, duration) = sequence_score(&state).unwrap();
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
                note: 60
            }
        )));

        let input = sequence(state, Path::new("."), "out.wav").unwrap();
        assert_eq!(input.render_duration_seconds, 6.0);
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
        state.chord_sets.push(ChordSet {
            at: 1.0,
            times: None,
            chords: vec![Chord {
                pitches: vec!["bs9".into()],
                duration: Duration {
                    length: 1.0,
                    ratio: 1.0,
                },
            }],
        });
        assert!(sequence_score(&state).is_err());
    }

    #[test]
    fn zero_gate_ratio_is_silent_but_advances_timeline() {
        let state = LuaState {
            chord_sets: vec![ChordSet {
                at: 1.0,
                times: None,
                chords: vec![Chord {
                    pitches: vec!["c4".into()],
                    duration: Duration {
                        length: 1.0,
                        ratio: 0.0,
                    },
                }],
            }],
            ..LuaState::default()
        };
        let (score, duration) = sequence_score(&state).unwrap();
        assert_eq!(duration, 0.5);
        assert!(!score.iter().any(|event| matches!(
            event,
            ScoreEvent::NoteOn { .. } | ScoreEvent::NoteOff { .. }
        )));
    }
}
