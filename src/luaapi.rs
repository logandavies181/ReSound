use std::{
    cell::RefCell,
    error::Error,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use mlua::{Lua, Table};

#[derive(Clone, Debug, PartialEq)]
pub struct LuaState {
    pub instruments: Vec<InstrumentConfig>,
    pub default_bpm: f64,
    pub default_meter: f64,
    pub chord_sets: Vec<ChordSet>,
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
    pub chords: Vec<Chord>,
    pub at: f64,
    pub times: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub pitches: Vec<String>,
    pub duration: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duration {
    pub length: f64,
    pub ratio: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentConfig {
    pub name: String,
    pub path: PathBuf,
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
    install_score_functions(lua)?;
    let module = lua.create_table()?;

    let instrument_state = Rc::clone(&state);
    module.set(
        "addInstrument",
        lua.create_function(move |lua, path: String| {
            let instrument_path = Path::new(&path);
            let name = instrument_path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!(
                        "instrument path has no valid file name: {}",
                        instrument_path.display()
                    ))
                })?;
            let extension = instrument_path
                .extension()
                .and_then(|extension| extension.to_str())
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!(
                        "instrument path has no file extension: {}",
                        instrument_path.display()
                    ))
                })?;
            let instrument_type = InstrumentType::from_extension(extension).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "unsupported instrument type '{extension}' in path: {}",
                    instrument_path.display()
                ))
            })?;

            instrument_state
                .borrow_mut()
                .instruments
                .push(InstrumentConfig {
                    name: name.to_owned(),
                    path: PathBuf::from(path),
                    r#type: instrument_type,
                });
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

    let chords_state = Rc::clone(&state);
    module.set(
        "pushChords",
        lua.create_function(move |lua, (chords, options): (Table, Table)| {
            let chords = chords
                .sequence_values::<Table>()
                .map(|chord| parse_chord(&chord?))
                .collect::<mlua::Result<Vec<_>>>()?;
            let at = options.raw_get("at")?;
            let times = options.raw_get("times")?;
            chords_state
                .borrow_mut()
                .chord_sets
                .push(ChordSet { chords, at, times });
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
    for pitch in pitch_names() {
        globals.set(pitch.as_str(), pitch.as_str())?;
    }

    Ok(())
}

fn install_score_functions(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();

    globals.set(
        "n",
        lua.create_function(|lua, (pitch, duration): (String, Table)| {
            create_chord(lua, vec![validate_pitch(&pitch)?], &duration)
        })?,
    )?;
    globals.set(
        "o",
        lua.create_function(|lua, (pitch, duration): (String, Table)| {
            let pitch = octave_above(&pitch)?;
            create_chord(lua, vec![pitch], &duration)
        })?,
    )?;
    globals.set(
        "c",
        lua.create_function(|lua, (pitches, duration): (Table, Table)| {
            let pitches = pitches
                .sequence_values::<String>()
                .map(|pitch| validate_pitch(&pitch?))
                .collect::<mlua::Result<Vec<_>>>()?;
            create_chord(lua, pitches, &duration)
        })?,
    )?;
    globals.set(
        "r",
        lua.create_function(|lua, duration: Table| create_chord(lua, Vec::new(), &duration))?,
    )?;

    Ok(())
}

fn create_chord(lua: &Lua, pitches: Vec<String>, tuple: &Table) -> mlua::Result<Table> {
    let length: f64 = tuple.raw_get(1)?;
    let ratio: Option<f64> = tuple.raw_get(2)?;
    let pitches_table = lua.create_sequence_from(pitches)?;
    let duration = lua.create_table()?;
    duration.set("length", length)?;
    duration.set("ratio", ratio.unwrap_or(1.0))?;
    let chord = lua.create_table()?;
    chord.set("pitches", pitches_table)?;
    chord.set("duration", duration)?;
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
            ratio: duration.raw_get("ratio")?,
        },
    })
}

fn validate_pitch(pitch: &str) -> mlua::Result<String> {
    if pitch_names().any(|name| name == pitch) {
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

    (0..=9).flat_map(|octave| {
        PITCH_CLASSES
            .into_iter()
            .map(move |pitch_class| format!("{pitch_class}{octave}"))
    })
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
                        path: "samples/drums.wav".into(),
                        r#type: InstrumentType::Wav,
                    },
                    super::InstrumentConfig {
                        name: "synth".into(),
                        path: "plugins/synth.vst3".into(),
                        r#type: InstrumentType::Vst,
                    },
                ],
                default_bpm: 132.0,
                default_meter: 4.0,
                chord_sets: Vec::new(),
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
                local note = n(c4, {2})
                assert(note.pitches[1] == "c4")
                assert(note.duration.length == 2)
                assert(note.duration.ratio == 1)

                local octave = o(bs8, {1, 0.5})
                assert(octave.pitches[1] == "bs9")
                assert(octave.duration.ratio == 0.5)

                local chord = c({c4, e4, g4}, {4, 0.75})
                assert(#chord.pitches == 3)
                assert(chord.pitches[2] == "e4")
                assert(chord.duration.length == 4)
                assert(chord.duration.ratio == 0.75)

                local rest = r({1})
                assert(#rest.pitches == 0)
                assert(rest.duration.length == 1)
            "#,
            "test.lua".into(),
        )
        .unwrap();
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
                    .pushChords({n(c4, {1}), r({0.5})}, {at = 2, times = 3})
            "#,
            "test.lua".into(),
        )
        .unwrap();

        assert_eq!(state.default_meter, 1.25);
        assert_eq!(state.chord_sets.len(), 1);
        assert_eq!(state.chord_sets[0].at, 2.0);
        assert_eq!(state.chord_sets[0].times, Some(3.0));
        assert_eq!(
            state.chord_sets[0].chords[0],
            super::Chord {
                pitches: vec!["c4".into()],
                duration: super::Duration {
                    length: 1.0,
                    ratio: 1.0,
                },
            }
        );
        assert!(state.chord_sets[0].chords[1].pitches.is_empty());
    }
}
