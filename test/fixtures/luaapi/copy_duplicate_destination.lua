local resound = require("resound")
resound.addInstrument({
    name = "lead",
    plugin = "Vital",
    preset = "lead.vital",
    volume = 1,
}).copyInstrument("lead", "lead")
