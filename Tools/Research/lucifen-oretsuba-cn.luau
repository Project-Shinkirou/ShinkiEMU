-- The patch input contains the translated SCRIPT_CHS, SYS and DATA folders.
-- The output uses the original archive names and loads the translated members from them.
local function replacements(ctx, directory, prefix, result)
    for _, entry in ipairs(ctx.list("patch", directory)) do
        ctx.check_cancelled()
        local path = directory .. "/" .. entry.name
        local name = prefix .. entry.name
        if entry.kind == "directory" then
            replacements(ctx, path, name .. "/", result)
        elseif entry.kind == "file" then
            table.insert(result, { name = name, path = path })
        else
            ctx.fail("PATH", "汉化资源目录包含链接")
        end
    end
    return result
end

return {
    id = "lucifen-oretsuba-cn",
    version = 1,
    title = "我们没有翅膀汉化资源",
    apply = function(ctx)
        ctx.lucifen_repack("SCRIPT.LPK", "gameinit.sob", {
            {
                archive = "SCRIPT.LPK",
                size = 8580821,
                sha256 = "578cd025c12d81bf094d99ece5ec87495b35143cc939e3136442fcd9b5505f3f",
                replacements = replacements(ctx, "SCRIPT_CHS", "", {}),
            },
            {
                archive = "SYS.LPK",
                size = 115566688,
                sha256 = "d08649a4d11f923c912615737473ee96033872e7668ed1dd6ae7f04e29c72155",
                replacements = replacements(ctx, "SYS", "", {}),
            },
            {
                archive = "DATA.LPK",
                size = 208840794,
                sha256 = "0a51156efbf789bf5da1ec80412e6c1658a8b2ebc61e144401f89c2c576bbc9d",
                replacements = replacements(ctx, "DATA", "", {}),
            },
        })
        local rewritten = { ["script.lpk"] = true, ["sys.lpk"] = true, ["data.lpk"] = true }
        local original_runtime = {
            ["ore_tuba.exe"] = true,
            ["ds_movie.dll"] = true,
            ["ds_sprite.dll"] = true,
            ["tvpgl.dll"] = true,
            ["wuvorbis.dll"] = true,
        }
        for _, entry in ipairs(ctx.list("source", "")) do
            local name = string.lower(entry.name)
            if entry.kind == "file" and (original_runtime[name]
                or (string.sub(name, -4) == ".lpk" and not rewritten[name])) then
                ctx.copy("source", entry.name, entry.name)
            end
        end
        return {
            plugin_id = "shinki.emu.lucifen",
            configuration = { text_encoding = "gbk" },
            required_capabilities = 256,
        }
    end,
}
