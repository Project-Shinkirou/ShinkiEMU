use super::*;

pub(super) struct FilteredImage {
    uri: String,
    parameters: [u32; 3],
    key: String,
}

/// `sub_43C2A0` operates on BGRA storage; the public frame is RGBA.
fn changes_pixels(mode: u32) -> bool {
    matches!(mode, 1 | 2 | 17 | 256 | 258 | 512 | 514)
}

pub(super) fn frame(
    textures: &mut TextureCache,
    cache: &mut BTreeMap<CmvsTextureSlot, FilteredImage>,
    parent: CmvsTextureSlot,
    uri: &str,
    parameters: [u32; 3],
    mut source: TextureFrame,
) -> Result<TextureFrame, CoreError> {
    if !changes_pixels(parameters[0]) {
        return Ok(source);
    }
    if let Some(cached) = cache
        .get(&parent)
        .filter(|entry| entry.uri == uri && entry.parameters == parameters)
        && let Some(frame) = textures.get(&cached.key)
    {
        return Ok(frame);
    }
    apply(&mut source, parameters)?;
    let key = format!("cmvs.filtered.{parent:?}");
    textures
        .insert(key.clone(), source.clone())
        .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
    cache.insert(
        parent,
        FilteredImage {
            uri: uri.into(),
            parameters,
            key,
        },
    );
    Ok(source)
}

pub(super) fn apply(frame: &mut TextureFrame, parameters: [u32; 3]) -> Result<(), CoreError> {
    let [mode, color, kernel] = parameters;
    if !changes_pixels(mode) {
        return Ok(());
    }
    let factors = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
    let pixels = frame.rgba8.make_mut_for_update();
    if matches!(mode, 1 | 2 | 17) {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            if mode == 2 {
                multiply(pixel, factors);
            } else {
                let gray = if mode == 1 {
                    (u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])) / 3 + 1
                } else {
                    (76 * u32::from(pixel[0])
                        + 150 * u32::from(pixel[1])
                        + 29 * u32::from(pixel[2]))
                        >> 8
                };
                for channel in 0..3 {
                    pixel[channel] = ((gray * u32::from(factors[channel])) >> 8) as u8;
                }
            }
        }
        return Ok(());
    }
    let radius = (kernel & 0xffff).max(1) as i32;
    let step = (kernel >> 16).max(1) as usize;
    let weighted = matches!(mode, 256 | 258);
    let tinted = matches!(mode, 258 | 514);
    let width = frame.width as usize;
    let mut output = vec![0; width * 4];
    for row in pixels.chunks_exact_mut(width * 4) {
        for x in 0..width {
            let mut sums = [0_u32; 4];
            let mut weight_sum = 0_u32;
            for offset in (-radius..=radius).step_by(step) {
                let source = x as i32 + offset;
                if source < 0 || source >= width as i32 {
                    continue;
                }
                let weight = if weighted {
                    (radius - offset.abs() + 1) as u32
                } else {
                    1
                };
                weight_sum = weight_sum.wrapping_add(weight);
                for channel in 0..4 {
                    sums[channel] = sums[channel].wrapping_add(
                        weight.wrapping_mul(u32::from(row[source as usize * 4 + channel])),
                    );
                }
            }
            if weight_sum == 0 {
                return Err(error("SHINKI_EMU_CMVS_IMAGE_FILTER_KERNEL"));
            }
            let pixel = &mut output[x * 4..x * 4 + 4];
            for channel in 0..4 {
                pixel[channel] = (sums[channel] / weight_sum) as u8;
            }
            if tinted {
                multiply(pixel, factors);
            }
        }
        row.copy_from_slice(&output);
    }
    Ok(())
}

fn multiply(pixel: &mut [u8], factors: [u8; 3]) {
    for channel in 0..3 {
        pixel[channel] =
            ((u32::from(pixel[channel]) * u32::from(factors[channel])) >> 7).min(255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tint_and_multiply_use_bgr_parameters_without_changing_alpha() {
        let original = vec![120, 60, 30, 77];
        let mut frame = TextureFrame {
            width: 1,
            height: 1,
            rgba8: original.clone().into(),
        };
        apply(&mut frame, [1, 0x00804020, 0]).unwrap();
        assert_eq!(frame.rgba8.as_slice(), &[35, 17, 8, 77]);
        frame.rgba8 = original.clone().into();
        apply(&mut frame, [2, 0x00ff8040, 0]).unwrap();
        assert_eq!(frame.rgba8.as_slice(), &[239, 60, 15, 77]);
        frame.rgba8 = original.into();
        apply(&mut frame, [17, 0x00ffffff, 0]).unwrap();
        assert_eq!(frame.rgba8.as_slice(), &[73, 73, 73, 77]);
    }
    #[test]
    fn row_filters_use_native_weights_and_clipped_edge_normalization() {
        let data = vec![0, 0, 0, 0, 120, 120, 120, 120, 240, 240, 240, 240];
        let mut weighted = TextureFrame {
            width: 3,
            height: 1,
            rgba8: data.clone().into(),
        };
        apply(&mut weighted, [256, 0, 0x00010001]).unwrap();
        assert_eq!(
            weighted.rgba8.as_slice(),
            &[40, 40, 40, 40, 120, 120, 120, 120, 200, 200, 200, 200]
        );
        let mut box_blur = TextureFrame {
            width: 3,
            height: 1,
            rgba8: data.into(),
        };
        apply(&mut box_blur, [512, 0, 0x00010001]).unwrap();
        assert_eq!(
            box_blur.rgba8.as_slice(),
            &[60, 60, 60, 60, 120, 120, 120, 120, 180, 180, 180, 180]
        );
    }
}
