use std::path::Path;

use crate::{
    luaapi::LuaState,
    renderer_input::{RendererInput, SampleInstrument, ScoreEvent, VstInstrument},
};

const VST_PLUGIN_PATH: &str = "/usr/lib/vst3/Vital.vst3";
const VST_PRESET_PATH: &str = "test.vital";
const SAMPLE_PATH: &str = "snare.wav";
const NOTE_LENGTH_SECONDS: f64 = 2.0;
const RENDER_LENGTH_SECONDS: f64 = 4.0;

pub fn sequence(
    state: LuaState,
    project_dir: &Path,
    output_path: impl Into<std::path::PathBuf>,
) -> RendererInput {
    RendererInput {
        output_path: output_path.into(),
        vst: VstInstrument {
            plugin_path: VST_PLUGIN_PATH.into(),
            preset_path: project_dir.join(VST_PRESET_PATH),
        },
        sample: SampleInstrument {
            path: project_dir.join(SAMPLE_PATH),
        },
        instrument_config: state.instrument_config,
        render_duration_seconds: RENDER_LENGTH_SECONDS,
        score: vec![
            ScoreEvent::NoteOn {
                at_seconds: 0.0,
                note: 60,
                velocity: 100,
            },
            ScoreEvent::NoteOff {
                at_seconds: NOTE_LENGTH_SECONDS,
                note: 60,
            },
            ScoreEvent::PlaySample { at_seconds: 0.0 },
        ],
    }
}
