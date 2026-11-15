local resound = require("resound")
resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
})
local effect = function() end
resound.pushChords("synth", {n(c4, {1}, effect)}, {at = 1})
