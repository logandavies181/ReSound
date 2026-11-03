use std::{cell::RefCell, error::Error, fs, path::Path, rc::Rc};

use mlua::{Lua, Table};

#[derive(Clone, Debug, PartialEq)]
pub struct LuaState {
    pub instruments: Vec<InstrumentConfig>,
    pub default_bpm: f64,
}

impl Default for LuaState {
    fn default() -> Self {
        Self {
            instruments: Vec::new(),
            default_bpm: 120.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentConfig {
    pub name: String,
    pub r#type: InstrumentType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstrumentType {
    Wav,
    Vst,
}

impl InstrumentType {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "wav" => Some(Self::Wav),
            "vst" | "vst3" => Some(Self::Vst),
            _ => None,
        }
    }
}

pub fn execute_file(path: &Path) -> Result<LuaState, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    execute_source(&source, path.to_string_lossy().into_owned()).map_err(Into::into)
}

fn execute_source(source: &str, name: String) -> mlua::Result<LuaState> {
    let lua = Lua::new();
    let state = Rc::new(RefCell::new(LuaState::default()));
    install_module(&lua, Rc::clone(&state))?;

    lua.load(source).set_name(name).exec()?;

    Ok(state.borrow().clone())
}

fn install_module(lua: &Lua, state: Rc<RefCell<LuaState>>) -> mlua::Result<()> {
    install_pitch_globals(lua)?;
    let module = lua.create_table()?;

    let instrument_state = Rc::clone(&state);
    module.set(
        "addInstrument",
        lua.create_function(move |lua, path: String| {
            let path = Path::new(&path);
            let name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!(
                        "instrument path has no valid file name: {}",
                        path.display()
                    ))
                })?;
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!(
                        "instrument path has no file extension: {}",
                        path.display()
                    ))
                })?;
            let instrument_type = InstrumentType::from_extension(extension).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "unsupported instrument type '{extension}' in path: {}",
                    path.display()
                ))
            })?;

            instrument_state
                .borrow_mut()
                .instruments
                .push(InstrumentConfig {
                    name: name.to_owned(),
                    r#type: instrument_type,
                });
            loaded_module(lua)
        })?,
    )?;

    module.set(
        "setDefaultBpm",
        lua.create_function(move |lua, bpm: f64| {
            state.borrow_mut().default_bpm = bpm;
            loaded_module(lua)
        })?,
    )?;

    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("resound", module)?;
    Ok(())
}

fn install_pitch_globals(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("cb0", "cb0")?;

    for octave in 0..=9 {
        for pitch_class in ["c", "cs", "d", "ds", "e", "f", "fs", "g", "gs"] {
            let pitch = format!("{pitch_class}{octave}");
            globals.set(pitch.as_str(), pitch.as_str())?;
        }

        if octave < 9 {
            for pitch_class in ["a", "as", "b"] {
                let pitch = format!("{pitch_class}{octave}");
                globals.set(pitch.as_str(), pitch.as_str())?;
            }
        }
    }

    Ok(())
}

fn loaded_module(lua: &Lua) -> mlua::Result<Table> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.get("resound")
}

#[cfg(test)]
mod tests {
    use super::{InstrumentType, LuaState, execute_source};

    #[test]
    fn exposes_specified_chaining_api_and_collects_config() {
        let state = execute_source(
            r#"
                local resound = require("resound")
                resound.addInstrument("samples/drums.wav")
                    .addInstrument("plugins/synth.vst3")
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
                        r#type: InstrumentType::Wav,
                    },
                    super::InstrumentConfig {
                        name: "synth".into(),
                        r#type: InstrumentType::Vst,
                    },
                ],
                default_bpm: 132.0,
            }
        );
    }

    #[test]
    fn rejects_instruments_with_unsupported_extensions() {
        let error = execute_source(
            r#"require("resound").addInstrument("plugins/synth.mp3")"#,
            "test.lua".into(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported instrument type"));
    }

    #[test]
    fn exposes_pitch_names_as_global_values() {
        execute_source(
            r#"
                assert(cb0 == "cb0")
                assert(c0 == "c0")
                assert(cs0 == "cs0")
                assert(b0 == "b0")
                assert(c1 == "c1")
                assert(a1 == "a1")
                assert(as1 == "as1")
                assert(b1 == "b1")
                assert(gs9 == "gs9")
            "#,
            "test.lua".into(),
        )
        .unwrap();
    }
}
