mod cmd;
mod luaapi;
mod renderer;
mod renderer_input;
mod sequencer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cmd::run()
}
