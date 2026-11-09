use std::{
    cell::RefCell,
    error::Error,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc,
};

use mlua::{Function, Lua, Table};

mod teal_lib {
    include!(concat!(env!("OUT_DIR"), "/teal_lib.rs"));
}

#[derive(Clone, Debug, PartialEq)]
pub struct LuaState {
    pub instruments: Vec<InstrumentConfig>,
    pub default_bpm: f64,
    pub default_meter: f64,
    pub chord_sets: Vec<ChordSet>,
}

pub struct LuaExecution {
    pub state: LuaState,
    pub initial_parameter_values: Vec<ParameterValue>,
    pub(crate) parameter_values: Rc<RefCell<Vec<ParameterValue>>>,
    pub(crate) lua: Lua,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterValue {
    pub instrument_name: String,
    pub parameter_name: String,
    pub value: f64,
}

impl Deref for LuaExecution {
    type Target = LuaState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl std::fmt::Debug for LuaExecution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.state.fmt(formatter)
    }
}

impl PartialEq<LuaState> for LuaExecution {
    fn eq(&self, other: &LuaState) -> bool {
        self.state == *other
    }
}

impl Default for LuaState {
    fn default() -> Self {
        Self {
            instruments: Vec::new(),
            default_bpm: 120.0,
            default_meter: 4.0,
            chord_sets: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChordSet {
    pub instrument_name: String,
    pub chords: Vec<Chord>,
    pub at: f64,
    pub times: Option<f64>,
    pub cache_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub pitches: Vec<String>,
    pub duration: Duration,
    pub effect_fn: Option<Function>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duration {
    pub length: f64,
    pub play_length: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentConfig {
    pub name: String,
    pub plugin: String,
    pub path: Option<PathBuf>,
    pub r#type: InstrumentType,
    pub volume: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstrumentType {
    Wav,
    Vst,
}

pub fn execute_file(path: &Path) -> Result<LuaExecution, Box<dyn Error>> {
    let name = path.to_string_lossy().into_owned();
    let (source, teal_path) = if path.extension().is_some_and(|extension| extension == "tl") {
        let module_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("Teal script name must be valid UTF-8")?;
        #[allow(deprecated)]
        let compiler = tealr::embed_compiler!(Local());
        (compiler(module_name), path.parent())
    } else {
        (fs::read_to_string(path)?, None)
    };
    execute_source_with_path(&source, name, teal_path).map_err(Into::into)
}

#[allow(unused)]
fn execute_source(source: &str, name: String) -> mlua::Result<LuaExecution> {
    execute_source_with_path(source, name, None)
}

fn execute_source_with_path(
    source: &str,
    name: String,
    teal_path: Option<&Path>,
) -> mlua::Result<LuaExecution> {
    let lua = Lua::new();
    let state = Rc::new(RefCell::new(LuaState::default()));
    let parameter_values = Rc::new(RefCell::new(Vec::new()));
    install_module(&lua, Rc::clone(&state), Rc::clone(&parameter_values))?;

    if let Some(teal_path) = teal_path {
        let package: Table = lua.globals().get("package")?;
        let package_path: String = package.get("path")?;
        let teal_module_path = teal_path.join("?.tl");
        package.set(
            "path",
            format!("{};{package_path}", teal_module_path.display()),
        )?;
    }

    lua.load(source).set_name(name).exec()?;
    let state = state.borrow().clone();
    let initial_parameter_values = parameter_values.borrow().clone();

    Ok(LuaExecution {
        state,
        initial_parameter_values,
        parameter_values,
        lua,
    })
}

fn install_module(
    lua: &Lua,
    state: Rc<RefCell<LuaState>>,
    parameter_values: Rc<RefCell<Vec<ParameterValue>>>,
) -> mlua::Result<()> {
    install_pitch_globals(lua)?;
    let module = lua.create_table()?;
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("resound", module.clone())?;
    install_score_functions(lua)?;

    let instrument_state = Rc::clone(&state);
    module.set(
        "addInstrument",
        lua.create_function(move |lua, config: Table| {
            let name: String = config.raw_get("name")?;
            let plugin: String = config.raw_get("plugin")?;
            let preset = config
                .raw_get::<Option<String>>("preset")?
                .map(PathBuf::from);
            let volume = config.raw_get::<Option<f64>>("volume")?.unwrap_or(1.0);
            let mut instrument_state = instrument_state.borrow_mut();
            ensure_unique_name(&instrument_state.instruments, &name)?;
            instrument_state.instruments.push(InstrumentConfig {
                name,
                plugin,
                path: preset,
                r#type: InstrumentType::Vst,
                volume: validate_volume(volume, "instrument")?,
            });
            loaded_module(lua)
        })?,
    )?;

    let sample_state = Rc::clone(&state);
    module.set(
        "addSample",
        lua.create_function(move |lua, config: Table| {
            let name: String = config.raw_get("name")?;
            let path: String = config.raw_get("path")?;
            let volume = config.raw_get::<Option<f64>>("volume")?.unwrap_or(1.0);
            let mut instrument_state = sample_state.borrow_mut();
            ensure_unique_name(&instrument_state.instruments, &name)?;
            instrument_state.instruments.push(InstrumentConfig {
                name,
                plugin: String::new(),
                path: Some(PathBuf::from(path)),
                r#type: InstrumentType::Wav,
                volume: validate_volume(volume, "sample")?,
            });
            loaded_module(lua)
        })?,
    )?;

    let copied_state = Rc::clone(&state);
    module.set(
        "copyInstrument",
        lua.create_function(move |lua, (source_name, dest_name): (String, String)| {
            let mut instrument_state = copied_state.borrow_mut();
            let mut instrument = instrument_state
                .instruments
                .iter()
                .find(|instrument| instrument.name == source_name)
                .cloned()
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!("unknown instrument '{source_name}'"))
                })?;
            ensure_unique_name(&instrument_state.instruments, &dest_name)?;
            instrument.name = dest_name;
            instrument_state.instruments.push(instrument);
            loaded_module(lua)
        })?,
    )?;

    let bpm_state = Rc::clone(&state);
    module.set(
        "setDefaultBpm",
        lua.create_function(move |lua, bpm: f64| {
            bpm_state.borrow_mut().default_bpm = bpm;
            loaded_module(lua)
        })?,
    )?;

    let meter_state = Rc::clone(&state);
    module.set(
        "setDefaultMeter",
        lua.create_function(move |lua, bar_length: f64| {
            meter_state.borrow_mut().default_meter = bar_length;
            loaded_module(lua)
        })?,
    )?;

    let parameter_state = Rc::clone(&state);
    module.set(
        "setParamValue",
        lua.create_function(
            move |lua, (instrument_name, parameter_name, value): (String, String, f64)| {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err(mlua::Error::RuntimeError(format!(
                        "parameter value must be finite and between 0 and 1, got {value}"
                    )));
                }
                let instruments = &parameter_state.borrow().instruments;
                let instrument = instruments
                    .iter()
                    .find(|instrument| instrument.name == instrument_name)
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError(format!(
                            "parameter update references unknown instrument '{instrument_name}'"
                        ))
                    })?;
                if instrument.r#type != InstrumentType::Vst {
                    return Err(mlua::Error::RuntimeError(format!(
                        "instrument '{instrument_name}' is not a VST instrument"
                    )));
                }
                parameter_values.borrow_mut().push(ParameterValue {
                    instrument_name,
                    parameter_name,
                    value,
                });
                loaded_module(lua)
            },
        )?,
    )?;

    let chords_state = Rc::clone(&state);
    module.set(
        "pushChords",
        lua.create_function(
            move |lua, (instrument_name, chords, options): (String, Table, Table)| {
                let chords = chords
                    .sequence_values::<Table>()
                    .map(|chord| parse_chord(&chord?))
                    .collect::<mlua::Result<Vec<_>>>()?;
                let at = options.raw_get("at")?;
                let times = options.raw_get("times")?;
                let cache_key = create_cache_key(&instrument_name, &chords);
                chords_state.borrow_mut().chord_sets.push(ChordSet {
                    instrument_name,
                    chords,
                    at,
                    times,
                    cache_key,
                });
                loaded_module(lua)
            },
        )?,
    )?;

    Ok(())
}

fn create_cache_key(instrument_name: &str, chords: &[Chord]) -> Option<String> {
    if chords.iter().any(|chord| chord.effect_fn.is_some()) {
        return None;
    }

    let mut key = format!(
        "v1|{}:{instrument_name}|{}",
        instrument_name.len(),
        chords.len()
    );
    for chord in chords {
        key.push_str(&format!(
            "|{}:{}:{:016x}:",
            chord.pitches.len(),
            chord.duration.length.to_bits(),
            chord.duration.play_length.map(f64::to_bits).unwrap_or(0)
        ));
        if chord.duration.play_length.is_some() {
            key.push('1');
        } else {
            key.push('0');
        }
        for pitch in &chord.pitches {
            key.push_str(&format!("|{}:{pitch}", pitch.len()));
        }
    }

    Some(key)
}

fn install_pitch_globals(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    for pitch in pitch_names() {
        globals.set(pitch.as_str(), pitch.as_str())?;
    }

    Ok(())
}

fn install_score_functions(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set(
        "n",
        lua.create_function(
            |lua, (pitch, duration, effect_fn): (String, Table, Option<Function>)| {
                create_chord(lua, vec![validate_pitch(&pitch)?], &duration, effect_fn)
            },
        )?,
    )?;
    globals.set(
        "o",
        lua.create_function(
            |lua, (pitch, duration, effect_fn): (String, Table, Option<Function>)| {
                let pitch = octave_above(&pitch)?;
                create_chord(lua, vec![pitch], &duration, effect_fn)
            },
        )?,
    )?;
    globals.set(
        "c",
        lua.create_function(
            |lua, (pitches, duration, effect_fn): (Table, Table, Option<Function>)| {
                let pitches = pitches
                    .sequence_values::<String>()
                    .map(|pitch| validate_pitch(&pitch?))
                    .collect::<mlua::Result<Vec<_>>>()?;
                create_chord(lua, pitches, &duration, effect_fn)
            },
        )?,
    )?;
    globals.set(
        "p",
        lua.create_function(|lua, (duration, effect_fn): (Table, Option<Function>)| {
            create_chord(lua, vec!["x".to_owned()], &duration, effect_fn)
        })?,
    )?;
    globals.set(
        "r",
        lua.create_function(|lua, (duration, effect_fn): (Table, Option<Function>)| {
            create_chord(lua, Vec::new(), &duration, effect_fn)
        })?,
    )?;

    lua.load(teal_lib::SOURCE).exec()
}

fn create_chord(
    lua: &Lua,
    pitches: Vec<String>,
    tuple: &Table,
    effect_fn: Option<Function>,
) -> mlua::Result<Table> {
    let length: f64 = tuple.raw_get(1)?;
    let play_length: Option<f64> = tuple.raw_get(2)?;
    let pitches_table = lua.create_sequence_from(pitches)?;
    let duration = lua.create_table()?;
    duration.set("length", length)?;
    duration.set("playLength", play_length)?;
    let chord = lua.create_table()?;
    chord.set("pitches", pitches_table)?;
    chord.set("duration", duration)?;
    chord.set("effectFn", effect_fn)?;
    Ok(chord)
}

fn parse_chord(chord: &Table) -> mlua::Result<Chord> {
    let pitches = chord
        .raw_get::<Table>("pitches")?
        .sequence_values::<String>()
        .collect::<mlua::Result<Vec<_>>>()?;
    let duration = chord.raw_get::<Table>("duration")?;
    Ok(Chord {
        pitches,
        duration: Duration {
            length: duration.raw_get("length")?,
            play_length: duration.raw_get("playLength")?,
        },
        effect_fn: chord.raw_get("effectFn")?,
    })
}

fn validate_pitch(pitch: &str) -> mlua::Result<String> {
    if pitch == "x" || pitch_names().any(|name| name == pitch) {
        Ok(pitch.to_owned())
    } else {
        Err(mlua::Error::RuntimeError(format!(
            "unknown pitch name: {pitch}"
        )))
    }
}

fn octave_above(pitch: &str) -> mlua::Result<String> {
    let pitch = validate_pitch(pitch)?;
    let split = pitch
        .find(|character: char| character.is_ascii_digit())
        .expect("validated pitch has an octave");
    let octave: u8 = pitch[split..]
        .parse()
        .expect("validated pitch has a numeric octave");
    if octave == 9 {
        return Err(mlua::Error::RuntimeError(format!(
            "cannot raise {pitch} by an octave: pitch range ends at octave 9"
        )));
    }
    Ok(format!(
        "{}{next_octave}",
        &pitch[..split],
        next_octave = octave + 1
    ))
}

pub(crate) fn pitch_names() -> impl Iterator<Item = String> {
    const PITCH_CLASSES: [&str; 21] = [
        "cb", "c", "cs", "db", "d", "ds", "eb", "e", "es", "fb", "f", "fs", "gb", "g", "gs", "ab",
        "a", "as", "bb", "b", "bs",
    ];

    std::iter::once("x".to_owned()).chain((0..=9).flat_map(|octave| {
        PITCH_CLASSES
            .into_iter()
            .map(move |pitch_class| format!("{pitch_class}{octave}"))
    }))
}

fn loaded_module(lua: &Lua) -> mlua::Result<Table> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.get("resound")
}

fn validate_volume(volume: f64, kind: &str) -> mlua::Result<f64> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(mlua::Error::RuntimeError(format!(
            "{kind} volume must be finite and between 0 and 1, got {volume}"
        )));
    }
    Ok(volume)
}

fn ensure_unique_name(instruments: &[InstrumentConfig], name: &str) -> mlua::Result<()> {
    if instruments.iter().any(|instrument| instrument.name == name) {
        return Err(mlua::Error::RuntimeError(format!(
            "instrument name '{name}' is already configured"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{InstrumentType, LuaState, execute_file, execute_source};

    #[test]
    fn exposes_specified_chaining_api_and_collects_config() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.addSample({name = "drums", path = "samples/drums.wav", volume = 0})
                    .addInstrument({
                        name = "synth",
                        plugin = "Vital",
                        preset = "presets/lead.vital",
                        volume = 0.6,
                    })
                    .setDefaultBpm(132)
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(
            state,
            LuaState {
                instruments: vec![
                    super::InstrumentConfig {
                        name: "drums".into(),
                        plugin: String::new(),
                        path: Some("samples/drums.wav".into()),
                        r#type: InstrumentType::Wav,
                        volume: 0.0,
                    },
                    super::InstrumentConfig {
                        name: "synth".into(),
                        plugin: "Vital".into(),
                        path: Some("presets/lead.vital".into()),
                        r#type: InstrumentType::Vst,
                        volume: 0.6,
                    },
                ],
                default_bpm: 132.0,
                default_meter: 4.0,
                chord_sets: Vec::new(),
            }
        );
    }

    #[test]
    fn defaults_missing_instrument_and_sample_volumes_to_one() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "lead",
                    plugin = "Vital",
                    preset = "lead.vital",
                }).addSample({name = "kick", path = "kick.wav"})
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.instruments[0].volume, 1.0);
        assert_eq!(state.instruments[1].volume, 1.0);
    }

    #[test]
    fn allows_vst_instrument_without_a_preset() {
        let state = execute_source(
            r#"
                require("resound").addInstrument({
                    name = "default_synth",
                    plugin = "Vital",
                })
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.instruments[0].path, None);
    }

    #[test]
    fn rejects_invalid_instrument_volume() {
        for volume in ["-0.1", "1.1", "math.huge"] {
            let source = format!(
                r#"
                    local resound = require("resound")
                    resound.addInstrument({{
                        name = "synth",
                        plugin = "Vital",
                        preset = "synth.vital",
                        volume = {volume},
                    }})
                "#
            );
            assert!(execute_source(&source, "test.lua".into()).is_err());
        }
    }

    #[test]
    fn uses_explicit_plugin_and_allows_any_preset_extension() {
        let error = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "synth",
                    plugin = "Configured Synth",
                    preset = "presets/patch.custom",
                    volume = 1,
                })
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(error.instruments[0].plugin, "Configured Synth");
        assert_eq!(
            error.instruments[0].path,
            Some(std::path::PathBuf::from("presets/patch.custom"))
        );
    }

    #[test]
    fn accepts_dexed_presets_as_vst_instruments() {
        let state = execute_source(
            r#"
                require("resound").addInstrument({
                    name = "synth",
                    plugin = "Dexed",
                    preset = "presets/synth.dexed",
                    volume = 1,
                })
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.instruments[0].r#type, InstrumentType::Vst);
        assert_eq!(state.instruments[0].plugin, "Dexed");
        assert_eq!(
            state.instruments[0].path,
            Some(std::path::PathBuf::from("presets/synth.dexed"))
        );
    }

    #[test]
    fn copies_instrument_settings_under_a_new_unique_name() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "lead",
                    plugin = "Vital",
                    preset = "presets/lead.vital",
                    volume = 0.6,
                }).copyInstrument("lead", "lead_quiet")
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.instruments.len(), 2);
        assert_eq!(state.instruments[1].name, "lead_quiet");
        assert_eq!(state.instruments[1].plugin, "Vital");
        assert_eq!(
            state.instruments[1].path,
            Some(std::path::PathBuf::from("presets/lead.vital"))
        );
        assert_eq!(state.instruments[1].volume, 0.6);
    }

    #[test]
    fn rejects_duplicate_names_across_instruments_and_samples() {
        let error = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "kick",
                    plugin = "Vital",
                    preset = "kick.vital",
                    volume = 1,
                }).addSample({name = "kick", path = "kick.wav", volume = 1})
            "#,
            "test.lua".into(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("instrument name 'kick' is already configured")
        );
    }

    #[test]
    fn rejects_copying_unknown_instruments_and_duplicate_destination_names() {
        for setup in [
            r#"resound.copyInstrument("missing", "copy")"#,
            r#"
                resound.addInstrument({
                    name = "lead",
                    plugin = "Vital",
                    preset = "lead.vital",
                    volume = 1,
                }).copyInstrument("lead", "lead")
            "#,
        ] {
            let source = format!(r#"local resound = require("resound"); {setup}"#);
            assert!(execute_source(&source, "test.lua".into()).is_err());
        }
    }

    #[test]
    fn set_param_value_records_initial_vst_parameter_values() {
        let execution = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "synth",
                    plugin = "Vital",
                    preset = "synth.vital",
                    volume = 1,
                })
                    .setParamValue("synth", "lfo_rate", 0.75)
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(
            execution.initial_parameter_values,
            vec![super::ParameterValue {
                instrument_name: "synth".into(),
                parameter_name: "lfo_rate".into(),
                value: 0.75,
            }]
        );
    }

    #[test]
    fn set_param_value_can_be_called_from_effect_fn() {
        let execution = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "synth",
                    plugin = "Vital",
                    preset = "synth.vital",
                    volume = 1,
                })
                local effect = function(t, playing)
                    resound.setParamValue("synth", "cutoff", playing and 0.8 or 0.1)
                end
                resound.pushChords("synth", {n(c4, {1}, effect)}, {at = 1})
            "#,
            "test.lua".into(),
        )
        .unwrap();
        let chord = &execution.state.chord_sets[0].chords[0];

        chord
            .effect_fn
            .as_ref()
            .unwrap()
            .call::<()>((0.0, true))
            .unwrap();
        assert_eq!(
            execution.parameter_values.borrow().last(),
            Some(&super::ParameterValue {
                instrument_name: "synth".into(),
                parameter_name: "cutoff".into(),
                value: 0.8,
            })
        );
    }

    #[test]
    fn set_param_value_rejects_unknown_wav_and_out_of_range_targets() {
        for (setup, call, expected) in [
            (
                "",
                r#"resound.setParamValue("missing", "param", 0.5)"#,
                "unknown instrument 'missing'",
            ),
            (
                r#"resound.addSample({name = "drums", path = "drums.wav", volume = 1})"#,
                r#"resound.setParamValue("drums", "param", 0.5)"#,
                "not a VST instrument",
            ),
            (
                r#"resound.addInstrument({name = "synth", plugin = "Vital", preset = "synth.vital", volume = 1})"#,
                r#"resound.setParamValue("synth", "param", 1.5)"#,
                "between 0 and 1",
            ),
        ] {
            let source = format!(r#"local resound = require("resound"); {setup}; {call}"#);
            assert!(
                execute_source(&source, "test.lua".into())
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
    }

    #[test]
    fn exposes_pitch_names_as_global_values() {
        execute_source(
            r#"
                assert(cb0 == "cb0")
                assert(c0 == "c0")
                assert(cs0 == "cs0")
                assert(db0 == "db0")
                assert(eb0 == "eb0")
                assert(b0 == "b0")
                assert(c1 == "c1")
                assert(a1 == "a1")
                assert(as1 == "as1")
                assert(b1 == "b1")
                assert(bs9 == "bs9")
            "#,
            "test.lua".into(),
        )
        .unwrap();
    }

    #[test]
    fn exposes_score_helpers_and_builds_chords() {
        execute_source(
            r#"
                local resound = require("resound")
                local note = n(c4, {2})
                assert(note.pitches[1] == "c4")
                assert(note.duration.length == 2)
                assert(note.duration.playLength == nil)

                local octave = o(bs8, {1, 0.5})
                assert(octave.pitches[1] == "bs9")
                assert(octave.duration.playLength == 0.5)

                local chord = c({c4, e4, g4}, {4, 0.75})
                assert(#chord.pitches == 3)
                assert(chord.pitches[2] == "e4")
                assert(chord.duration.length == 4)
                assert(chord.duration.playLength == 0.75)

                local percussion = p({0.5})
                assert(#percussion.pitches == 1)
                assert(percussion.pitches[1] == "x")
                assert(percussion.duration.length == 0.5)

                local rest = r({1})
                assert(#rest.pitches == 0)
                assert(rest.duration.length == 1)

                resound.addInstrument({
                    name = "synth",
                    plugin = "Vital",
                    preset = "synth.vital",
                    volume = 1,
                })
                    .pushChords("synth", {rest}, {at = 1})
            "#,
            "test.lua".into(),
        )
        .unwrap();
    }

    #[test]
    fn loads_teal_library_functions() {
        execute_source(
            r#"
                local chords = strum({c4, e4}, {1})
                local first, second = chords[1], chords[2]
                assert(first.pitches[1] == "c4")
                assert(first.duration.length == 0.1)
                assert(first.duration.playLength == 1)
                assert(second.pitches[1] == "e4")
                assert(second.duration.length == 0.9)
                assert(second.duration.playLength == 0.9)

                local flattened = straighten({
                    {n(c4, {1}), n(e4, {1})},
                    n(g4, {1}),
                })
                assert(#flattened == 3)
            "#,
            "test.lua".into(),
        )
        .unwrap();
    }

    #[test]
    fn executes_teal_files() {
        let script_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test/fixtures/perform.tl");
        let state = execute_file(&script_path).unwrap();
        assert_eq!(state.chord_sets.len(), 1);
        assert_eq!(state.chord_sets[0].chords.len(), 2);
        assert_eq!(state.chord_sets[0].chords[0].pitches, vec!["c4"]);
        assert_eq!(state.chord_sets[0].chords[1].pitches, vec!["e4"]);
    }

    #[test]
    fn stores_and_preserves_effect_fn_callbacks() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                local effect = function(t, playing)
                    return t, playing
                end
                resound.addInstrument({
                    name = "synth",
                    plugin = "Vital",
                    preset = "synth.vital",
                    volume = 1,
                })
                    .pushChords("synth", {n(c4, {1}, effect)}, {at = 1})
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.chord_sets[0].chords[0].effect_fn.is_some(), true);
        let result = state.chord_sets[0].chords[0]
            .effect_fn
            .as_ref()
            .unwrap()
            .call::<(f64, bool)>((-0.25, true))
            .unwrap();
        assert_eq!(result, (-0.25, true));
    }

    #[test]
    fn rejects_unknown_pitch_names_and_octaving_past_range() {
        let error = execute_source(r#"n("h4", {1})"#, "test.lua".into()).unwrap_err();
        assert!(error.to_string().contains("unknown pitch name"));

        let error = execute_source(r#"o(bs9, {1})"#, "test.lua".into()).unwrap_err();
        assert!(error.to_string().contains("ends at octave 9"));
    }

    #[test]
    fn collects_default_meter_and_pushed_chords() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.setDefaultMeter(1.25)
                    .pushChords("synth", {n(c4, {1}), r({0.5})}, {at = 2, times = 3})
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.default_meter, 1.25);
        assert_eq!(state.chord_sets.len(), 1);
        assert_eq!(state.chord_sets[0].at, 2.0);
        assert_eq!(state.chord_sets[0].instrument_name, "synth");
        assert_eq!(state.chord_sets[0].times, Some(3.0));
        assert_eq!(
            state.chord_sets[0].chords[0],
            super::Chord {
                pitches: vec!["c4".into()],
                duration: super::Duration {
                    length: 1.0,
                    play_length: None,
                },
                effect_fn: None,
            }
        );
        assert!(state.chord_sets[0].chords[1].pitches.is_empty());
    }

    #[test]
    fn creates_reusable_cache_keys_independent_of_bar_and_repeat_count() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "synth",
                    plugin = "Vital",
                    preset = "synth.vital",
                    volume = 1,
                })
                resound.pushChords("synth", {n(c4, {1})}, {at = 1, times = 2})
                resound.pushChords("synth", {n(c4, {1})}, {at = 4})
                resound.pushChords("synth", {n(d4, {1})}, {at = 4})
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.chord_sets[0].cache_key, state.chord_sets[1].cache_key);
        assert_ne!(state.chord_sets[1].cache_key, state.chord_sets[2].cache_key);
    }

    #[test]
    fn effect_chords_are_not_cacheable() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument({
                    name = "synth",
                    plugin = "Vital",
                    preset = "synth.vital",
                    volume = 1,
                })
                local effect = function() end
                resound.pushChords("synth", {n(c4, {1}, effect)}, {at = 1})
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.chord_sets[0].cache_key, None);
    }

    #[test]
    fn rejects_duplicate_sample_names() {
        let error = execute_source(
            r#"
                local resound = require("resound")
                resound.addSample({name = "kick", path = "samples/kick.wav", volume = 1})
                    .addSample({name = "kick", path = "other/kick.wav", volume = 1})
            "#,
            "test.lua".into(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("instrument name 'kick' is already configured")
        );
    }

    #[test]
    fn exposes_sample_trigger_pitch() {
        execute_source(
            r#"
                assert(x == "x")
                assert(n(x, {1}).pitches[1] == "x")
            "#,
            "test.lua".into(),
        )
        .unwrap();
    }
}
