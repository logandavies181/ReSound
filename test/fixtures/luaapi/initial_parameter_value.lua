local resound = require("resound")
resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
}).setParamValue("synth", "lfo_rate", 0.75)
