use std::path::PathBuf;

use crate::luaapi::LuaState;

#[derive(Debug)]
pub struct RendererInput {
    pub output_path: PathBuf,
    pub vst: VstInstrument,
    pub sample: SampleInstrument,
    pub config: LuaState,
    pub render_duration_seconds: f64,
    pub score: Vec<ScoreEvent>,
}

#[derive(Debug)]
pub struct VstInstrument {
    pub plugin_path: PathBuf,
    pub preset_path: PathBuf,
}

#[derive(Debug)]
pub struct SampleInstrument {
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug)]
pub enum ScoreEvent {
    NoteOn {
        at_seconds: f64,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        at_seconds: f64,
        note: u8,
    },
    PlaySample {
        at_seconds: f64,
    },
}
