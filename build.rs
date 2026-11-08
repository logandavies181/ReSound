use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=lib.tl");

    let source = fs::read_to_string("lib.tl")
        .expect("failed to read lib.tl")
        .replace("require(\"resound\")", "require('resound')");
    let inline_source = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    fs::write(
        output.join("teal_lib.rs"),
        format!("pub const SOURCE: &str = tealr::compile_inline_teal!(\"{inline_source}\");\n"),
    )
    .expect("failed to generate Teal source wrapper");
}
