local resound = require("resound")
resound.addSample({name = "drums", path = "samples/drums.wav", volume = 0})
    .addInstrument({
        name = "synth",
        plugin = "Vital",
        preset = "presets/lead.vital",
        volume = 0.6,
    })
    .setDefaultBpm(132)
