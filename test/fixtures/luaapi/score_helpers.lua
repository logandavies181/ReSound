local resound = require("resound")
local note = n(c4, {2})
assert(note.pitches[1] == "c4")
assert(note.duration.length == 2)
assert(note.duration.playLength == nil)

local octave = o(bs8, {1, 0.5})
assert(octave.pitches[1] == "bs9")
assert(octave.duration.playLength == 0.5)

local chord = c({c4, e4, g4}, {4, 0.75})
assert(#chord.pitches == 3)
assert(chord.pitches[2] == "e4")
assert(chord.duration.length == 4)
assert(chord.duration.playLength == 0.75)

local percussion = p({0.5})
assert(#percussion.pitches == 1)
assert(percussion.pitches[1] == "x")
assert(percussion.duration.length == 0.5)

local rest = r({1})
assert(#rest.pitches == 0)
assert(rest.duration.length == 1)

resound.addInstrument({
    name = "synth",
    plugin = "Vital",
    preset = "synth.vital",
    volume = 1,
}).pushChords("synth", {rest}, {at = 1})
