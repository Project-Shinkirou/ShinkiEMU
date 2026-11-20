@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var destination: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var scratch: texture_storage_2d<rgba32float, write>;

fn sinc(x: f32) -> f32 {
    if (x == 0.0) {
        return 1.0;
    }
    let angle = 3.141592653589793 * x;
    return sin(angle) / angle;
}

fn load(p: vec2i, premultiplied: bool) -> vec4f {
    let rgba = textureLoad(source, p, 0);
    if (premultiplied) {
        return rgba;
    }
    return vec4f(rgba.rgb * rgba.a, rgba.a);
}

// Each pass changes at most one axis. No sampler interpolation, alpha division,
// or range clipping takes place between the two convolutions.
fn resample(p: vec2u, dst_size: vec2u, premultiplied: bool) -> vec4f {
    let src_size = textureDimensions(source);
    let horizontal = src_size.x != dst_size.x;
    let src = select(src_size.y, src_size.x, horizontal);
    let dst = select(dst_size.y, dst_size.x, horizontal);
    let i = select(p.y, p.x, horizontal);

    // Split ((i + 0.5) * src / dst - 0.5) into integer center and phase
    // before conversion to f32. This preserves the phase even on large images.
    let numerator = (2u * i + 1u) * src;
    let denominator = 2u * dst;
    var center = i32(numerator / denominator);
    let remainder = i32(numerator % denominator) - i32(dst);
    var phase = f32(remainder) / f32(denominator);
    if (remainder < 0) {
        center -= 1;
        phase += 1.0;
    }

    let filter_scale = min(1.0, f32(dst) / f32(src));
    let last = i32(src) - 1;
    if (filter_scale == 1.0 && phase == 0.0) {
        let j = clamp(center, 0, last);
        return load(select(vec2i(i32(p.x), j), vec2i(j, i32(p.y)), horizontal), premultiplied);
    }
    let radius = 3.0 / filter_scale;
    var sum = vec4f(0.0);
    var total = 0.0;
    for (var offset = i32(ceil(phase - radius)); offset <= i32(floor(phase + radius)); offset++) {
        let x = (f32(offset) - phase) * filter_scale;
        if (abs(x) < 3.0) {
            let weight = sinc(x) * sinc(x / 3.0);
            let j = clamp(center + offset, 0, last);
            let q = select(vec2i(i32(p.x), j), vec2i(j, i32(p.y)), horizontal);
            sum += weight * load(q, premultiplied);
            total += weight;
        }
    }
    return sum / total;
}

// The published texture holds straight alpha in the same UNORM color space as
// the input. Divide by the *unclamped* alpha before UNORM clips any overshoot.
fn straight(premul: vec4f) -> vec4f {
    if (premul.a < 0.5 / 255.0) {
        return vec4f(0.0);
    }
    return vec4f(premul.rgb / premul.a, premul.a);
}

@compute @workgroup_size(8, 8)
fn first(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(scratch);
    if (all(id.xy < size)) {
        textureStore(scratch, id.xy, resample(id.xy, size, false));
    }
}

@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(destination);
    if (all(id.xy < size)) {
        textureStore(destination, id.xy, straight(resample(id.xy, size, true)));
    }
}

@compute @workgroup_size(8, 8)
fn direct(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(destination);
    if (all(id.xy < size)) {
        if (all(size == textureDimensions(source))) {
            textureStore(destination, id.xy, textureLoad(source, id.xy, 0));
        } else {
            textureStore(destination, id.xy, straight(resample(id.xy, size, false)));
        }
    }
}
