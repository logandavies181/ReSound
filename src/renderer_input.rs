use std::path::PathBuf;

use mlua::{Function, Lua};

use crate::luaapi::LuaState;

pub struct RendererInput {
    pub output_path: PathBuf,
    pub vst: Vec<VstInstrument>,
    pub samples: Vec<SampleInstrument>,
    pub config: LuaState,
    pub render_duration_seconds: f64,
    pub score: Vec<ScoreEvent>,
    pub effects: Vec<EffectSpan>,
    pub(crate) _lua: Lua,
}

#[derive(Debug)]
pub struct EffectSpan {
    pub start_seconds: f64,
    pub playing_end_seconds: f64,
    pub effect_fn: Function,
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
        vst_index: usize,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        at_seconds: f64,
        vst_index: usize,
        note: u8,
    },
    PlaySample {
        at_seconds: f64,
        sample_index: usize,
    },
}
