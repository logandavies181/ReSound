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

Placeholder for now, just local the .wav and VST file already specified and output a middle 'C' for VSTs and play samples a single time.

## Renderer

Load the relevant files as per the input. Also accept an argument for the output location.

## Cmd

Orchestrate a subcommand hierarchy, defaulting to `perform` if the first arg ends with `.lua`.

### perform

#### Args

File name - default to main.lua. File to read from.

#### Options

Output (o) - default to gen.wav. File to write to.
