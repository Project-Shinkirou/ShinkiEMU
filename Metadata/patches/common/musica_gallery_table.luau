-- The native CG catalogs are PE32 arrays of string VAs terminated by a null pointer.
-- A leading '*' starts a group. Selection, geometry and unlock rules are not in this table.
local cg = {}

function cg.groups(ctx, pe, file, table_rva, row_count, group_count)
    if file.machine ~= 0x14c or row_count < 1 or row_count > 16384
        or row_count % 1 ~= 0 or group_count < 1 or group_count > row_count
        or group_count % 1 ~= 0 then
        ctx.fail("UNSUPPORTED_INPUT", "invalid native CG table dimensions")
    end
    local length = (row_count + 1) * 4
    local rows = file.reader:bytes(pe.offset(file, table_rva, length), length)
    if buffer.readu32(rows, row_count * 4) ~= 0 then
        ctx.fail("UNSUPPORTED_INPUT", "native CG table has no terminator at the expected bound")
    end
    local result = {}
    for i = 0, row_count - 1 do
        local address = buffer.readu32(rows, i * 4)
        if address == 0 then ctx.fail("UNSUPPORTED_INPUT", "native CG table ends early") end
        local rva = address - file.base
        local bytes = file.reader:cstring(pe.offset(file, rva, 1), 128)
        pe.offset(file, rva, buffer.len(bytes) + 1)
        local name = buffer.tostring(bytes)
        if string.sub(name, 1, 1) == "*" then
            name = string.sub(name, 2)
            table.insert(result, {})
        end
        -- Keep composition syntax intact; the consumer must resolve its native semantics.
        if #result == 0 or not (string.match(name, "^[%w_%-]+%.png$")
            or string.match(name, "^[%w_%-]+%.png%([%w_%-]+%.png%)$")) then
            ctx.fail("UNSUPPORTED_INPUT", "invalid native CG entry")
        end
        table.insert(result[#result], name)
    end
    if #result ~= group_count then
        ctx.fail("UNSUPPORTED_INPUT", "native CG group count disagrees with the layout")
    end
    return result
end

return cg
