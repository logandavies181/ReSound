# ReSound

This project allows the user to write Lua code to drive VST3 plugins and play WAV samples in code defined music.

## Code structure

### src/luaapi.rs

Exposes functions the embedded Lua runtime.

### src/sequencer.rs

Parses the end state after the user's Lua code and builds out the input for the renderer.

### src/renderer_input.rs

Shared type that sequencer creates and renderer consumes.

### src/renderer.rs

Loads files backing the instruments and writes out rendered WAV.

### src/cmd.rs

Handles the command line interface.

### src/main.rs

Thin wrapper around the CLI.

## Lua API

The user is expected to `require('resound')` (provided by this program's Lua host environment) and call the provided
methods to configure their instruments and declare their performance.

See [resound.d.tl](./resound.d.tl)

Stdout/Stderr from the user's code is displayed as normal.

## Sequencer

The sequencer passes through config for the instruments to the renderer input unaltered from the Lua state and flattens
any score data in the Lua state from the user's code and prepares it for the renderer.

Instrument config is a placeholder for now: the sample uses the local .wav file, and the VST is
selected from discovered plugins using the name configured by `addInstrument()`.

Notes are based on the `pushChords()` function. Bars are automatically added as needed based on the output of the Lua
State. Bars may be modified in terms of BPM and length in the future. For now they take on the default values as set in
the user's Lua.

## Renderer

Load the relevant files as per the input. Also accept an argument for the output location.

## Cmd

Orchestrate a subcommand hierarchy, defaulting to `perform` if the first arg ends with `.lua`.

### init

Creates `tlconfig.lua` and a complete `resound.d.tl` pitch declaration in the current directory. Existing files are not overwritten.

### perform

#### Args

File name - default to main.lua. File to read from.

#### Options

Output (o) - default to gen.wav. File to write to.
