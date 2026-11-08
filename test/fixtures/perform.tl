local resound = require("resound")
local first, second = strum({c4, e4}, {1})

resound.addInstrument({name = "synth", plugin = "Vital"})
    .pushChords("synth", {first, second}, {at = 1})
