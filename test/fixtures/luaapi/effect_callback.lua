local resound = require("resound")
local effect = function(t, playing)
    return t, playing
end
resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
}).pushChords("synth", {n(c4, {1}, effect)}, {at = 1})
