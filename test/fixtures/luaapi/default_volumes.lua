local resound = require("resound")
resound.addInstrument({
    name = "lead",
    plugin = "Vital",
    preset = "lead.vital",
}).addSample({name = "kick", path = "kick.wav"})
