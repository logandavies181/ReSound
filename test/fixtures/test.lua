local r = require('resound')

r.addInstrument("snare.wav")
    .setDefaultBpm(120)
    .setDefaultMeter(4)
    .pushChords({
        o(c2, {0.5}),
        o(d2, {0.5}),
        o(c2, {1}),
    }, {at = 1})
