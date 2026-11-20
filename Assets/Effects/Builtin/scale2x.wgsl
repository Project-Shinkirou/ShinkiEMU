@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var destination: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(source);
    if (any(id.xy >= size)) {
        return;
    }
    let p = vec2i(id.xy);
    let hi = vec2i(size) - vec2i(1);
    let e = textureLoad(source, p, 0);
    let b = textureLoad(source, vec2i(p.x, max(p.y - 1, 0)), 0);
    let d = textureLoad(source, vec2i(max(p.x - 1, 0), p.y), 0);
    let f = textureLoad(source, vec2i(min(p.x + 1, hi.x), p.y), 0);
    let h = textureLoad(source, vec2i(p.x, min(p.y + 1, hi.y)), 0);
    let q = p * 2;
    // RGBA8 UNORM loads are injective, so equality is exact byte equality.
    if (any(b != h) && any(d != f)) {
        textureStore(destination, q, select(e, d, all(d == b)));
        textureStore(destination, q + vec2i(1, 0), select(e, f, all(b == f)));
        textureStore(destination, q + vec2i(0, 1), select(e, d, all(d == h)));
        textureStore(destination, q + vec2i(1, 1), select(e, f, all(h == f)));
    } else {
        textureStore(destination, q, e);
        textureStore(destination, q + vec2i(1, 0), e);
        textureStore(destination, q + vec2i(0, 1), e);
        textureStore(destination, q + vec2i(1, 1), e);
    }
}
