//! Cmd203's GDI region predicates. Curves use GDI's 28.4 cubic path and
//! adaptive forward differences, not an analytic ellipse or a midpoint raster.
//! Numerical contracts recovered from EBOX, bEllipse/bRoundRect and BEZIER32/64;
//! boundaries independently compared with Windows Create*Rgn/PtInRegion.

use super::*;

type Point = [i64; 2];
const CIRCLE_COMPLEMENT: i64 = 1_922_922_357;
const CURVE_ERROR: i64 = 1 << 34;
const CHUNK_ERROR: i64 = 3 << 44;

pub(super) fn contains(state: &CmvsPs2aVmState, words: [u32; 7]) -> Result<bool, CoreError> {
    let [shape, left, top, right, bottom, width, height] = words;
    if shape == 4 {
        // sub_48AA10 reads the same process array as expression prefix 0x101.
        let count = top as i32;
        if count <= 0 {
            return Ok(false);
        }
        if count > 256 {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_POINTER_REGION",
                "CMVS polygon exceeds the native 256-point buffer",
            ));
        }
        let end = left.checked_add(count as u32 * 2).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_POINTER_REGION",
                "CMVS polygon variable range overflows",
            )
        })?;
        if end > crate::vm::MAX_PROCESS_INDEXED_WORDS {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_POINTER_REGION",
                "CMVS polygon variable range exceeds the process table",
            ));
        }
        let mut winding = Winding::new([i64::from(state.pointer_x), i64::from(state.pointer_y)]);
        for index in (left..end).step_by(2) {
            let point = [
                state
                    .process_indexed_words
                    .get(&index)
                    .copied()
                    .unwrap_or(0) as i32,
                state
                    .process_indexed_words
                    .get(&(index + 1))
                    .copied()
                    .unwrap_or(0) as i32,
            ];
            if !point.into_iter().all(valid_coordinate) {
                return Ok(false); // CreatePolygonRgn fails; native publishes zero.
            }
            winding.push(point.map(i64::from));
        }
        return Ok(winding.finish());
    }
    if !(1..=3).contains(&shape) {
        return Ok(false); // sub_4800F0's default arm writes result zero.
    }
    let bounds = [left, top, right, bottom].map(|word| word as i32);
    if !bounds.into_iter().all(valid_coordinate) {
        return Ok(false); // GDI rejects coordinates outside its signed 28-bit range.
    }
    let [left, top, right, bottom] = bounds;
    let (left, right) = (left.min(right), left.max(right));
    let (top, bottom) = (top.min(bottom), top.max(bottom));
    if shape == 1 {
        return Ok(state.pointer_x >= left
            && state.pointer_x < right
            && state.pointer_y >= top
            && state.pointer_y < bottom);
    }
    Ok(curve_contains(
        [left, top, right, bottom],
        (shape == 3).then_some([width as i32, height as i32]),
        [
            i64::from(state.pointer_x) * 16,
            i64::from(state.pointer_y) * 16,
        ],
    ))
}

fn valid_coordinate(value: i32) -> bool {
    (-0x0800_0000..0x0800_0000).contains(&value)
}

/// A scanline crossing at rational x contributes at ceil(x). Testing the
/// integer query against that rational value avoids sorting, division and any
/// allocation. Top endpoints participate; bottom endpoints and right edges do not.
struct Winding {
    query: Point,
    first: Option<Point>,
    last: Point,
    value: i32,
}

impl Winding {
    fn new(query: Point) -> Self {
        Self {
            query,
            first: None,
            last: [0; 2],
            value: 0,
        }
    }

    fn edge(&mut self, mut a: Point, mut b: Point) {
        let direction = if a[1] < b[1] { 1 } else { -1 };
        if direction < 0 {
            std::mem::swap(&mut a, &mut b);
        }
        let [x, y] = self.query;
        if a[1] <= y
            && y < b[1]
            && i128::from(x - a[0]) * i128::from(b[1] - a[1])
                >= i128::from(y - a[1]) * i128::from(b[0] - a[0])
        {
            self.value += direction;
        }
    }

    fn push(&mut self, point: Point) {
        if self.first.is_some() {
            self.edge(self.last, point);
        } else {
            self.first = Some(point);
        }
        self.last = point;
    }

    fn finish(mut self) -> bool {
        if let Some(first) = self.first {
            self.edge(self.last, first);
        }
        self.value != 0
    }
}

fn curve_contains(bounds: [i32; 4], diameters: Option<[i32; 2]>, query: Point) -> bool {
    let [left, top, right, bottom] = bounds;
    // EBOX orders the input, excludes right/bottom, then biases the path by
    // (-1/4,-1/4). Keep the original signed fixed-point wrap at extreme bounds.
    let l = left.wrapping_mul(16).wrapping_sub(4);
    let t = top.wrapping_mul(16).wrapping_sub(4);
    let r = right.wrapping_mul(16).wrapping_sub(28);
    let b = bottom.wrapping_mul(16).wrapping_sub(28);
    let mut rx = r.wrapping_sub(l).wrapping_add(1) >> 1;
    let mut ny = t.wrapping_sub(b).wrapping_add(1) >> 1;
    if let Some([width, height]) = diameters {
        // bRoundRect uses single-precision EFLOAT, rounds the scaled fixed
        // radii to nearest, and accepts negative diameters by magnitude.
        let scale = |diameter: i32, span: i64| {
            if span == 0 {
                0.0
            } else {
                ((diameter.unsigned_abs() as f32 / (span as f32 * 0.5)) * 0.5).min(1.0)
            }
        };
        // Native sets both scales to zero if either original dimension is zero.
        let scales = if left == right || top == bottom {
            [0.0; 2]
        } else {
            [
                scale(width, i64::from(right) - i64::from(left)),
                scale(height, i64::from(bottom) - i64::from(top)),
            ]
        };
        rx = (rx as f32 * scales[0]).round() as i32;
        ny = (ny as f32 * scales[1]).round() as i32;
    }
    let kx = ((CIRCLE_COMPLEMENT * i64::from(rx)) >> 32) as i32;
    let ky = ((CIRCLE_COMPLEMENT * i64::from(ny)) >> 32) as i32;
    let point = |x: i32, y: i32| [i64::from(x), i64::from(y)];
    let start = point(r, t.wrapping_sub(ny));
    let corners = [
        [
            start,
            point(r, t.wrapping_sub(ky)),
            point(r.wrapping_sub(kx), t),
            point(r.wrapping_sub(rx), t),
        ],
        [
            point(l.wrapping_add(rx), t),
            point(l.wrapping_add(kx), t),
            point(l, t.wrapping_sub(ky)),
            point(l, t.wrapping_sub(ny)),
        ],
        [
            point(l, b.wrapping_add(ny)),
            point(l, b.wrapping_add(ky)),
            point(l.wrapping_add(kx), b),
            point(l.wrapping_add(rx), b),
        ],
        [
            point(r.wrapping_sub(rx), b),
            point(r.wrapping_sub(kx), b),
            point(r, b.wrapping_add(ky)),
            point(r, b.wrapping_add(ny)),
        ],
    ];
    let mut winding = Winding::new(query);
    for corner in corners {
        winding.push(corner[0]);
        flatten(corner, |point| winding.push(point));
    }
    winding.finish()
}

/// Hybrid forward-difference basis: position, first difference, and the two
/// curvature errors. Halving/doubling changes the parameter step exactly.
#[derive(Clone, Copy)]
struct Basis([i64; 4]);

impl Basis {
    fn from_points([a, b, c, d]: [i64; 4], scale: i64) -> Self {
        Self([
            a.wrapping_mul(scale),
            d.wrapping_sub(a).wrapping_mul(scale),
            d.wrapping_add(b)
                .wrapping_sub(c.wrapping_mul(2))
                .wrapping_mul(6)
                .wrapping_mul(scale),
            a.wrapping_add(c)
                .wrapping_sub(b.wrapping_mul(2))
                .wrapping_mul(6)
                .wrapping_mul(scale),
        ])
    }

    fn error(self) -> i64 {
        self.0[2].wrapping_abs().max(self.0[3].wrapping_abs())
    }

    fn parent_error(self) -> i64 {
        self.0[3].wrapping_mul(4).wrapping_abs().max(
            self.0[2]
                .wrapping_mul(8)
                .wrapping_sub(self.0[3].wrapping_mul(4))
                .wrapping_abs(),
        )
    }

    fn step(&mut self) {
        let [a, b, c, d] = self.0;
        self.0 = [
            a.wrapping_add(b),
            b.wrapping_add(c),
            c.wrapping_mul(2).wrapping_sub(d),
            c,
        ];
    }

    fn halve(&mut self) {
        let [a, b, c, d] = self.0;
        let next = c.wrapping_add(d) >> 3;
        self.0 = [a, b.wrapping_sub(next) >> 1, next, d >> 2];
    }

    fn double(&mut self) {
        let [a, b, c, d] = self.0;
        self.0 = [
            a,
            b.wrapping_mul(2).wrapping_add(c),
            c.wrapping_mul(8).wrapping_sub(d.wrapping_mul(4)),
            d.wrapping_mul(4),
        ];
    }

    fn untransform(self) -> [i64; 4] {
        let [a, b, c, d] = self.0;
        let q = b.wrapping_mul(6).wrapping_sub(c);
        [
            a,
            a.wrapping_add(q.wrapping_sub(d.wrapping_mul(2)) / 18),
            a.wrapping_add(q.wrapping_mul(2).wrapping_sub(d) / 18),
            a.wrapping_add(b),
        ]
        .map(|value| (value.wrapping_add(1 << 27) >> 28) as i32 as i64)
    }
}

fn max_error(bases: &[Basis; 2]) -> i64 {
    bases.iter().map(|basis| basis.error()).max().unwrap_or(0)
}

fn adapt(bases: &mut [Basis; 2], count: &mut u32, limit: i64) {
    if max_error(bases) > limit {
        bases.iter_mut().for_each(Basis::halve);
        *count *= 2;
    }
    while *count & 1 == 0 && bases.iter().all(|basis| basis.parent_error() <= limit) {
        bases.iter_mut().for_each(Basis::double);
        *count >>= 1;
    }
}

fn flatten(points: [Point; 4], mut emit: impl FnMut(Point)) {
    let origin: Point =
        std::array::from_fn(|axis| points.iter().map(|point| point[axis]).min().unwrap_or(0));
    let fits_small = points.iter().all(|point| {
        (0..16_384).contains(&(point[0] - origin[0]))
            && (0..16_384).contains(&(point[1] - origin[1]))
    });
    if !fits_small {
        flatten_wide(points, emit);
        return;
    }
    let mut bases: [Basis; 2] = std::array::from_fn(|axis| {
        Basis::from_points(points.map(|point| point[axis] - origin[axis]), 1 << 10)
    });
    let mut count = 1_u32;
    let mut shift = 0;
    while max_error(&bases) > 65_472 << shift {
        shift += 2;
        count *= 2;
        for basis in &mut bases {
            basis.0[2] = (basis.0[2] + basis.0[3]) >> 1;
            basis.0[1] = (basis.0[1] - (basis.0[2] >> shift)) >> 1;
        }
    }
    for basis in &mut bases {
        basis.0[0] *= 8;
        basis.0[1] *= 8;
        for value in &mut basis.0[2..] {
            *value = if shift >= 3 {
                *value >> (shift - 3)
            } else {
                *value << (3 - shift)
            };
        }
        basis.step();
    }
    count -= 1;
    loop {
        emit(std::array::from_fn(|axis| {
            origin[axis] + ((bases[axis].0[0] + 4096) >> 13)
        }));
        if count == 0 {
            break;
        }
        adapt(&mut bases, &mut count, 523_776);
        bases.iter_mut().for_each(Basis::step);
        count -= 1;
    }
}

fn flatten_wide(points: [Point; 4], mut emit: impl FnMut(Point)) {
    let mut outer: [Basis; 2] =
        std::array::from_fn(|axis| Basis::from_points(points.map(|point| point[axis]), 1 << 28));
    let mut outer_count = 1_u32;
    while max_error(&outer) > CHUNK_ERROR {
        outer.iter_mut().for_each(Basis::halve);
        outer_count *= 2;
    }
    loop {
        let mut bases = outer.map(|basis| Basis::from_points(basis.untransform(), 1 << 28));
        let mut count = 1_u32;
        while max_error(&bases) > CURVE_ERROR {
            bases.iter_mut().for_each(Basis::halve);
            count *= 2;
        }
        outer_count -= 1;
        if outer_count != 0 {
            outer.iter_mut().for_each(Basis::step);
            adapt(&mut outer, &mut outer_count, CHUNK_ERROR);
        }
        loop {
            bases.iter_mut().for_each(Basis::step);
            emit(bases.map(|basis| (basis.0[0].wrapping_add(1 << 27) >> 28) as i32 as i64));
            count -= 1;
            if count == 0 {
                break;
            }
            adapt(&mut bases, &mut count, CURVE_ERROR);
        }
        if outer_count == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curved_scanlines_match_windows_gdi_oracle() {
        // Captured using real CreateEllipticRgn/CreateRoundRectRgn + PtInRegion.
        // These narrow curves distinguish the native cubic raster from both
        // an analytic ellipse and Wine's midpoint/Zingl raster.
        type Fixture<'a> = ([i32; 4], Option<[i32; 2]>, &'a [[i64; 3]]);
        let fixtures: &[Fixture<'_>] = &[
            (
                [0, 0, 8, 6],
                None,
                &[[0, 3, 4], [1, 1, 6], [2, 0, 7], [3, 1, 6], [4, 3, 4]],
            ),
            (
                [0, 0, 3, 8],
                None,
                &[[1, 0, 2], [2, 0, 2], [3, 0, 2], [4, 0, 2], [5, 0, 2]],
            ),
            (
                [0, 0, 4, 12],
                None,
                &[
                    [0, 1, 2],
                    [1, 1, 2],
                    [2, 1, 2],
                    [3, 0, 3],
                    [4, 0, 3],
                    [5, 0, 3],
                    [6, 0, 3],
                    [7, 0, 3],
                    [8, 1, 2],
                    [9, 1, 2],
                    [10, 1, 2],
                ],
            ),
            (
                [0, 0, 5, 12],
                None,
                &[
                    [1, 1, 3],
                    [2, 1, 3],
                    [3, 1, 3],
                    [4, 0, 4],
                    [5, 0, 4],
                    [6, 0, 4],
                    [7, 1, 3],
                    [8, 1, 3],
                    [9, 1, 3],
                ],
            ),
            (
                [0, 0, 0, 5],
                None,
                &[[0, -1, 0], [1, -1, 0], [2, -1, 0], [3, -1, 0]],
            ),
            (
                [-3, -2, 12, 9],
                Some([5, 8]),
                &[
                    [-2, -1, 9],
                    [-1, -2, 10],
                    [0, -2, 10],
                    [1, -3, 11],
                    [2, -3, 11],
                    [3, -3, 11],
                    [4, -3, 11],
                    [5, -2, 10],
                    [6, -2, 10],
                    [7, -1, 9],
                ],
            ),
            (
                [0, 0, 8, 6],
                Some([0, 0]),
                &[[0, 0, 7], [1, 0, 7], [2, 0, 7], [3, 0, 7], [4, 0, 7]],
            ),
            (
                [0, 0, 8, 6],
                Some([-3, -5]),
                &[[0, 1, 6], [1, 0, 7], [2, 0, 7], [3, 0, 7], [4, 1, 6]],
            ),
            (
                [0, 0, 8, 6],
                Some([1, 1]),
                &[[0, 0, 7], [1, 0, 7], [2, 0, 7], [3, 0, 7], [4, 0, 7]],
            ),
        ];
        for &(bounds, diameters, rows) in fixtures {
            for y in i64::from(bounds[1]) - 2..=i64::from(bounds[3]) + 2 {
                for x in i64::from(bounds[0]) - 2..=i64::from(bounds[2]) + 2 {
                    let expected = rows
                        .iter()
                        .any(|&[row, left, right]| row == y && left <= x && x < right);
                    assert_eq!(
                        curve_contains(bounds, diameters, [x * 16, y * 16]),
                        expected,
                        "bounds={bounds:?}, diameters={diameters:?}, point=({x},{y})"
                    );
                }
            }
        }
    }

    #[test]
    fn wide_curves_match_native_forward_difference_boundaries() {
        // Each pair straddles a native GDI scanline edge. The cases cover
        // both sides of the BEZIER32/BEZIER64 arithmetic cutoff.
        for (bounds, diameters, y, outside, inside) in [
            ([0, 0, 2048, 1200], [2048, 1200], 2, 937, 938),
            ([0, 0, 4096, 800], [4096, 800], 0, 1994, 1995),
            ([10, -10, 4000, 5000], [3000, 2000], -9, 1435, 1436),
        ] {
            assert!(!curve_contains(
                bounds,
                Some(diameters),
                [outside * 16, y * 16]
            ));
            assert!(curve_contains(
                bounds,
                Some(diameters),
                [inside * 16, y * 16]
            ));
        }
    }

    #[test]
    fn polygon_uses_nonzero_winding_and_top_left_edge_ownership() {
        let twice = [
            [0, 0],
            [4, 0],
            [4, 4],
            [0, 4],
            [0, 0],
            [4, 0],
            [4, 4],
            [0, 4],
        ];
        for y in -1..=5 {
            for x in -1..=5 {
                let mut winding = Winding::new([x, y]);
                for point in twice {
                    winding.push(point);
                }
                assert_eq!(winding.finish(), (0..4).contains(&x) && (0..4).contains(&y));
            }
        }
        let triangle = [[-3, -2], [4, 1], [-1, 6]];
        for (point, expected) in [
            ([-3, -2], false),
            ([-2, 0], true),
            ([4, 1], false),
            ([-1, 6], false),
        ] {
            let mut winding = Winding::new(point);
            triangle.into_iter().for_each(|point| winding.push(point));
            assert_eq!(winding.finish(), expected);
        }
    }

    #[test]
    fn native_creation_failures_and_polygon_memory_guards() {
        let mut state = CmvsPs2aVmState::new(0);
        state.pointer_x = 3;
        state.pointer_y = 3;
        assert!(contains(&state, [1, 5, 5, 0, 0, 0, 0]).unwrap());
        assert!(!contains(&state, [1, 0, 0, i32::MAX as u32, 5, 0, 0]).unwrap());
        assert!(!contains(&state, [2, 0, 0, i32::MAX as u32, 5, 0, 0]).unwrap());
        assert!(!contains(&state, [99, 0, 0, 5, 5, 0, 0]).unwrap());
        assert!(!contains(&state, [4, u32::MAX, 0, 0, 0, 0, 0]).unwrap());
        assert!(!contains(&state, [4, u32::MAX, u32::MAX, 0, 0, 0, 0]).unwrap());
        for words in [
            [4, 0, 257, 0, 0, 0, 0],
            [4, u32::MAX, 3, 0, 0, 0, 0],
            [4, 4091, 3, 0, 0, 0, 0],
        ] {
            assert_eq!(
                contains(&state, words).unwrap_err().code(),
                "SHINKI_EMU_CMVS_VM_POINTER_REGION"
            );
        }
        // Missing process words are native zero-initialized variables, so a
        // valid all-zero contour is simply empty.
        assert!(!contains(&state, [4, 0, 3, 0, 0, 0, 0]).unwrap());
    }
}
