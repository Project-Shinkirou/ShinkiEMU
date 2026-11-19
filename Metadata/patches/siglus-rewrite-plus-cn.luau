-- Resource-only merge for the installed 2017 A·T·Field Chinese edition.
-- The native core selects the CHS pair, G01 images and mov_chs movies.
-- The original launcher's five machine-code patches remain unimplemented.
local function pinned(ctx, root, path, size, hash)
    local input = ctx.open(root, path)
    ctx.check(path .. " size", input:size() == size, "Requires the Complete Chinese Patch V2 input")
    ctx.check(path .. " SHA256", input:sha256() == hash, "Requires the pinned 2017 Chinese edition")
end

local function resources(path, kind)
    -- Preserve native game resource directories, never launchers or savedata.
    return kind == "directory" or kind == "file"
end

local function localized_images(path, kind)
    return kind == "directory" or (kind == "file" and string.lower(string.sub(path, -4)) == ".g01")
end

return {
    id = "siglus-rewrite-plus-cn",
    version = 1,
    title = "Rewrite+ 中文资源合并",
    apply = function(ctx)
        local copy = ctx.load("common/copy.luau")
        pinned(ctx, "source", "SiglusEngine.exe", 6003200,
            "739153681fbb0bc1d98f8a41f4e4ee94b35b01c4f61f47b99ba9bbd97ccc5570")
        -- --patch defaults to --source. A separate patch directory may supply
        -- the CHS pair, g00/*.g01 and mov_chs/ at their native relative paths.
        local chinese = ctx.exists("patch", "Gameexe.chs") and "patch" or "source"
        pinned(ctx, chinese, "Gameexe.chs", 32012,
            "4ca0b84e8ef1a855de9375eb045a5074887e3c8118ee326058a0106ebcd81073")
        pinned(ctx, chinese, "Scene.chs", 22256752,
            "ef787ae9ff5fa73236d479bd1d3842364d2361923ef2b271dc39291ab0a6b100")

        -- Keep the source executable byte-for-byte; never install a wrapper.
        copy.files(ctx, "source", {"SiglusEngine.exe", "Gameexe.dat", "Scene.pck"})
        if ctx.exists("source", "key.toml") then
            copy.files(ctx, "source", {"key.toml"})
        end
        for _, directory in {"bgm", "dat", "koe", "mov", "wav", "font", "fonts"} do
            if ctx.exists("source", directory) then
                copy.tree(ctx, "source", directory, directory, resources)
            end
        end
        copy.files(ctx, chinese, {"Gameexe.chs", "Scene.chs"})
        -- ctx.copy creates new files, so select each image/movie only once.
        if ctx.exists("source", "g00") then
            copy.tree(ctx, "source", "g00", "g00", function(path, kind)
                if kind == "file" and localized_images(path, kind) and ctx.exists(chinese, path) then
                    return false
                end
                return resources(path, kind)
            end)
        end
        if ctx.exists(chinese, "g00") then
            copy.tree(ctx, chinese, "g00", "g00", localized_images)
        end
        local movies = ctx.exists(chinese, "mov_chs") and chinese or "source"
        if ctx.exists(movies, "mov_chs") then
            copy.tree(ctx, movies, "mov_chs", "mov_chs", resources)
        end
        return {
            plugin_id = "siglus_rs",
            configuration = {resource_language = "chs"},
        }
    end,
}
