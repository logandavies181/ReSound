mod cmd;
mod luaapi;
mod renderer;
mod renderer_input;
mod sequencer;
mod vst_loader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cmd::run()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use crate::{luaapi, renderer, sequencer};

    #[test]
    fn renders_fixtures() {
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures_dir = project_dir.join("test/fixtures");
        let outputs_dir = project_dir.join("test/outputs");
        fs::create_dir_all(&outputs_dir).unwrap();

        for fixture in fs::read_dir(fixtures_dir).unwrap() {
            let fixture = fixture.unwrap().path();
            if fixture
                .extension()
                .is_some_and(|extension| extension == "lua")
            {
                let state = luaapi::execute_file(&fixture).unwrap();
                let output_path = outputs_dir
                    .join(fixture.file_stem().unwrap())
                    .with_extension("wav");
                let input = sequencer::sequence(state, project_dir, output_path).unwrap();
                renderer::render(input).unwrap();
            }
        }
    }
}
