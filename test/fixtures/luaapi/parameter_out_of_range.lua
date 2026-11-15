local resound = require("resound")
resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
})
resound.setParamValue("synth", "param", 1.5)
