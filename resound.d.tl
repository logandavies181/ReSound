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

global record InstrumentConfig
  name: string
  type: InstrumentType
end

global enum InstrumentType
  "wav"
  "vst"
end

global record Pitch
  frequency: number
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

local M = {} as ReSound

return M
