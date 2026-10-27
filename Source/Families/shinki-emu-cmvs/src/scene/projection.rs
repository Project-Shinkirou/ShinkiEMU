use super::*;

pub(super) fn reference_scale(stage_width: u32, reference_width: u32) -> f32 {
    if reference_width == 0 {
        1.0
    } else {
        stage_width as f32 / reference_width as f32
    }
}

pub(super) struct CameraProjection {
    pub(super) position: [f32; 2],
    pub(super) scale: f32,
    pub(super) alpha: u32,
    pub(super) rotation: f32,
    pub(super) center: [f32; 2],
}

/// Native 457BF0 defaults and 4566B0 plane projection. Per-corner rotations
/// and quantization are applied separately by basic_geometry, without W division.
pub(super) fn project(
    vm: &CmvsPs2aVmState,
    bank: u32,
    position: [f32; 3],
    baseline: f32,
    height: f32,
) -> Option<CameraProjection> {
    let words = if bank == 0 {
        Some(&vm.screen_words)
    } else {
        vm.screen_camera_words.get(&bank)
    };
    let raw = |index, default| {
        words
            .and_then(|words| words.get(&index))
            .copied()
            .unwrap_or(default)
    };
    // 425F40 draws camera-one's queue only when 456E60 returns true.
    // 432C60 enables camera zero explicitly; camera one keeps ctor flag 0.
    if bank == 1 && raw(37, 0) == 0 {
        return None;
    }
    let f = |index, default: f32| f32::from_bits(raw(index, default.to_bits()));
    let mode = raw(0, 1);
    let [x, y, z] = position;
    let depth = z - f(3, 0.0);
    let output = [f(9, 1024.0), f(10, 614.0)];
    let center = [output[0] * 0.5, output[1] * 0.5];
    let cx = f(1, 0.0);
    let cy = f(2, 3.0);
    let far = f(6, 10.0);
    let width = f(7, 10.0);
    let world_height = f(8, 6.0);
    let slope = f(11, 24.0);
    let (position, scale, alpha) = match mode {
        1 => {
            if depth <= 0.0 {
                return None;
            }
            let difference = (baseline - depth) * 0.5;
            let scale = if difference < 0.0 {
                1.0 / (difference.abs() + 1.0)
            } else {
                difference + 1.0
            };
            let perspective = far / depth;
            (
                [
                    (x - cx) * perspective * (output[0] / width) + center[0],
                    ((cy - y) * perspective + height) * (output[1] / world_height)
                        + center[1]
                        + (baseline - depth) * slope,
                ],
                scale,
                if depth >= 0.5 {
                    255
                } else {
                    (depth * 512.0) as u32
                },
            )
        }
        2 | 3 => {
            if depth <= 9.0 {
                return None;
            }
            let yaw = f(13, 0.0);
            let shift = if yaw == 0.0 {
                0.0
            } else {
                (width * 0.5) * (yaw / 45.0) * (depth / far)
            };
            let perspective = baseline / depth;
            let ratio = depth / far;
            let mut horizontal = output[0] / (ratio * width);
            let mut vertical = output[1] / world_height;
            let slope_offset = if mode == 2 {
                (baseline - depth + 0.3) * slope
            } else {
                (baseline - depth) * slope
            };
            if mode == 3 {
                horizontal /= f(17, 1.0);
                vertical = output[1] / (world_height * ratio) / f(18, 1.0);
            }
            (
                [
                    horizontal * ((x - (cx + shift)) * perspective) + f(15, 512.0),
                    ((cy - y) * perspective + height) * vertical + f(16, 307.0) + slope_offset,
                ],
                perspective * perspective,
                if depth >= 10.0 {
                    255
                } else {
                    ((depth - 9.0) * 256.0) as u32
                },
            )
        }
        // 4566B0's other modes leave the output scratch unchanged. They do
        // not define drawable camera geometry.
        _ => return None,
    };
    if position
        .iter()
        .chain([&scale])
        .any(|value| !value.is_finite())
        || scale <= 0.0
    {
        return None;
    }
    Some(CameraProjection {
        position,
        scale,
        alpha,
        rotation: f(14, 0.0),
        center,
    })
}
