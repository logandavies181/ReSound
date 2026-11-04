use std::{error::Error, fs};

use vst3_host::{
    Plugin, StateContext,
    audio::{AudioBuffers, BusDirection, MediaType, write_wav},
    midi::{MidiChannel, MidiEvent},
    simple,
};

use crate::renderer_input::{RendererInput, ScoreEvent};

const SAMPLE_RATE: f64 = 44_100.0;
const BLOCK_SIZE: usize = 512;

pub fn render(input: RendererInput) -> Result<(), Box<dyn Error>> {
    let _ = &input.config;
    let samples = input
        .samples
        .iter()
        .map(|sample| vst3_host::audio::read_wav(&sample.path))
        .collect::<vst3_host::Result<Vec<_>>>()?;
    let mut plugin =
        simple::load_plugin_with_settings(&input.vst.plugin_path, SAMPLE_RATE, BLOCK_SIZE)?;
    let preset = fs::read(&input.vst.preset_path)?;
    plugin.load_state_with_context(
        &preset,
        &StateContext::preset_from_path(&input.vst.preset_path),
    )?;

    render_score(&mut plugin, &input, &samples)?;
    println!("Wrote {}", input.output_path.display());
    Ok(())
}

fn render_score(
    plugin: &mut Plugin,
    input: &RendererInput,
    samples: &[(Vec<Vec<f32>>, u32)],
) -> Result<(), Box<dyn Error>> {
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
            ScoreEvent::NoteOff { at_seconds, .. } => {
                Some((*at_seconds * output_sample_rate).ceil() as usize)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let sample_end_frames = input
        .score
        .iter()
        .filter_map(|event| match event {
            ScoreEvent::PlaySample { at_seconds } => Some(
                samples
                    .iter()
                    .map(|(sample, sample_rate)| {
                        let frames = sample.iter().map(Vec::len).min().unwrap_or(0) as f64;
                        let duration = frames / (*sample_rate).max(1) as f64;
                        ((*at_seconds + duration) * output_sample_rate).ceil() as usize
                    })
                    .max()
                    .unwrap_or(0),
            ),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let total_frames = ((input.render_duration_seconds * output_sample_rate).ceil() as usize)
        .max(note_off_frames)
        .max(sample_end_frames);

    plugin.start_processing()?;
    let render_result = (|| -> Result<(), Box<dyn Error>> {
        let mut channels = vec![Vec::with_capacity(total_frames); output_channels];
        let mut rendered = 0;
        while rendered < total_frames {
            let frames = block_size.min(total_frames - rendered);
            let block_end = rendered + frames;

            for event in &input.score {
                match *event {
                    ScoreEvent::NoteOn {
                        at_seconds,
                        note,
                        velocity,
                    } => {
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
                    ScoreEvent::NoteOff { at_seconds, note } => {
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
                    ScoreEvent::PlaySample { .. } => {}
                }
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

        for (sample, sample_rate) in samples {
            mix_samples(
                &mut channels,
                sample,
                *sample_rate,
                output_sample_rate,
                &input.score,
            )?;
        }
        write_wav(&input.output_path, &channels, output_sample_rate as u32)?;
        Ok(())
    })();
    let stop_result = plugin.stop_processing();

    render_result?;
    stop_result?;
    Ok(())
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
        let ScoreEvent::PlaySample { at_seconds } = event else {
            continue;
        };
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
    use super::mix_samples;
    use crate::renderer_input::ScoreEvent;

    #[test]
    fn mixes_every_configured_sample_at_each_sample_event() {
        let score = [ScoreEvent::PlaySample { at_seconds: 0.0 }];
        let mut output = vec![vec![0.0; 2]];
        let samples = [(vec![vec![0.25, 0.25]], 1), (vec![vec![0.5, 0.5]], 1)];

        for (sample, sample_rate) in samples {
            mix_samples(&mut output, &sample, sample_rate, 1.0, &score).unwrap();
        }

        assert_eq!(output, vec![vec![0.75, 0.75]]);
    }
}
