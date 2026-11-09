use std::{
    env,
    error::Error,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::{luaapi, renderer, sequencer, vst_loader};

pub fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let (command, command_args) = match args.first() {
        None => {
            print_usage();
            return Ok(());
        }
        Some(first)
            if first.to_string_lossy().ends_with(".lua")
                || first.to_string_lossy().ends_with(".tl") =>
        {
            ("perform", &args[..])
        }
        Some(first) if first == "perform" => ("perform", &args[1..]),
        Some(first) if first == "init" => ("init", &args[1..]),
        Some(first) if first == "list-plugins" => ("list-plugins", &args[1..]),
        Some(first) if first == "show-parameters" => ("show-parameters", &args[1..]),
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
        "init" => init(command_args),
        "list-plugins" => list_plugins(command_args),
        "show-parameters" => show_parameters(command_args),
        _ => unreachable!(),
    }
}

fn init(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: resound init");
        return Ok(());
    }
    if !args.is_empty() {
        return Err("init accepts no arguments".into());
    }

    let project_dir = env::current_dir()?;
    init_project(&project_dir)
}

fn init_project(project_dir: &Path) -> Result<(), Box<dyn Error>> {
    let tlconfig_path = project_dir.join("tlconfig.lua");
    let mut created_tlconfig = false;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tlconfig_path)
    {
        Ok(mut file) => {
            created_tlconfig = true;
            if let Err(error) = file.write_all(include_str!("../tlconfig.lua").as_bytes()) {
                drop(file);
                cleanup_created(std::slice::from_ref(&tlconfig_path), &error)?;
                return Err(error.into());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !tlconfig_path.is_file() {
                return Err(format!("{} is not a file", tlconfig_path.display()).into());
            }
        }
        Err(error) => return Err(error.into()),
    }

    if let Err(error) = fs::write(
        project_dir.join("resound.d.tl"),
        include_str!("../resound.d.tl"),
    ) {
        if created_tlconfig {
            cleanup_created(std::slice::from_ref(&tlconfig_path), &error)?;
        }
        return Err(error.into());
    }

    println!("Initialized tlconfig.lua if missing and refreshed resound.d.tl");
    Ok(())
}

fn cleanup_created(paths: &[PathBuf], cause: &io::Error) -> io::Result<()> {
    let mut cleanup_errors = Vec::new();
    for path in paths {
        if let Err(error) = fs::remove_file(path) {
            cleanup_errors.push(format!("{}: {error}", path.display()));
        }
    }
    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{cause}; also failed to remove generated files: {}",
            cleanup_errors.join(", ")
        )))
    }
}

fn perform(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let mut script_path = PathBuf::from("main.lua");
    let mut output_path = PathBuf::from("gen.wav");
    let mut from_bar = 1;
    let mut script_set = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].to_str() {
            Some("-f" | "--from-bar") => {
                index += 1;
                let Some(bar) = args.get(index) else {
                    return Err("missing bar number after --from-bar".into());
                };
                from_bar = parse_from_bar(bar)?;
            }
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
            _ => return Err("perform accepts only one Lua or Teal file".into()),
        }
        index += 1;
    }

    let project_dir = env::current_dir()?;
    let script_path = resolve_from(&project_dir, &script_path);
    let output_path = resolve_from(&project_dir, &output_path);
    let lua_state = luaapi::execute_file(&script_path)?;
    let input = sequencer::sequence_from_bar(lua_state, &project_dir, output_path, from_bar)?;
    renderer::render(input)
}

fn parse_from_bar(value: &OsString) -> Result<u64, Box<dyn Error>> {
    let bar = value
        .to_str()
        .ok_or("from-bar value must be a positive whole number")?
        .parse::<u64>()?;
    if bar == 0 {
        return Err("from-bar value must be a positive whole number".into());
    }
    Ok(bar)
}

fn list_plugins(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: resound list-plugins");
        return Ok(());
    }
    if !args.is_empty() {
        return Err("list-plugins accepts no arguments".into());
    }

    for name in vst_loader::list_plugins()? {
        println!("{name}");
    }
    Ok(())
}

fn show_parameters(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: resound show-parameters PLUGIN_NAME");
        return Ok(());
    }
    let [plugin_name] = args else {
        return Err("show-parameters requires one plugin name".into());
    };
    let plugin_name = plugin_name
        .to_str()
        .ok_or("plugin name must be valid UTF-8")?;

    for (name, value) in vst_loader::show_plugin_parameters(plugin_name)? {
        println!("{name}: {value}");
    }
    Ok(())
}

fn resolve_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

fn print_usage() {
    println!(
        "Usage: resound [perform] [FILE.lua|FILE.tl] [-o|--output FILE.wav] [-f|--from-bar BAR]"
    );
    println!("       resound init");
    println!("       resound list-plugins");
    println!("       resound show-parameters PLUGIN_NAME");
    println!("       resound -h|--help");
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{init_project, parse_from_bar};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn init_creates_config_and_complete_pitch_declaration() {
        let project_dir = temp_project_dir();
        init_project(&project_dir).unwrap();

        assert_eq!(
            fs::read_to_string(project_dir.join("tlconfig.lua")).unwrap(),
            include_str!("../tlconfig.lua")
        );
        let declaration = fs::read_to_string(project_dir.join("resound.d.tl")).unwrap();
        let pitches = declaration
            .split("global enum Pitch")
            .nth(1)
            .unwrap()
            .split("\nend")
            .next()
            .unwrap();
        assert_eq!(pitches.matches("  \"").count(), 211);
        assert!(declaration.contains("global cb0: Pitch = \"cb0\""));
        assert!(declaration.contains("global bs9: Pitch = \"bs9\""));
        assert!(!declaration.contains("And so on"));

        fs::remove_dir_all(project_dir).unwrap();
    }

    #[test]
    fn init_preserves_tlconfig_and_refreshes_declaration() {
        let project_dir = temp_project_dir();
        fs::write(project_dir.join("tlconfig.lua"), "existing config").unwrap();
        fs::write(project_dir.join("resound.d.tl"), "existing").unwrap();

        init_project(&project_dir).unwrap();
        assert_eq!(
            fs::read_to_string(project_dir.join("tlconfig.lua")).unwrap(),
            "existing config"
        );
        assert_eq!(
            fs::read_to_string(project_dir.join("resound.d.tl")).unwrap(),
            include_str!("../resound.d.tl")
        );

        fs::remove_dir_all(project_dir).unwrap();
    }

    #[test]
    fn init_creates_missing_tlconfig_even_when_declaration_exists() {
        let project_dir = temp_project_dir();
        fs::write(project_dir.join("resound.d.tl"), "existing").unwrap();

        init_project(&project_dir).unwrap();

        assert_eq!(
            fs::read_to_string(project_dir.join("tlconfig.lua")).unwrap(),
            include_str!("../tlconfig.lua")
        );
        assert_eq!(
            fs::read_to_string(project_dir.join("resound.d.tl")).unwrap(),
            include_str!("../resound.d.tl")
        );

        fs::remove_dir_all(project_dir).unwrap();
    }

    #[test]
    fn parses_from_bar_as_a_positive_whole_number() {
        assert_eq!(parse_from_bar(&OsString::from("3")).unwrap(), 3);
        assert!(parse_from_bar(&OsString::from("0")).is_err());
        assert!(parse_from_bar(&OsString::from("1.5")).is_err());
    }

    fn temp_project_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "resound-init-test-{}-{}",
            std::process::id(),
            NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
