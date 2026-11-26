-- Release geometry from the native UI constructors. Entry names and order
-- come from the user's original image, never from a checked-in catalog.
local gallery = {}
gallery.path = "localized/perseus/gallery.json"

local function need(ctx, condition, message)
    if not condition then ctx.fail("UNSUPPORTED_INPUT", message) end
end

local function uri(role, name)
    return "musica:/" .. role .. "/" .. name
end

local function layer(name, at, clip)
    return {resource = uri("sys", name), at = at, clip = clip}
end

local function button(rect, kind, fields)
    local action = fields or {}
    action.kind = kind
    return {rect = rect, action = action}
end

local function open(rect, target)
    return button(rect, "open", {screen = target})
end

local function nav(buttons, stem, p, count, rects)
    if count > 1 then
        table.insert(buttons, button(rects[1], "previous_page", {screen = stem .. ((p - 2) % count + 1)}))
        table.insert(buttons, button(rects[2], "next_page", {screen = stem .. (p % count + 1)}))
    end
    table.insert(buttons, button(rects[3], "back"))
end

function gallery.read(ctx, spec)
    need(ctx, ctx.exists("source", spec.executable),
        "Perseus gallery requires the unmodified executable in the original game directory")
    local original = ctx.open("source", spec.executable)
    need(ctx, original:size() == spec.size and original:sha256() == spec.sha256,
        "Perseus gallery requires the matching original executable")
    local pe = ctx.load("common/pe.luau")
    local file = pe.parse(ctx, original)
    need(ctx, file.machine == 0x14c, "Perseus gallery requires the original PE32 image")
    local function ptr(address)
        return file.reader:u32le(pe.offset(file, address - file.base, 4))
    end
    local function string_at(address)
        local rva = address - file.base
        local bytes = file.reader:cstring(pe.offset(file, rva, 1), 128)
        pe.offset(file, rva, buffer.len(bytes) + 1)
        return buffer.tostring(bytes)
    end
    local function operand(rva, opcode, width)
        local at = pe.offset(file, rva, #opcode + width)
        need(ctx, buffer.tostring(file.reader:bytes(at, #opcode)) == opcode,
            "unknown native gallery instruction layout")
        return if width == 1 then file.reader:u8(at + #opcode) else file.reader:u32le(at + #opcode)
    end
    local function bytes(rva, length)
        return buffer.tostring(file.reader:bytes(pe.offset(file, rva, length), length))
    end
    local function rect(edges)
        return {edges[1], edges[2], edges[3] - edges[1], edges[4] - edges[2]}
    end
    local function rect_table(rva, count)
        local result = {}
        for i = 0, count - 1 do
            local edges = {}
            for j = 0, 3 do edges[j + 1] = ptr(file.base + rva + i * 16 + j * 4) end
            result[i + 1] = rect(edges)
        end
        return result
    end
    local function root_rect(rva)
        local edges = {}
        for i = 0, 3 do
            edges[i + 1] = operand(rva + i * 8, "\184", 4)
            if i < 3 then
                need(ctx, bytes(rva + i * 8 + 5, 3) == string.char(137, 69, 208 + i * 4),
                    "unknown native menu rectangle stores")
            end
        end
        return rect(edges)
    end
    local columns = operand(spec.cg_columns_rva, "\190", 4)
    local slots = operand(spec.cg_slots_rva, "\131\248", 1)
    need(ctx, columns > 0 and slots >= columns and slots <= 29 and slots % columns == 0,
        "invalid native CG grid")
    local cg_x, cg_y = {}, {}
    for i = 0, columns - 1 do
        cg_x[i] = operand(spec.cg_grid_rva + i * 10,
            "\199\133" .. string.char(104 + i * 4) .. "\255\255\255", 4)
    end
    for i = 0, slots / columns - 1 do
        cg_y[i] = operand(spec.cg_grid_rva + (columns + i) * 10,
            "\199\133" .. string.char(84 + i * 4) .. "\255\255\255", 4)
    end
    local cg_w = operand(spec.cg_size_rva, "\141\144", 4)
    local cg_h = operand(spec.cg_size_rva + 16, "\141\81", 1)
    local nav_rects = rect_table(spec.page_controls_rva, 3)
    local flash_rows = operand(spec.flash_row_table_rva, "\139\20\133", 4) - file.base
    local flash_cols = operand(spec.flash_col_table_rva, "\139\12\181", 4) - file.base
    local flash_col_opcode = bytes(spec.flash_col_table_rva + 7, 3)
    need(ctx, flash_col_opcode == "\141\52\134", "unknown native Flash row stride")
    local flash_w = 256 - operand(spec.flash_size_rva, "\131\233", 1)
    local flash_h = operand(spec.flash_size_rva + 3, "\131\194", 1)
    local function flash_slots(start)
        local result, pc = {}, start
        while pc < spec.flash_finish_rva and #result < 20 do
            local row = operand(pc, "\106", 1)
            pc += 2
            if bytes(pc, 1) == "\235" then
                local jump = operand(pc, "\235", 1)
                need(ctx, jump < 128, "invalid native Flash branch")
                pc += 2 + jump
            end
            need(ctx, bytes(pc, 1) == "\83", "unknown native Flash control call")
            pc += 1
            local col
            if bytes(pc, 2) == "\51\201" then
                col = 0
                pc += 2
            else
                col = operand(pc, "\185", 4)
                pc += 5
            end
            local rel = operand(pc, "\232", 4)
            if rel >= 2147483648 then rel -= 4294967296 end
            pc += 5
            need(ctx, pc + rel == spec.flash_constructor_rva and col < 4 and row < 5,
                "invalid native Flash control call")
            table.insert(result, {col, row})
        end
        need(ctx, pc == spec.flash_finish_rva and #result > 0, "native Flash control list is incomplete")
        return result
    end
    local first_slots = flash_slots(spec.flash_first_rva)
    local other_slots = flash_slots(spec.flash_other_rva)
    local flash_pages = operand(spec.flash_pages_rva, "\184", 4)
    need(ctx, flash_pages > 0 and flash_pages <= 32, "invalid native Flash page count")
    local music_rects = rect_table(spec.music_controls_rva, 8)
    local row_shift = operand(spec.music_row_shift_rva, "\193\255", 1)
    local music_pages = operand(spec.music_pages_rva, "\106", 1)
    need(ctx, row_shift <= 8 and music_pages > 0 and music_pages <= 32,
        "invalid native music dimensions")
    local row_step = 2 ^ row_shift
    local rows = music_rects[1][4] / row_step
    need(ctx, rows > 0 and rows <= 64 and rows % 1 == 0, "invalid native music row count")

    local cg = ctx.load("common/musica_gallery_table.luau")
    local table_rva = ptr(file.base + spec.cg_pointer_rva) - file.base
    local groups = cg.groups(ctx, pe, file, table_rva, spec.cg_rows, spec.cg_groups)
    for _, group in groups do
        for _, name in group do
            need(ctx, not string.find(name, "(", 1, true), "unexpected composite in Perseus CG table")
        end
    end

    local screens = {{
        id = "memories", layers = {layer("memories.png")}, hover = uri("sys", "memoriesOver.png"),
        buttons = {
            open(root_rect(spec.root_buttons[1]), "cg1"),
            open(root_rect(spec.root_buttons[2]), "flash1"),
            open(root_rect(spec.root_buttons[3]), "music1"),
            button(root_rect(spec.root_buttons[4]), "script", {resource = uri("scr", string_at(file.base + spec.movie_entry_rva))}),
            button(root_rect(spec.root_buttons[5]), "back"),
        },
    }}
    local cg_pages = math.ceil(#groups / slots)
    for p = 1, cg_pages do
        local buttons, layers = {}, {layer("cgmode0.png")}
        for slot = 0, slots - 1 do
            local entries = groups[(p - 1) * slots + slot + 1]
            if not entries then break end
            local rect = {cg_x[slot % columns], cg_y[math.floor(slot / columns)], cg_w, cg_h}
            local images = {}
            for i, name in ipairs(entries) do images[i] = uri("bg", name) end
            -- In this edition the native viewer checks archive availability,
            -- not cgv.dat. Preserve group order and all original variants.
            local slot_button = button(rect, "view", {images = images})
            slot_button.thumbnail = uri("sys", "cgthumb/" .. entries[1])
            table.insert(buttons, slot_button)
            table.insert(layers, layer("cgmode0box.png", nil, rect))
        end
        table.insert(layers, layer(string.format("cgpage0%02d.png", p), {operand(spec.cg_page_x_rva, "\104", 4),0}))
        table.insert(layers, layer("cgmode0menu.png", {operand(spec.cg_menu_rva, "\199\69\164", 4),
            operand(spec.cg_menu_rva + 7, "\199\69\168", 4)}))
        nav(buttons, "cg", p, cg_pages, nav_rects)
        table.insert(screens, {id = "cg" .. p, layers = layers, buttons = buttons})
    end

    local routes = file.base + spec.flash_routes_rva
    for p = 1, flash_pages do
        local route = string_at(ptr(routes + (p - 1) * 4))
        need(ctx, string.match(route, "^%a+$"), "invalid native Flash route")
        local buttons = {}
        for _, slot in ipairs(if p == 1 then first_slots else other_slots) do
            local col, row = slot[1], slot[2]
            local name = string.format("fb_%s_%02d.sc", route, 4 * row + col)
            local x = ptr(file.base + flash_cols + col * 4)
            local y = ptr(file.base + flash_rows + row * 4)
            table.insert(buttons, button({x,y,flash_w,flash_h}, "script", {resource = uri("scr", name)}))
        end
        nav(buttons, "flash", p, flash_pages, nav_rects)
        local stem = "flash" .. (p - 1)
        table.insert(screens, {
            id = "flash" .. p, layers = {layer(stem .. ".png"), layer(stem .. "menu.png",
                {operand(spec.flash_menu_x_rva, "\199\133\124\255\255\255", 4),
                 operand(spec.flash_menu_y_rva, "\199\69\128", 4)})},
            hover = uri("sys", stem .. "Over.png"), buttons = buttons,
        })
    end

    local music = file.base + spec.music_rva
    local controls = {"previous_track", "play_selected", "stop", "next_track"}
    for p = 1, music_pages do
        local buttons = {}
        local function control(rect, kind, target)
            local b = button(rect, kind, if target then {screen = target} else nil)
            b.hover = uri("sys", "musicPageOver.png")
            table.insert(buttons, b)
        end
        for row = 0, rows - 1 do
            local index = (p - 1) * rows + row
            local track = ptr(music + index * 4)
            need(ctx, (track == 0) == (index == music_pages * rows - 1), "native music slots disagree")
            if track ~= 0 then
                local name = string_at(track)
                need(ctx, string.match(name, "^[%w_%-]+%.ogg$"), "invalid native music entry")
                local grid = music_rects[1]
                table.insert(buttons, button({grid[1],grid[2] + row * row_step,grid[3],row_step}, "track", {resource = uri("bgm", name)}))
            end
        end
        for i, kind in ipairs(controls) do control(music_rects[i + 1], kind) end
        if p > 1 then control(music_rects[6], "previous_page", "music" .. (p - 1)) end
        if p < music_pages then control(music_rects[7], "next_page", "music" .. (p + 1)) end
        control(music_rects[8], "back")
        table.insert(screens, {
            id = "music" .. p, layers = {layer("musicPage" .. p .. ".png")},
            track_group = "music", playing = layer("musicNote.png", {-17,0}), buttons = buttons,
        })
    end
    return {screens = screens}
end

function gallery.install(ctx, layout)
    ctx.mkdir("localized/perseus")
    local output = ctx.create(gallery.path)
    output:write(0, buffer.fromstring(ctx.json.encode(layout)))
    -- CAP_AUTHORED_GALLERY_CLIP: the native CG slot plates use clip.
    return gallery.path, 1024
end

return gallery
