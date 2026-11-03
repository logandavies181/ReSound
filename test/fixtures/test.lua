local r = require('resound')

r.addInstrument("snare.wav")
    .setDefaultBpm(120)
    .setDefaultMeter(4)
    .pushChords({
        o(c5, {0.5}),
        o(d5, {0.5}),
        o(c5, {1}),
    }, {at = 1})
