global record ReSound
  -- All the methods chain
  addInstrument: function(path: string): ReSound --parses the path into a name and type, based on file extension.
  setDefaultBpm: function(bpm: number): ReSound
end

-- Define the internal state, managed in the ReSound Host.
local record ReSoundConfig
  instruments: {InstrumentConfig}
  defaultBpm: number
end

-- Not part of the API, just here to silence lsp warnings.
local _: ReSoundConfig = {}

global record InstrumentConfig
  name: string
  type: InstrumentType
end

global enum InstrumentType
  "wav"
  "vst"
end

global enum Pitch
  "cb0"
  "c0"
  "cs0"
  -- And so on, up to gs9
end

global record Chord
  pitches: {Pitch}
  duration: Duration
end

global record Duration
  --- How long the note lasts
  length: number
  --- Fraction of how long to actually play the note for
  ratio: number
end

-- Midi note names. These are exposed as ambient variables by the ReSound Host.
global cb0 = "cb0"
global c0 = "c0"
global cs0 = "cs0"
-- And so on, up to gs9

local M = {} as ReSound

return M
