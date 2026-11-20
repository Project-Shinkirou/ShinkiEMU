"""Render synthetic RGBA8 fixtures using the unconverted upstream GLSL in OpenGL."""

from pathlib import Path
import re
import sys

import moderngl

ROOT = Path(__file__).resolve().parent
RESTORE = ROOT / "Anime4K_Restore_CNN_S.glsl"
UPSCALE = ROOT / "Anime4K_Upscale_CNN_x2_S.glsl"
VERTEX = """#version 330
void main() {
    vec2 p = vec2((gl_VertexID & 1) * 4 - 1, (gl_VertexID >> 1) * 4 - 1);
    gl_Position = vec4(p, 0.0, 1.0);
}
"""


def render(ctx, main, model, final_type):
    """Supply mpv's hook macros, normalized coordinates and texture/FBO formats."""
    textures = {"MAIN": main}
    blocks = model.read_text(encoding="ascii").split("//!DESC ")[1:]
    for index, block in enumerate(blocks):
        resize = model == UPSCALE and index == len(blocks) - 1
        save = re.search(r"^//!SAVE (\S+)", block, re.M)[1]
        width, height = textures["MAIN"].size
        extent = (2 * width, 2 * height) if resize else (width, height)
        types = ["#version 330", "out vec4 fragment;"]
        types.append(f"#define uv (gl_FragCoord.xy / vec2({extent[0]}, {extent[1]}))")
        binds = re.findall(r"^//!BIND (\S+)", block, re.M)
        for slot, name in enumerate(binds):
            # Integer offsets are in source texels and the sampler clamps at edges.
            types.extend([
                f"uniform sampler2D input_{slot};",
                f"#define {name}_size vec2(textureSize(input_{slot}, 0))",
                f"#define {name}_pt (1.0 / {name}_size)",
                f"#define {name}_pos uv",
                f"#define {name}_tex(p) texture(input_{slot}, (p))",
                f"#define {name}_texOff(d) {name}_tex({name}_pos + (d) * {name}_pt)",
            ])
        source = block[block.index("#define go_") if "#define go_" in block else block.index("vec4 hook()") :]
        # Keep the upstream GLSL hook intact. Like production, preserve the base alpha
        # at Upscale output; the shader's video-only alpha residual is not presented.
        alpha = "fragment.a = MAIN_tex(MAIN_pos).a;" if resize else ""
        fragment = "\n".join([*types, source, f"void main() {{ fragment = hook(); {alpha} }}"])
        program = ctx.program(vertex_shader=VERTEX, fragment_shader=fragment)
        vao = ctx.vertex_array(program, [])
        for slot, name in enumerate(binds):
            textures[name].use(slot)
            program[f"input_{slot}"].value = slot
        out_type = final_type if index == len(blocks) - 1 else "f2"
        out = ctx.texture(extent, 4, dtype=out_type)
        out.filter = (moderngl.LINEAR, moderngl.LINEAR)
        out.repeat_x = out.repeat_y = False
        fbo = ctx.framebuffer([out])
        fbo.use()
        vao.render(moderngl.TRIANGLES, vertices=3)
        fbo.release()
        vao.release()
        program.release()
        textures[save] = out
    return textures["MAIN"]


def pixels(width, height, phase):
    # Asymmetric chroma/alpha checker edges with a corner and center impulse.
    data = bytearray()
    for y in range(height):
        for x in range(width):
            edge = 73 * ((x + y + phase) % 2)
            impulse = 101 if (x, y) in [(0, height - 1), (width // 2, height // 2)] else 0
            data.extend([
                (27 * x + 5 * y + edge + phase + impulse) % 256,
                (13 * x + 37 * y + 3 * phase + impulse) % 256,
                (9 * x - 25 * y + edge + 7 * phase) % 256,
                (67 * x + 59 * y + 11 * phase) % 256,
            ])
    return data


def main():
    ctx = moderngl.create_standalone_context(require=330)
    print(ctx.info["GL_RENDERER"], ctx.info["GL_VERSION"], file=sys.stderr)
    rows = []
    for label, w, h, phase in [
        ("odd-first", 11, 9, 1),
        ("odd-second", 11, 9, 2),
        ("column", 1, 5, 0),
        ("row", 7, 1, 3),
        ("single", 1, 1, 4),
    ]:
        data = pixels(w, h, phase)
        image = ctx.texture((w, h), 4, data, dtype="f1")
        image.filter = (moderngl.LINEAR, moderngl.LINEAR)
        image.repeat_x = image.repeat_y = False
        restored = render(ctx, image, RESTORE, "f1")
        upscaled = render(ctx, image, UPSCALE, "f1")
        restored_float = render(ctx, image, RESTORE, "f2")
        combined = render(ctx, restored_float, UPSCALE, "f1")
        rows.append(f"{label} {w} {h}\n" + "\n".join(
            block.hex() for block in (data, restored.read(), upscaled.read(), combined.read())
        ))
    sys.stdout.buffer.write(("\n\n".join(rows) + "\n").encode("ascii"))


if __name__ == "__main__":
    main()
