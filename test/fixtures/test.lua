local resound = require('resound')

resound
    .addSample({
        name = "snare",
        path = "test/fixtures/snare.wav",
        volume = 1,
    })
    .addInstrument({
        name = "test",
        plugin = "Vital",
        preset = "test/fixtures/test.vital",
        volume = 1,
    })
    .addInstrument({
        name = "sine",
        plugin = "Vital",
        preset = "test/fixtures/sine.vital",
        volume = 1,
    })
    .addInstrument({
        name = "test2",
        plugin = "Dexed",
        preset = "test/fixtures/test2.dexed",
        volume = 1,
    })
    .setDefaultBpm(120)
    .setDefaultMeter(4)
    .pushChords("test", {
        o(c2, {0.5}),
        o(d2, {0.5}),
        o(c2, {1}),
    }, {at = 1})
    .pushChords("sine", {
        o(c2, {2}),
    }, {at = 1})
    .pushChords("test2", {
        o(c4, {0.5}),
        o(d4, {0.5}),
        o(c4, {1}),
        r({1}),
    }, {at = 2})
    .pushChords("snare", {
        p({1}),
        p({1}),
    }, {at = 1})
