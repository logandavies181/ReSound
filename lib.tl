require("resound")

global function strum(notes: {string}, input: DurationTuple): Chord...
    local len = input[1]
    local playLen = input[2] or 1

    local ret = {}
    for i, v in ipairs(notes) do
        local offsetSize = 0.1
        local offset = (i-1) * offsetSize

        local outputLen = (i == #notes) and (len - offset) or offsetSize

        table.insert(ret, n(v as Pitch, {outputLen, playLen-offset}))
    end
    return table.unpack(ret)
end
