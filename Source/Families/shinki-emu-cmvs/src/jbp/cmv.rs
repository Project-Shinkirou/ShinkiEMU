//! CMV's MMX transform differs from PB's scalar JBP transform. These helpers
//! preserve the native signed saturation and rounding without changing PB.
use super::{clamp, wrap_i16};

const ROW_TABLES: [[i16; 32]; 4] = [
    [
        16384, 16384, 16384, -16384, 21407, 8867, 8867, -21407, 16384, -16384, 16384, 16384, -8867,
        21407, -21407, -8867, 22725, 12873, 19266, -22725, 19266, 4520, -4520, -12873, 12873, 4520,
        4520, 19266, -22725, 19266, -12873, -22725,
    ],
    [
        22725, 22725, 22725, -22725, 29692, 12299, 12299, -29692, 22725, -22725, 22725, 22725,
        -12299, 29692, -29692, -12299, 31521, 17855, 26722, -31521, 26722, 6270, -6270, -17855,
        17855, 6270, 6270, 26722, -31521, 26722, -17855, -31521,
    ],
    [
        21407, 21407, 21407, -21407, 27969, 11585, 11585, -27969, 21407, -21407, 21407, 21407,
        -11585, 27969, -27969, -11585, 29692, 16819, 25172, -29692, 25172, 5906, -5906, -16819,
        16819, 5906, 5906, 25172, -29692, 25172, -16819, -29692,
    ],
    [
        19266, 19266, 19266, -19266, 25172, 10426, 10426, -25172, 19266, -19266, 19266, 19266,
        -10426, 25172, -25172, -10426, 26722, 15137, 22654, -26722, 22654, 5315, -5315, -15137,
        15137, 5315, 5315, 22654, -26722, 22654, -15137, -26722,
    ],
];

fn sat(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}
fn add(a: i16, b: i16) -> i16 {
    a.saturating_add(b)
}
fn sub(a: i16, b: i16) -> i16 {
    a.saturating_sub(b)
}
fn high(a: i16, b: i16) -> i16 {
    ((i32::from(a) * i32::from(b)) >> 16) as i16
}

/// Native 4c6140: row-dependent fixed-point matrix, then saturating columns.
pub(crate) fn inverse_dct(values: &mut [i16; 64], quant: &[i16; 64]) {
    for (value, quant) in values.iter_mut().zip(quant) {
        *value = wrap_i16(i64::from(*value) * i64::from(*quant));
    }
    for row in 0..8 {
        let t = &ROW_TABLES[[0, 1, 2, 3, 0, 3, 2, 1][row]];
        let x = &values[row * 8..row * 8 + 8];
        let dot = |a: usize, b: usize, odd: bool| -> i32 {
            let p = if odd { 1 } else { 0 };
            [
                i32::from(x[p]) * i32::from(t[a]),
                i32::from(x[p + 4]) * i32::from(t[a + 1]),
                i32::from(x[p + 2]) * i32::from(t[b]),
                i32::from(x[p + 6]) * i32::from(t[b + 1]),
            ]
            .into_iter()
            .fold(0_i32, i32::wrapping_add)
        };
        let a = dot(0, 4, false).wrapping_add(1024);
        let b = dot(2, 6, false).wrapping_add(1024);
        let c = dot(8, 12, false).wrapping_add(1024);
        let d = dot(10, 14, false).wrapping_add(1024);
        let e = dot(16, 20, true);
        let f = dot(18, 22, true);
        let g = dot(24, 28, true);
        let h = dot(26, 30, true);
        let out = [
            a.wrapping_add(e),
            b.wrapping_add(f),
            c.wrapping_add(g),
            d.wrapping_add(h),
            d.wrapping_sub(h),
            c.wrapping_sub(g),
            b.wrapping_sub(f),
            a.wrapping_sub(e),
        ];
        for (dst, value) in values[row * 8..row * 8 + 8].iter_mut().zip(out) {
            *dst = sat(value >> 11);
        }
    }
    for col in 0..8 {
        let x: [i16; 8] = std::array::from_fn(|row| values[row * 8 + col]);
        let a = add(add(high(x[5], -21746), x[5]), x[3]);
        let b = sub(x[5], add(high(x[3], -21746), x[3]));
        let c = add(high(x[7], 13036), x[1]);
        let d = add(high(x[6], 27146), x[2]);
        let e = sub(high(x[1], 13036), x[7]);
        let f = sub(high(x[2], 27146), x[6]);
        let g = add(sub(e, b), 1);
        let h = add(add(a, c), 1);
        let i = sub(c, a);
        let j = add(i, g);
        let k = sub(i, g);
        let j = add(j, high(j, -19195)) | 1;
        let k = add(k, high(k, -19195)) | 1;
        let l = add(x[4], x[0]);
        let m = sub(x[0], x[4]);
        let n = add(add(l, d), 32);
        let o = add(add(m, f), 32);
        let p = add(sub(m, f), 31);
        let q = add(sub(l, d), 31);
        let r = add(e, b);
        let out = [
            add(h, n),
            add(o, j),
            add(p, k),
            add(r, q),
            sub(q, r),
            sub(p, k),
            sub(o, j),
            sub(n, h),
        ];
        for row in 0..8 {
            values[row * 8 + col] = out[row] >> 6;
        }
    }
}

/// Native 4c6b30 color arithmetic, including its separate high-word green terms.
pub(crate) fn rgba(y: i16, cb: i16, cr: i16, alpha: i16) -> [u8; 4] {
    let blue = high(cb.wrapping_shl(2), 29028);
    let red = high(cr.wrapping_shl(2), 22972);
    let green = high(cb, 22544)
        .wrapping_add(high(cr, 23392).wrapping_shl(1))
        .wrapping_neg();
    let y = y.wrapping_add(128);
    [
        clamp(i64::from(add(y, red))),
        clamp(i64::from(add(y, green))),
        clamp(i64::from(add(y, blue))),
        clamp(i64::from(alpha)),
    ]
}
