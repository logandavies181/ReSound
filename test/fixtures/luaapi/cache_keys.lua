local resound = require("resound")
resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
})
resound.pushChords("synth", {n(c4, {1})}, {at = 1, times = 2})
resound.pushChords("synth", {n(c4, {1})}, {at = 4})
resound.pushChords("synth", {n(d4, {1})}, {at = 4})
