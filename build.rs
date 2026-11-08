use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));
    let source_path = manifest_dir.join("lib.tl");
    println!("cargo:rerun-if-changed={}", source_path.display());

    let source = fs::read_to_string(source_path)
        .expect("failed to read lib.tl")
        .replace("require(\"resound\")", "require('resound')");
    let inline_source = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    fs::write(
        output.join("teal_lib.rs"),
        format!("pub const SOURCE: &str = tealr::compile_inline_teal!(\"{inline_source}\");\n"),
    )
    .expect("failed to generate Teal source wrapper");
}
