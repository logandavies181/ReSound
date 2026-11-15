local resound = require("resound")
resound.setDefaultMeter(1.25)
    .pushChords("synth", {n(c4, {1}), r({0.5})}, {at = 2, times = 3})
