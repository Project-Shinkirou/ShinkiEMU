-- Opt-in local recipe for the original Japanese release. The catalogued
-- translation keeps its existing direct-start recipe and receipt.
local function recall(ctx, gallery)
    local pe, file = gallery.open(ctx)
    local require_input = gallery.require_input
    local function bytes(rva, size)
        return file.reader:bytes(pe.offset(file, rva, size), size)
    end
    local function word(rva)
        return buffer.readu32(bytes(rva, 4), 0)
    end
    local function string_at(address)
        local rva = address - file.base
        local raw = file.reader:cstring(pe.offset(file, rva, 1), 128)
        pe.offset(file, rva, buffer.len(raw) + 1)
        return buffer.tostring(raw)
    end

    -- Exact compiler instruction shapes: six immediate pushes, LEA and
    -- register pushes, then a relative call to the native button constructor.
    -- Addresses below are RVAs in the fingerprinted image, not runtime VAs.
    local pc = 0x91d37
    local function push()
        local kind = buffer.readu8(bytes(pc, 1), 0)
        if kind == 0x6a then
            local value = buffer.readi8(bytes(pc + 1, 1), 0)
            pc += 2
            return value
        end
        require_input(ctx, kind == 0x68, "invalid native Recall immediate")
        local value = buffer.readi32(bytes(pc + 1, 4), 0)
        pc += 5
        return value
    end
    local buttons = {}
    for i = 0, 44 do
        local parent, h, w, y, x, index = push(), push(), push(), push(), push(), push()
        local args = bytes(pc, 9)
        local reg = buffer.readu8(args, 1)
        local reg_push = ({[0x85] = 0x50, [0x8d] = 0x51, [0x95] = 0x52})[reg]
        require_input(ctx, index == i and parent >= -1 and parent < i and reg_push and
            buffer.readu8(args, 0) == 0x8d and buffer.readi32(args, 2) == -340 and
            buffer.readu8(args, 6) == reg_push and buffer.readu8(args, 7) == 0x53 and
            buffer.readu8(args, 8) == 0xe8, "unknown native Recall button constructor layout")
        local displacement = buffer.readi32(bytes(pc + 9, 4), 0)
        pc += 13
        require_input(ctx, pc + displacement == 0x91020, "invalid native Recall button call")

        local target = string_at(word(0x13efd8 + i * 4))
        local script, label = string.match(target, "^([%w_%-]+%.sc)#([%w_%-]+)$")
        if not script then script = string.match(target, "^([%w_%-]+%.sc)$") end
        require_input(ctx, script, "invalid original Recall script target")
        local button = {rect = {x,y,w,h}, action = {kind="script", resource="musica:/scr/" .. script, label=label}}
        local thumb = word(0x13f0a0 + i * 4)
        if thumb ~= 0 then
            local name = string_at(thumb)
            require_input(ctx, parent >= 0 and string.match(name, "^[%w_%-]+%.png$"),
                "invalid original Recall thumbnail or preview parent")
            local origin = buttons[parent + 1].rect
            button.preview = {resource = "musica:/sys/memories/recall/thumb/" .. name,
                at = {origin[1], origin[2]}}
        end
        buttons[i + 1] = button
    end
    require_input(ctx, pc == 0x92331, "native Recall constructor count disagrees")
    local rect = {word(0x13efc8),word(0x13efcc),word(0x13efd0),word(0x13efd4)}
    rect[3] -= rect[1]
    rect[4] -= rect[2]
    table.insert(buttons, {rect=rect, action={kind="back"}})
    return {screens={{id="recall", layers={{resource="musica:/sys/memories/recall/recall.png"}},
        hover="musica:/sys/memories/recall/recallOver.png", buttons=buttons}}}
end

return {
    id = "musica-yorino-recall-local", version = 1, title = "Yorino 原版 Recall（本地可选）",
    apply = function(ctx)
        local gallery = ctx.load("musica-yorino-gallery-common.luau")
        return gallery.install(ctx, "recall-layout.json", recall(ctx, gallery))
    end,
}
