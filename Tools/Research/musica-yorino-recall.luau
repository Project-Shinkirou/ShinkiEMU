-- Opt-in local recipe for the original Japanese release. The catalogued
-- translation keeps its existing direct-start recipe and receipt.
local function require_input(ctx, condition, message)
    if not condition then ctx.fail("UNSUPPORTED_INPUT", message) end
end

local function recall(ctx)
    local path = "yorino.exe"
    require_input(ctx, ctx.exists("source", path), "Recall requires the original Japanese executable")
    local original = ctx.open("source", path)
    require_input(ctx, original:size() == 2278400 and original:sha256() ==
        "0664a773545cccfa04f6f0e840fbf6302913a721bc2fc956cf206111b9c93553",
        "unsupported original Recall executable")
    local pe = ctx.load("common/pe.luau")
    local file = pe.parse(ctx, original)
    require_input(ctx, file.machine == 0x14c, "Recall requires a PE32 executable")
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
        local layout = recall(ctx)
        local path = "recall-layout.json"
        ctx.create(path):write(0, buffer.fromstring(ctx.json.encode(layout)))
        -- The deterministic E lane is an explicit one-screen title entry to
        -- Recall, not a reconstruction of the native time/route title menu.
        local frontend = ctx.json.encode({gallery_layout=path, titles={{
            background="musica:/sys/topmenu/TopMenuE.png",
            hover="musica:/sys/topmenu/TopMenuE3.png",
            buttons={{rect={696,236,164,484},action="memories"}},
        }}})
        ctx.musica_validate_title_frontend("key.toml", frontend, "shift_jis", 1280, 720, "source")
        return {plugin_id="shinki.emu.musica", configuration={
            profile_file="key.toml", script_encoding="shift_jis", launch_mode="title", frontend=frontend,
        }}
    end,
}
