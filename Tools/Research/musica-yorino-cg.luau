-- Local opt-in for the identified Japanese original. The catalog remains direct.
local function layout(ctx, gallery)
    local pe, file = gallery.open(ctx)
    local expect = gallery.require_input
    local function bytes(rva, size)
        return file.reader:bytes(pe.offset(file, rva, size), size)
    end
    local function word(rva)
        return buffer.readi32(bytes(rva, 4), 0)
    end
    local function immediate(rva, opcode)
        local encoded = bytes(rva, #opcode + 4)
        expect(ctx, buffer.readstring(encoded, 0, #opcode) == opcode, "unknown native CG geometry instruction")
        return buffer.readi32(encoded, #opcode)
    end
    local function rect(l, t, r, b)
        return {l,t,r-l,b-t}
    end
    expect(ctx, buffer.readstring(bytes(0x7dada, 3), 0, 2) == "\x83\xf9", "unknown native CG slot count instruction")
    local slots = buffer.readu8(bytes(0x7dadc, 1), 0)
    expect(ctx, slots == 21, "unexpected native CG slot count")
    local pane = rect(word(0x141ccc),word(0x141cd0),word(0x141cd4),word(0x141cd8))
    local top = immediate(0x7e165, "\xb9")
    local bottom = immediate(0x7e160, "\xb8")
    local previous = rect(immediate(0x7e188, "\xc7\x45\x8c"),top,immediate(0x7e18f, "\xc7\x45\x94"),bottom)
    local next = rect(immediate(0x7e196, "\xc7\x45\x9c"),top,immediate(0x7e19d, "\xc7\x45\xa4"),bottom)
    local back = rect(immediate(0x7e1a4, "\xc7\x45\xb0"),immediate(0x7e1ab, "\xc7\x45\xb4"),
        immediate(0x7e1b2, "\xc7\x45\xb8"),immediate(0x7e1b9, "\xc7\x45\xbc"))
    local tab_y = immediate(0x7e4b8, "\xc7\x45\xe0")
    -- The constructor dereferences a table pointer, then admits every '*' group.
    local table_rva = word(0x13f26c) - file.base
    local groups = ctx.load("common/musica_gallery_table.luau").groups(ctx, pe, file, table_rva, 2054, 342)
    local prefix = "musica:/sys/memories/cg/"
    local probes = {}
    local function probe(uris, png_size)
        table.insert(probes, {candidates=uris,png_size=png_size})
        return #probes
    end
    local tabs = probe({prefix .. "select.png"}, true)
    local selected_tab = probe({prefix .. "selectOver.png"}, true)
    local bindings = {}
    for i, group in ipairs(groups) do
        local variants = {}
        local thumbnail
        for j, raw in ipairs(group) do
            local base, foreground = string.match(raw, "^([%w_%-]+%.png)%(([%w_%-]+%.png)%)$")
            if not base then
                base = string.match(raw, "^([%w_%-]+%.png)$")
                expect(ctx, base, "unsupported native CG variant encoding")
            end
            -- SceneCGView 0x48136e/0x481398 searches BG (role 2), then ST (role 3).
            local background = probe({"musica:/bg/" .. base,"musica:/st/" .. base}, foreground ~= nil)
            if j == 1 then thumbnail = probe({prefix .. "thumb/" .. base}, false) end
            if foreground then
                local suffix = string.lower(string.sub(foreground, -7))
                expect(ctx, suffix ~= "_ov.png" and suffix ~= "_ml.png", "unsupported native CG foreground blend")
                local blend = if suffix == "_sc.png" then "screen" elseif suffix == "_ad.png" then "add" else "alpha"
                table.insert(variants, {background=background,foreground=probe({"musica:/st/" .. foreground}, true),blend=blend})
            else
                table.insert(variants, {background=background})
            end
        end
        bindings[i] = {variants=variants,thumbnail=thumbnail}
    end
    local resolved = ctx.musica_gallery_source_resources("key.toml", probes)
    local tab = resolved[tabs]
    local selected = resolved[selected_tab]
    local tab_width, tab_height = tab.size[1], tab.size[2]
    local count = math.ceil(#bindings / slots)
    expect(ctx, tab_width > 0 and tab_height > 0 and count * tab_width <= 1280 and tab_y + tab_height <= 720
        and selected.size[1] == tab_width and selected.size[2] == tab_height, "invalid original CG page-tab art size")
    local tab_x = math.floor((1280 - count * tab_width) / 2)
    local function id(page) return "cg-" .. page end
    local pages = {}
    for page = 1, count do
        local layers = {{resource=prefix .. "BG.png"},{resource=prefix .. "base.png"},
            {resource=prefix .. "thumbBase.png",at={pane[1],pane[2]},clip=pane}}
        local buttons = {}
        for slot = 0, slots - 1 do
            local group = bindings[(page - 1) * slots + slot + 1]
            if group == nil then break end
            local hit = 0x141cf8 + 16 * slot
            local x = word(0x141cdc + 4 * (slot % 7)) + pane[1]
            local y = word(0x141e48 + 4 * math.floor(slot / 7)) + pane[2]
            table.insert(layers, {resource=resolved[group.thumbnail].resource,at={x,y},clip=pane})
            local images = {}
            for _, variant in ipairs(group.variants) do
                local background = resolved[variant.background]
                if variant.foreground then
                    local foreground = resolved[variant.foreground]
                    -- This release positions full-canvas foregrounds over the base.
                    -- Reject another asset geometry instead of guessing its transform.
                    expect(ctx, background.size[1] == 1280 and background.size[2] == 720 and
                        foreground.size[1] == 1280 and foreground.size[2] == 720, "unsupported native composite CG image geometry")
                    table.insert(images, {base=background.resource,layers={{
                        resource=foreground.resource,at={0,0},blend=variant.blend,
                    }}})
                else
                    table.insert(images, background.resource)
                end
            end
            table.insert(buttons, {rect=rect(word(hit),word(hit+4),word(hit+8),word(hit+12)),
                action={kind="view",images=images}})
        end
        local over = prefix .. "baseOver.png"
        if page > 1 then table.insert(buttons, {rect=previous,hover=over,action={kind="previous_page",screen=id(page-1)}}) end
        if page < count then table.insert(buttons, {rect=next,hover=over,action={kind="next_page",screen=id(page+1)}}) end
        table.insert(buttons, {rect=back,hover=over,action={kind="back"}})
        for i = 1, count do
            local x = tab_x + (i - 1) * tab_width
            if i == page then
                table.insert(layers, {resource=selected.resource,at={x,tab_y}})
            else
                table.insert(buttons, {rect={x,tab_y,tab_width,tab_height},thumbnail=tab.resource,
                    preview={resource=selected.resource,at={x,tab_y}},
                    action={kind=if i < page then "previous_page" else "next_page",screen=id(i)}})
            end
        end
        pages[page] = {id=id(page),layers=layers,buttons=buttons}
    end
    return {screens=pages}
end

return {
    id="musica-yorino-cg-local", version=1, title="Yorino 原版 CG（本地可选）",
    apply=function(ctx)
        local gallery=ctx.load("musica-yorino-gallery-common.luau")
        return gallery.install(ctx, "cg-layout.json", layout(ctx, gallery))
    end,
}
