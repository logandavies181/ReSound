use std::{cell::RefCell, error::Error, fs, path::Path, rc::Rc};

use mlua::{Lua, Table};

#[derive(Debug, Default)]
pub struct LuaState {
    pub instrument_config: Option<String>,
}

pub fn execute_file(path: &Path) -> Result<LuaState, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    let lua = Lua::new();
    let instrument_config = Rc::new(RefCell::new(None));
    install_module(&lua, Rc::clone(&instrument_config))?;

    lua.load(&source)
        .set_name(path.to_string_lossy().into_owned())
        .exec()?;

    Ok(LuaState {
        instrument_config: instrument_config.borrow().clone(),
    })
}

fn install_module(lua: &Lua, config: Rc<RefCell<Option<String>>>) -> mlua::Result<()> {
    let module = lua.create_table()?;

    let set_config = Rc::clone(&config);
    module.set(
        "set",
        lua.create_function(move |_, value: String| {
            *set_config.borrow_mut() = Some(value);
            Ok(())
        })?,
    )?;

    module.set(
        "say",
        lua.create_function(move |_, ()| {
            if let Some(value) = config.borrow().as_ref() {
                println!("{value}");
            }
            Ok(())
        })?,
    )?;

    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("resound", module)?;
    Ok(())
}
