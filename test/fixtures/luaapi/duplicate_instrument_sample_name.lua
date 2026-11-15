local resound = require("resound")
resound.addInstrument({
    name = "kick",
    plugin = "Vital",
    preset = "kick.vital",
    volume = 1,
}).addSample({name = "kick", path = "kick.wav", volume = 1})
