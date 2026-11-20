// Anime4K v4.0.1 port; see LICENSE and the vendored GLSL notices.
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var base_image: texture_2d<f32>;
// Entry points use only one output binding, selected by output format.
@group(0) @binding(2) var output_float: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var output_unorm: texture_storage_2d<rgba8unorm, write>;

fn restore_pixel(p: vec2<i32>) -> vec4<f32> {
    // The upstream final Restore convolution has zero alpha weights and bias.
    return restore_3(p) + textureLoad(base_image, p, 0);
}

@compute @workgroup_size(8, 8)
fn restore_to_unorm(@builtin(global_invocation_id) id: vec3<u32>) {
    if all(id.xy < textureDimensions(source)) {
        let p = vec2<i32>(id.xy);
        textureStore(output_unorm, p, restore_pixel(p));
    }
}

@compute @workgroup_size(8, 8)
fn restore_to_float(@builtin(global_invocation_id) id: vec3<u32>) {
    if all(id.xy < textureDimensions(source)) {
        let p = vec2<i32>(id.xy);
        textureStore(output_float, p, restore_pixel(p));
    }
}

fn store_subpixel(p: vec2<i32>, base: vec4<f32>, residual: f32) {
    // GLSL adds the same scalar to all four channels on mpv's RGB video path.
    // This host has meaningful alpha: apply the scalar only to RGB.
    textureStore(output_unorm, p, vec4<f32>(base.rgb + vec3<f32>(residual), base.a));
}

@compute @workgroup_size(8, 8)
fn depth_to_space(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(source);
    if any(id.xy >= size) {
        return;
    }
    let p = vec2<i32>(id.xy);
    let lo = max(p - vec2<i32>(1), vec2<i32>(0));
    let hi = min(p + vec2<i32>(1), vec2<i32>(size) - vec2<i32>(1));
    let r = textureLoad(source, p, 0);
    // MAIN_tex(MAIN_pos) at a 2x destination pixel center lies at +/- 1/4
    // of a source texel from p on each axis. Reuse the common bilinear taps.
    let a = textureLoad(base_image, vec2<i32>(lo.x, lo.y), 0);
    let b = textureLoad(base_image, vec2<i32>( p.x, lo.y), 0);
    let c = textureLoad(base_image, vec2<i32>(hi.x, lo.y), 0);
    let d = textureLoad(base_image, vec2<i32>(lo.x,  p.y), 0);
    let e = textureLoad(base_image, p, 0);
    let f = textureLoad(base_image, vec2<i32>(hi.x,  p.y), 0);
    let g = textureLoad(base_image, vec2<i32>(lo.x, hi.y), 0);
    let h = textureLoad(base_image, vec2<i32>( p.x, hi.y), 0);
    let i = textureLoad(base_image, vec2<i32>(hi.x, hi.y), 0);
    let left = mix(e, d, 0.25);
    let right = mix(e, f, 0.25);
    let dst = p * 2;
    store_subpixel(dst,                  mix(left,  mix(b, a, 0.25), 0.25), r.x);
    store_subpixel(dst + vec2<i32>(1, 0), mix(right, mix(b, c, 0.25), 0.25), r.y);
    store_subpixel(dst + vec2<i32>(0, 1), mix(left,  mix(h, g, 0.25), 0.25), r.z);
    store_subpixel(dst + vec2<i32>(1, 1), mix(right, mix(h, i, 0.25), 0.25), r.w);
}
