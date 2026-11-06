use std::collections::HashMap;
use std::{error::Error, fs};

use vst3_host::{
    Plugin, StateContext,
    audio::{AudioBuffers, BusDirection, MediaType, write_wav},
    midi::{MidiChannel, MidiEvent},
    simple,
};

use crate::renderer_input::{EffectSpan, RendererInput, ScoreEvent, TimedParameterValue};

const SAMPLE_RATE: f64 = 44_100.0;
const BLOCK_SIZE: usize = 512;

pub fn render(input: RendererInput) -> Result<(), Box<dyn Error>> {
    let _ = &input.config;
    let samples = input
        .samples
        .iter()
        .map(|sample| vst3_host::audio::read_wav(&sample.path))
        .collect::<vst3_host::Result<Vec<_>>>()?;
    let parameter_values = run_effects(
        &input.effects,
        SAMPLE_RATE,
        60.0 / input.config.default_bpm,
        &input.parameter_values,
        input.initial_parameter_values.len(),
    )?;
    let mut rendered_plugins = Vec::with_capacity(input.vst.len());
    for (vst_index, vst) in input.vst.iter().enumerate() {
        let mut plugin =
            simple::load_plugin_with_settings(&vst.plugin_path, SAMPLE_RATE, BLOCK_SIZE)?;
        let preset = fs::read(&vst.preset_path)?;
        plugin
            .load_state_with_context(&preset, &StateContext::preset_from_path(&vst.preset_path))?;
        for value in input
            .initial_parameter_values
            .iter()
            .filter(|value| value.instrument_name == vst.name)
        {
            plugin.set_parameter_by_name(&value.parameter_name, value.value)?;
        }
        rendered_plugins.push(render_score(
            &mut plugin,
            vst_index,
            &vst.name,
            &parameter_values,
            &input,
            &samples,
        )?);
    }

    let output_channels = rendered_plugins
        .iter()
        .map(Vec::len)
        .max()
        .ok_or("no VST instrument configured")?;
    let total_frames = rendered_plugins
        .iter()
        .flat_map(|channels| channels.iter().map(Vec::len))
        .max()
        .unwrap_or(0);
    let mut channels = vec![vec![0.0; total_frames]; output_channels];
    for plugin_channels in rendered_plugins {
        mix_channels(&mut channels, &plugin_channels);
    }
    for (sample_index, (sample, sample_rate)) in samples.iter().enumerate() {
        mix_samples(
            &mut channels,
            sample,
            *sample_rate,
            SAMPLE_RATE,
            &input.score,
            sample_index,
        )?;
    }
    write_wav(&input.output_path, &channels, SAMPLE_RATE as u32)?;
    println!("Wrote {}", input.output_path.display());
    Ok(())
}

fn run_effects(
    effects: &[EffectSpan],
    sample_rate: f64,
    seconds_per_quarter: f64,
    parameter_values: &std::cell::RefCell<Vec<crate::luaapi::ParameterValue>>,
    initial_count: usize,
) -> Result<Vec<TimedParameterValue>, Box<dyn Error>> {
    if effects.is_empty() {
        return Ok(Vec::new());
    }

    let mut scheduled = effects
        .iter()
        .map(|effect| {
            (
                ((effect.start_seconds - 1.0) * sample_rate).ceil() as i64,
                (effect.playing_end_seconds * sample_rate).ceil() as i64,
                effect,
            )
        })
        .collect::<Vec<_>>();
    scheduled.sort_by_key(|(start_frame, _, _)| *start_frame);

    let mut active = Vec::new();
    let mut next = 0;
    let mut next_parameter_value = initial_count;
    let mut timed_values = Vec::new();
    let first_frame = scheduled[0].0;
    let last_frame = scheduled
        .iter()
        .map(|(_, end_frame, _)| *end_frame)
        .max()
        .unwrap_or(first_frame);

    for frame in first_frame..=last_frame {
        while next < scheduled.len() && scheduled[next].0 <= frame {
            active.push(next);
            next += 1;
        }

        let time_seconds = frame as f64 / sample_rate;
        for &index in &active {
            let (_, end_frame, effect) = scheduled[index];
            let playing = time_seconds >= effect.start_seconds
                && frame < end_frame
                && time_seconds < effect.playing_end_seconds;
            effect.effect_fn.call::<()>((
                (time_seconds - effect.start_seconds) / seconds_per_quarter,
                playing,
            ))?;
        }
        let current_values = parameter_values.borrow();
        for value in &current_values[next_parameter_value..] {
            timed_values.push(TimedParameterValue {
                frame: frame.max(0),
                value: value.clone(),
            });
        }
        next_parameter_value = current_values.len();
        active.retain(|&index| frame < scheduled[index].1);
    }

    Ok(timed_values)
}

fn render_score(
    plugin: &mut Plugin,
    vst_index: usize,
    instrument_name: &str,
    parameter_values: &[TimedParameterValue],
    input: &RendererInput,
    samples: &[(Vec<Vec<f32>>, u32)],
) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let layout = plugin.audio_bus_layout()?;
    for (index, bus) in layout.outputs.iter().enumerate() {
        if !bus.active {
            plugin.set_bus_active(MediaType::Audio, BusDirection::Output, index as i32, true)?;
        }
    }

    let output_sample_rate = plugin.sample_rate();
    let output_channels = plugin.output_channel_count();
    if output_channels == 0 {
        return Err("plugin has no active audio output channels".into());
    }
    let block_size = plugin.block_size().max(1);
    let note_off_frames = input
        .score
        .iter()
        .filter_map(|event| match event {
            ScoreEvent::NoteOff {
                at_seconds,
                vst_index: event_vst_index,
                ..
            } if *event_vst_index == vst_index => {
                Some((*at_seconds * output_sample_rate).ceil() as usize)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let mut parameter_ids = HashMap::new();
    for timed_value in parameter_values
        .iter()
        .filter(|timed_value| timed_value.value.instrument_name == instrument_name)
    {
        if !parameter_ids.contains_key(&timed_value.value.parameter_name) {
            let parameter = plugin.find_parameter(&timed_value.value.parameter_name)?;
            parameter_ids.insert(timed_value.value.parameter_name.clone(), parameter.id);
        }
    }
    let sample_end_frames = input
        .score
        .iter()
        .filter_map(|event| match event {
            ScoreEvent::PlaySample {
                at_seconds,
                sample_index,
            } => samples.get(*sample_index).map(|(sample, sample_rate)| {
                let frames = sample.iter().map(Vec::len).min().unwrap_or(0) as f64;
                let duration = frames / (*sample_rate).max(1) as f64;
                ((*at_seconds + duration) * output_sample_rate).ceil() as usize
            }),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let total_frames = ((input.render_duration_seconds * output_sample_rate).ceil() as usize)
        .max(note_off_frames)
        .max(sample_end_frames);

    plugin.start_processing()?;
    let render_result = (|| -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
        let mut channels = vec![Vec::with_capacity(total_frames); output_channels];
        let mut rendered = 0;
        while rendered < total_frames {
            let frames = block_size.min(total_frames - rendered);
            let block_end = rendered + frames;

            for event in &input.score {
                match *event {
                    ScoreEvent::NoteOn {
                        at_seconds,
                        vst_index: event_vst_index,
                        note,
                        velocity,
                    } if event_vst_index == vst_index => {
                        send_midi_event(
                            plugin,
                            event_frame(at_seconds, output_sample_rate),
                            rendered,
                            block_end,
                            MidiEvent::NoteOn {
                                channel: MidiChannel::Ch1,
                                note,
                                velocity,
                            },
                        )?;
                    }
                    ScoreEvent::NoteOff {
                        at_seconds,
                        vst_index: event_vst_index,
                        note,
                    } if event_vst_index == vst_index => {
                        send_midi_event(
                            plugin,
                            event_frame(at_seconds, output_sample_rate),
                            rendered,
                            block_end,
                            MidiEvent::NoteOff {
                                channel: MidiChannel::Ch1,
                                note,
                                velocity: 0,
                            },
                        )?;
                    }
                    ScoreEvent::NoteOn { .. }
                    | ScoreEvent::NoteOff { .. }
                    | ScoreEvent::PlaySample { .. } => {}
                }
            }

            for timed_value in parameter_values.iter().filter(|timed_value| {
                timed_value.value.instrument_name == instrument_name
                    && (rendered as i64..block_end as i64).contains(&timed_value.frame)
            }) {
                plugin.set_parameter_at(
                    parameter_ids[&timed_value.value.parameter_name],
                    timed_value.value.value,
                    (timed_value.frame - rendered as i64) as i32,
                )?;
            }

            let mut buffers = AudioBuffers::new(0, output_channels, frames, output_sample_rate);
            plugin.process_audio(&mut buffers)?;
            for (channel, samples) in channels.iter_mut().enumerate() {
                if let Some(output) = buffers.outputs.get(channel) {
                    samples.extend_from_slice(&output[..frames.min(output.len())]);
                }
            }
            rendered = block_end;
        }

        Ok(channels)
    })();
    let stop_result = plugin.stop_processing();

    let channels = render_result?;
    stop_result?;
    Ok(channels)
}

fn mix_channels(output: &mut [Vec<f32>], input: &[Vec<f32>]) {
    for (output_channel, output_samples) in output.iter_mut().enumerate() {
        let Some(input_samples) = input.get(if input.len() == 1 {
            0
        } else {
            output_channel.min(input.len().saturating_sub(1))
        }) else {
            continue;
        };
        for (output_sample, input_sample) in output_samples.iter_mut().zip(input_samples) {
            *output_sample += input_sample;
        }
    }
}

fn send_midi_event(
    plugin: &mut Plugin,
    frame: usize,
    block_start: usize,
    block_end: usize,
    event: MidiEvent,
) -> vst3_host::Result<()> {
    if block_start <= frame && frame < block_end {
        plugin.send_midi_event_at(event, (frame - block_start) as i32)?;
    }
    Ok(())
}

fn event_frame(at_seconds: f64, sample_rate: f64) -> usize {
    (at_seconds * sample_rate).ceil() as usize
}

fn mix_samples(
    output: &mut [Vec<f32>],
    sample: &[Vec<f32>],
    sample_rate: u32,
    output_sample_rate: f64,
    score: &[ScoreEvent],
    sample_index: usize,
) -> Result<(), Box<dyn Error>> {
    if sample_rate == 0 || !output_sample_rate.is_finite() || output_sample_rate <= 0.0 {
        return Err("sample rates must be greater than zero".into());
    }

    let Some(sample_frames) = sample.iter().map(Vec::len).min() else {
        return Ok(());
    };
    if sample_frames == 0 {
        return Ok(());
    }

    let rate_ratio = sample_rate as f64 / output_sample_rate;
    for event in score {
        let ScoreEvent::PlaySample {
            at_seconds,
            sample_index: event_sample_index,
        } = event
        else {
            continue;
        };
        if *event_sample_index != sample_index {
            continue;
        }
        let start_frame = event_frame(*at_seconds, output_sample_rate);
        for (output_channel, output_samples) in output.iter_mut().enumerate() {
            let sample_channel = if sample.len() == 1 {
                0
            } else {
                output_channel.min(sample.len() - 1)
            };
            let sample_samples = &sample[sample_channel];

            for (frame, output_sample) in output_samples.iter_mut().enumerate().skip(start_frame) {
                let source_position = (frame - start_frame) as f64 * rate_ratio;
                let source_frame = source_position.floor() as usize;
                if source_frame >= sample_frames {
                    break;
                }

                let next_frame = (source_frame + 1).min(sample_frames - 1);
                let fraction = (source_position - source_frame as f64) as f32;
                *output_sample += sample_samples[source_frame] * (1.0 - fraction)
                    + sample_samples[next_frame] * fraction;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::{mix_samples, run_effects};
    use crate::renderer_input::{EffectSpan, ScoreEvent};

    #[test]
    fn evaluates_effect_each_sample_and_calls_once_after_gate() {
        let lua = Lua::new();
        let effect_fn = lua
            .load("events = {}; return function(t, playing) table.insert(events, {t, playing}) end")
            .eval()
            .unwrap();
        let effects = [EffectSpan {
            start_seconds: 0.5,
            playing_end_seconds: 1.0,
            effect_fn,
        }];

        run_effects(&effects, 2.0, 0.5, &std::cell::RefCell::new(Vec::new()), 0).unwrap();

        let events: mlua::Table = lua.globals().get("events").unwrap();
        let calls = events
            .sequence_values::<mlua::Table>()
            .map(|call| {
                let call = call.unwrap();
                (
                    call.raw_get::<f64>(1).unwrap(),
                    call.raw_get::<bool>(2).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            vec![(-2.0, false), (-1.0, false), (0.0, true), (1.0, false)]
        );
    }

    #[test]
    fn captures_per_sample_parameter_updates_from_effect_callbacks() {
        let lua = Lua::new();
        let parameter_values = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let callback_values = std::rc::Rc::clone(&parameter_values);
        let effect_fn = lua
            .create_function(move |_, (_t, playing): (f64, bool)| {
                callback_values
                    .borrow_mut()
                    .push(crate::luaapi::ParameterValue {
                        instrument_name: "synth".into(),
                        parameter_name: "cutoff".into(),
                        value: if playing { 0.8 } else { 0.1 },
                    });
                Ok(())
            })
            .unwrap();
        let effects = [EffectSpan {
            start_seconds: 0.5,
            playing_end_seconds: 1.0,
            effect_fn,
        }];

        let timed_values = run_effects(&effects, 2.0, 0.5, &parameter_values, 0).unwrap();
        assert_eq!(
            timed_values
                .iter()
                .map(|timed| (timed.frame, timed.value.value))
                .collect::<Vec<_>>(),
            vec![(0, 0.1), (0, 0.1), (1, 0.8), (2, 0.1)]
        );
    }

    #[test]
    fn mixes_every_configured_sample_at_each_sample_event() {
        let score = [
            ScoreEvent::PlaySample {
                at_seconds: 0.0,
                sample_index: 0,
            },
            ScoreEvent::PlaySample {
                at_seconds: 0.0,
                sample_index: 1,
            },
        ];
        let mut output = vec![vec![0.0; 2]];
        let samples = [(vec![vec![0.25, 0.25]], 1), (vec![vec![0.5, 0.5]], 1)];

        for (sample_index, (sample, sample_rate)) in samples.into_iter().enumerate() {
            mix_samples(&mut output, &sample, sample_rate, 1.0, &score, sample_index).unwrap();
        }

        assert_eq!(output, vec![vec![0.75, 0.75]]);
    }

    #[test]
    fn mixes_only_the_sample_selected_by_an_event() {
        let score = [ScoreEvent::PlaySample {
            at_seconds: 0.0,
            sample_index: 0,
        }];
        let mut output = vec![vec![0.0; 1]];

        mix_samples(&mut output, &[vec![0.25]], 1, 1.0, &score, 1).unwrap();

        assert_eq!(output, vec![vec![0.0]]);
    }
}
