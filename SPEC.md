# ReSound

This project allows the user to write Lua code to drive VST3 plugins and play WAV samples in code defined music.

## Code structure

### src/luaapi.rs

Exposes functions the embedded Lua runtime.

### src/sequencer.rs

Parses the end state after the user's Lua code and builds out the input for the renderer.

### src/vst_loader.rs

Resolves the configured VST instrument to a discovered VST3 plugin and preset path. The renderer is responsible for
instantiating the plugin and loading its preset.

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

The score helpers (`n`, `o`, `c`, `p`, and `r`) are global Lua functions. `p` creates a percussion/sample event
using the special `x` pitch; `r` creates a silent rest. Avoid assigning the module to a local named `r`, which
shadows the rest helper; prefer `local resound = require('resound')`.

The score helpers accept an optional final `EffectFn` argument. When provided, it is called once per sample starting
at the chord. `t` is elapsed time in quarter notes since chord start, beginning at zero, and `playing` is true only
during the chord's gate. The callback is called once more after the gate with `playing` false for cleanup.

## Sequencer

The sequencer passes through config for the instruments to the renderer input unaltered from the Lua state and flattens
any score data in the Lua state from the user's code and prepares it for the renderer.

VST plugins are selected from discovered plugins using the instrument name configured by `addInstrument()`. The
`.vital` and `.dexed` extensions select Vital and Dexed respectively. Multiple VST and WAV instruments may be added.

Each `pushChords()` call names its target instrument. Pitches sent to a VST instrument become MIDI notes; WAV
instruments are triggered with the special `x` pitch. Sample events play only the named WAV instrument, and instrument
names must be unique.

`addInstrument(path, { volume = value })` optionally sets a per-instrument linear output multiplier from 0 to 1.
It applies to both WAV and VST instruments, defaults to 1, and rejects non-finite or out-of-range values.

A chord duration's ratio controls its gate length as a multiple of its rhythmic length. Ratios may exceed 1, so a note
can ring past the next chord, which still starts on its normal schedule.

Bars are automatically added as needed based on the output of the Lua State. Bars may be modified in terms of BPM and
length in the future. For now they take on the default values as set in the user's Lua.

## Renderer

Load the relevant files as per the input. Also accept an argument for the output location.

## Cmd

Orchestrate a subcommand hierarchy, defaulting to `perform` if the first arg ends with `.lua`.

### init

Creates `tlconfig.lua` and a complete `resound.d.tl` pitch declaration in the current directory. Existing tlconfig.lua
is not overwritten, but resound.d.tl is.

### list-plugins

Lists the names of all discovered plugins.

### show-parameters

Displays all of the parameters of the named plugin.

#### Args

Plugin name - required. Name of the plugin as displayed in list-plugins

### perform

#### Args

File name - default to main.lua. File to read from.

#### Options

Output (o) - default to gen.wav. File to write to.

From-Bar (`-f` / `--from-bar`) - positive whole bar number to start rendering from; defaults to bar 1.
The output timeline starts at 0 for this bar. Events that begin before it are omitted, including notes that would
otherwise ring into the selected bar.
