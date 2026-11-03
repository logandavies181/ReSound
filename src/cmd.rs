use std::{
    env,
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::{luaapi, renderer, sequencer};

pub fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let (command, command_args) = match args.first() {
        None => {
            print_usage();
            return Ok(());
        }
        Some(first) if first.to_string_lossy().ends_with(".lua") => ("perform", &args[..]),
        Some(first) if first == "perform" => ("perform", &args[1..]),
        Some(first) if first == "--help" || first == "-h" => {
            print_usage();
            return Ok(());
        }
        Some(first) => {
            return Err(format!("unknown command: {}", first.to_string_lossy()).into());
        }
    };

    match command {
        "perform" => perform(command_args),
        _ => unreachable!(),
    }
}

fn perform(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let mut script_path = PathBuf::from("main.lua");
    let mut output_path = PathBuf::from("gen.wav");
    let mut script_set = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].to_str() {
            Some("-o" | "--output") => {
                index += 1;
                let Some(path) = args.get(index) else {
                    return Err("missing path after --output".into());
                };
                output_path = PathBuf::from(path);
            }
            Some("-h" | "--help") => {
                print_usage();
                return Ok(());
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option: {value}").into());
            }
            _ if !script_set => {
                script_path = PathBuf::from(&args[index]);
                script_set = true;
            }
            _ => return Err("perform accepts only one Lua file".into()),
        }
        index += 1;
    }

    let project_dir = env::current_dir()?;
    let script_path = resolve_from(&project_dir, &script_path);
    let output_path = resolve_from(&project_dir, &output_path);
    let lua_state = luaapi::execute_file(&script_path)?;
    let input = sequencer::sequence(lua_state, &project_dir, output_path);
    renderer::render(input)
}

fn resolve_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

fn print_usage() {
    println!("Usage: resound [perform] [FILE.lua] [-o|--output FILE.wav]");
    println!("       resound -h|--help");
}
