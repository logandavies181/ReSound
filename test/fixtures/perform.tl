local resound = require("resound")
local chords = strum({c4, e4}, {1})
local first, second = chords[1], chords[2]

resound.addInstrument({name = "synth", plugin = "Vital"})
    .pushChords("synth", {first, second}, {at = 1})
