local function require_input(ctx, condition, message)
    if not condition then ctx.fail("UNSUPPORTED_INPUT", message) end
end

local function open(ctx)
    local path = "yorino.exe"
    require_input(ctx, ctx.exists("source", path), "gallery requires the original Japanese executable")
    local original = ctx.open("source", path)
    require_input(ctx, original:size() == 2278400 and original:sha256() ==
        "0664a773545cccfa04f6f0e840fbf6302913a721bc2fc956cf206111b9c93553",
        "unsupported original Yorino gallery executable")
    local pe = ctx.load("common/pe.luau")
    local file = pe.parse(ctx, original)
    require_input(ctx, file.machine == 0x14c, "gallery requires a PE32 executable")
    return pe, file
end

local function install(ctx, path, layout)
    ctx.create(path):write(0, buffer.fromstring(ctx.json.encode(layout)))
    -- A fixed title shortcut, not the native time/route title or Memories hub.
    local frontend = ctx.json.encode({gallery_layout=path, titles={{
        background="musica:/sys/topmenu/TopMenuE.png",
        hover="musica:/sys/topmenu/TopMenuE3.png",
        buttons={{rect={696,236,164,484},action="memories"}},
    }}})
    ctx.musica_validate_title_frontend("key.toml", frontend, "shift_jis", 1280, 720, "source")
    return {plugin_id="shinki.emu.musica", configuration={
        profile_file="key.toml", script_encoding="shift_jis", launch_mode="title", frontend=frontend,
    }}
end

return {require_input=require_input, open=open, install=install}
