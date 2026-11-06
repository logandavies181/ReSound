local resound = require('resound')

resound.addInstrument("test/fixtures/snare.wav")
    .addInstrument("test/fixtures/test.vital")
    .addInstrument("test/fixtures/test2.dexed")
    .setDefaultBpm(120)
    .setDefaultMeter(4)
    .pushChords("test", {
        o(c2, {0.5}),
        o(d2, {0.5}),
        o(c2, {1}),
    }, {at = 1})
    .pushChords("test2", {
        o(c4, {0.5}),
        o(d4, {0.5}),
        o(c4, {1}),
        r({1}),
    }, {at = 2})
    .pushChords("snare", {
        n(x, {1}),
        n(x, {1}),
    }, {at = 1})
