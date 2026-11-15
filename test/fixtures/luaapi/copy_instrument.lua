local resound = require("resound")
resound.addInstrument({
    name = "lead",
    plugin = "Vital",
    preset = "presets/lead.vital",
    volume = 0.6,
}).copyInstrument("lead", "lead_quiet")
