use std::path::PathBuf;

use mlua::{Function, Lua};

use crate::luaapi::{LuaState, ParameterValue};

pub struct RendererInput {
    pub output_path: PathBuf,
    pub vst: Vec<VstInstrument>,
    pub samples: Vec<SampleInstrument>,
    pub config: LuaState,
    pub render_duration_seconds: f64,
    pub score: Vec<ScoreEvent>,
    pub effects: Vec<EffectSpan>,
    pub initial_parameter_values: Vec<ParameterValue>,
    pub(crate) parameter_values: std::rc::Rc<std::cell::RefCell<Vec<ParameterValue>>>,
    pub(crate) _lua: Lua,
}

#[derive(Debug)]
pub struct EffectSpan {
    pub start_seconds: f64,
    pub playing_end_seconds: f64,
    pub effect_fn: Function,
}

#[derive(Clone, Debug)]
pub struct TimedParameterValue {
    pub frame: i64,
    pub value: ParameterValue,
}

#[derive(Debug)]
pub struct VstInstrument {
    pub name: String,
    pub plugin_path: PathBuf,
    pub preset_path: PathBuf,
    pub volume: f64,
}

#[derive(Debug)]
pub struct SampleInstrument {
    pub path: PathBuf,
    pub volume: f64,
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
