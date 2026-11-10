require("resound")

global function strum(notes: {Pitch}, input: DurationTuple): {Chord}
    local len = input[1]
    local playLen = input[2] or len

    local ret = {}
    for i, v in ipairs(notes) do
        local offsetSize = 0.1
        local offset = (i-1) * offsetSize

        local outputLen = (i == #notes) and (len - offset) or offsetSize

        table.insert(ret, n(v, {outputLen, playLen-offset}))
    end
    return ret
end

global function pianoStrum(notes: {Pitch}, input: DurationTuple): {Chord}
    local len = input[1]
    local playLen = input[2] or len

    local ret = {}
    local currentOffset = 0.05
    local totalOffset = 0.0

    for i, v in ipairs(notes) do
        if i == #notes then break end

        local outputLen = currentOffset

        table.insert(ret, n(v, {outputLen, playLen - totalOffset}))
        totalOffset = totalOffset + currentOffset
        currentOffset = 0.001
    end

    table.insert(ret, n(notes[#notes], {len - currentOffset, playLen - currentOffset}))

    return ret
end


global function arpeggiate(notes: {Pitch}, input: DurationTuple, spacing: number): {Chord}
    local len = input[1]
    local playLen = input[2] or len

    local ret = {}
    for i, v in ipairs(notes) do
        if i == #notes then break end

        local offset = (i-1) * spacing
        table.insert(ret, n(v, {spacing, playLen - offset}))
    end
    local totalOffset = spacing * #notes
    table.insert(ret, n(notes[#notes], {len - totalOffset + spacing, playLen - totalOffset + spacing}))

    return ret 
end

--- Convert {Chord|{Chord}} to {Chord}
global function straighten(input: {any}): {Chord}
    local ret = {}

    for _, v in ipairs(input) do
        local val = (v as {any})[1]
        if val ~= nil then
            for _, w in ipairs(v as {any}) do
                table.insert(ret, w as Chord)
            end
        else
            table.insert(ret, v as Chord)
        end
    end

    return ret
end
