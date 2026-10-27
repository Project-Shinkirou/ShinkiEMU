/// Auto-advance duration shared by native 467060 and 467510.
///
/// Integer products and subtraction wrap at 32 bits. The native four-glyph
/// unrolling still adds each ramp value separately in f32; a closed-form sum
/// would change its rounding. Native truncates the nonnegative accumulation to
/// signed 64 bits, then consumes only its low 32 bits (not a saturating u32 cast).
pub(super) fn auto_delay_ms(glyphs: u32, base: u32, per_glyph: u32, acceleration: u32) -> u32 {
    let raw = glyphs
        .wrapping_mul(per_glyph)
        .wrapping_add(base.wrapping_mul(20))
        .wrapping_mul(5);
    if acceleration == 0 || per_glyph == 0 {
        return raw;
    }

    let limit = per_glyph.wrapping_mul(5) as f32;
    let step = acceleration as f32 * 0.25;
    let mut ramp = 0.0_f32;
    let mut accumulated = 0.0_f32;
    for _ in 0..glyphs {
        accumulated += ramp;
        if ramp < limit {
            // Native retains the increment that crosses the limit.
            ramp += step;
        }
    }

    let adjusted = raw.wrapping_sub((accumulated as u64) as u32);
    let duration = if adjusted > 0xFFFF { raw } else { adjusted };
    duration.max(base.wrapping_mul(100))
}

#[cfg(test)]
mod tests {
    use super::auto_delay_ms;

    #[test]
    fn zero_glyphs_keep_the_base_duration() {
        assert_eq!(auto_delay_ms(0, 6, 7, 0), 600);
        assert_eq!(auto_delay_ms(0, 6, 7, 3), 600);
        assert_eq!(auto_delay_ms(0, 0, 7, 3), 0);
    }

    #[test]
    fn disabled_ramp_uses_wrapping_raw_duration_without_the_floor() {
        assert_eq!(auto_delay_ms(4, 2, 6, 0), 320);
        assert_eq!(auto_delay_ms(4, 2, 0, 9), 200);
        assert_eq!(auto_delay_ms(u32::MAX, 1, 1, 0), 95);
        assert_eq!(auto_delay_ms(1, 0, u32::MAX, 0), 4_294_967_291);
    }

    #[test]
    fn fractional_reduction_is_truncated_after_accumulation() {
        // Ramps 0, .25, .5, .75, 1 give reductions .75, 1.5, 2.5.
        assert_eq!(auto_delay_ms(3, 1, 1, 1), 115);
        assert_eq!(auto_delay_ms(4, 1, 1, 1), 119);
        assert_eq!(auto_delay_ms(5, 1, 1, 1), 123);
    }

    #[test]
    fn ramp_conversion_and_each_addition_keep_native_f32_rounding() {
        // Conversion rounds the acceleration up before division by four.
        assert_eq!(auto_delay_ms(2, 0, 1_677_722, 67_108_879), 0);
        // Three additions of 16_777_218 round to 50_331_656, not 50_331_654.
        assert_eq!(auto_delay_ms(4, 0, 2_516_583, 67_108_872), 4);
    }

    #[test]
    fn truncated_accumulation_wraps_to_its_low_32_bits() {
        // The rounded acceleration gives a 2^30 ramp; four additions give 2^32.
        assert_eq!(auto_delay_ms(5, 0, 1, u32::MAX), 25);
        assert_eq!(auto_delay_ms(6, 0, 1, u32::MAX), 30);
    }

    #[test]
    fn overshoot_is_retained_until_the_base_floor() {
        // The 3-ms increment passes the 5-ms limit: ramps are 0, 3, 6, 6, ... .
        for (glyphs, expected) in [
            (1, 105),
            (2, 107),
            (3, 106),
            (4, 105),
            (8, 101),
            (9, 100),
            (10, 100),
        ] {
            assert_eq!(auto_delay_ms(glyphs, 1, 1, 12), expected);
        }
    }

    #[test]
    fn unsigned_underflow_restores_raw_before_applying_the_floor() {
        // At 109 glyphs reduction and raw duration both equal 645 ms.
        // The next glyph makes reduction 651 ms versus a 650-ms raw duration.
        assert_eq!(auto_delay_ms(109, 1, 1, 12), 100);
        assert_eq!(auto_delay_ms(110, 1, 1, 12), 650);
    }

    #[test]
    fn adjusted_ffff_is_accepted_but_larger_values_restore_raw() {
        assert_eq!(auto_delay_ms(2, 655, 7, 140), 65_535);
        assert_eq!(auto_delay_ms(2, 655, 8, 160), 65_580);
    }

    #[test]
    fn ramp_limit_uses_the_wrapping_unsigned_product() {
        // Five times 0xCCCC_CCCD wraps to 1, so only the first increment runs.
        assert_eq!(auto_delay_ms(4, 0, 0xCCCC_CCCD, 4), 1);
    }
}
