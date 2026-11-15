local resound = require("resound")
resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
})
local effect = function(t, playing)
    resound.setParamValue("synth", "cutoff", playing and 0.8 or 0.1)
end
resound.pushChords("synth", {n(c4, {1}, effect)}, {at = 1})
