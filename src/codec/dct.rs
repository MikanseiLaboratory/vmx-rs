//! Scalar 8×8 FDCT / IDCT with quantization and zigzag (AAN-style, matching libvmx).

use crate::tables::{
    FDCT_ROUND1, FDCT_SQRT2, FDCT_TAN1, FDCT_TAN2, FDCT_TAN3, FTAB1_128, FTAB2_128, FTAB3_128,
    FTAB4_128, IDCT_COS4, IDCT_ROW_TABLES, IDCT_TG1, IDCT_TG2, IDCT_TG3, IRND_INV_COL,
    IRND_INV_CORR, IRND_INV_ROW, RND_FRW_ROW, SHIFT_FRW_COL, SHIFT_FRW_ROW, SHIFT_INV_COL,
    SHIFT_INV_ROW, ZIGZAG_INV,
};

#[inline(always)]
fn sat_add_i16(a: i16, b: i16) -> i16 {
    a.saturating_add(b)
}

#[inline(always)]
fn sat_sub_i16(a: i16, b: i16) -> i16 {
    a.saturating_sub(b)
}

#[inline(always)]
fn mulhi_i16(a: i16, b: i16) -> i16 {
    (((a as i32) * (b as i32)) >> 16) as i16
}

#[inline(always)]
fn mulhi_u16(a: u16, b: u16) -> u16 {
    (((a as u32) * (b as u32)) >> 16) as u16
}

#[inline]
fn packs_i32(a: i32, b: i32) -> (i16, i16) {
    (
        a.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        b.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
    )
}

/// Forward DCT row transform using ftab coefficients (scalar port of SSE madd path).
fn fdct_row(input: [i16; 8], ftab: &[i16; 32]) -> [i16; 8] {
    // libvmx VMX_FDCT row block:
    //   after butterfly + unpacklo: xmm0 = [s0,s1,d0,d1,s2,s3,d2,d3]
    //   shuffle 0b01001110:         xmm2 = [s2,s3,d2,d3,s0,s1,d0,d1]
    //   temp4=madd(xmm0,ftab[0..8]);  temp1=madd(xmm2,ftab[8..16]);
    //   temp2=madd(xmm0,ftab[16..24]); temp3=madd(xmm2,ftab[24..32]);
    //   packs(srai(temp4+temp1+rnd), srai(temp3+temp2+rnd))
    let s0 = sat_add_i16(input[0], input[7]);
    let d0 = sat_sub_i16(input[0], input[7]);
    let s1 = sat_add_i16(input[1], input[6]);
    let d1 = sat_sub_i16(input[1], input[6]);
    let s2 = sat_add_i16(input[2], input[5]);
    let d2 = sat_sub_i16(input[2], input[5]);
    let s3 = sat_add_i16(input[3], input[4]);
    let d3 = sat_sub_i16(input[3], input[4]);

    let full = [s0, s1, d0, d1, s2, s3, d2, d3];
    let shuf = [s2, s3, d2, d3, s0, s1, d0, d1];

    #[inline]
    fn madd8(v: [i16; 8], f: &[i16]) -> [i32; 4] {
        [
            v[0] as i32 * f[0] as i32 + v[1] as i32 * f[1] as i32,
            v[2] as i32 * f[2] as i32 + v[3] as i32 * f[3] as i32,
            v[4] as i32 * f[4] as i32 + v[5] as i32 * f[5] as i32,
            v[6] as i32 * f[6] as i32 + v[7] as i32 * f[7] as i32,
        ]
    }

    let temp4 = madd8(full, &ftab[0..8]);
    let temp1 = madd8(shuf, &ftab[8..16]);
    let temp2 = madd8(full, &ftab[16..24]);
    let temp3 = madd8(shuf, &ftab[24..32]);

    let round = RND_FRW_ROW;
    let shift = SHIFT_FRW_ROW;
    let mut lo = [0i32; 4];
    let mut hi = [0i32; 4];
    for i in 0..4 {
        lo[i] = (temp4[i] + temp1[i] + round) >> shift;
        hi[i] = (temp3[i] + temp2[i] + round) >> shift;
    }

    let (o0, o1) = packs_i32(lo[0], lo[1]);
    let (o2, o3) = packs_i32(lo[2], lo[3]);
    let (o4, o5) = packs_i32(hi[0], hi[1]);
    let (o6, o7) = packs_i32(hi[2], hi[3]);
    [o0, o1, o2, o3, o4, o5, o6, o7]
}

fn fdct_column_stage(in_rows: [[i16; 8]; 8]) -> [[i16; 8]; 8] {
    // Process each column independently (transpose view).
    let mut cols = [[0i16; 8]; 8];
    for x in 0..8 {
        let mut c = [0i16; 8];
        for y in 0..8 {
            c[y] = in_rows[y][x];
        }

        let (mut xmm0, mut xmm2, mut xmm7, mut xmm5) = (c[0], c[2], c[7], c[5]);
        let xmm3 = xmm0;
        let xmm4 = xmm2;
        xmm0 = sat_sub_i16(xmm0, xmm7);
        xmm7 = sat_add_i16(xmm7, xmm3);
        xmm2 = sat_sub_i16(xmm2, xmm5);
        xmm5 = sat_add_i16(xmm5, xmm4);

        let (mut xmm3, mut xmm4) = (c[3], c[4]);
        let xmm1 = xmm3;
        xmm3 = sat_sub_i16(xmm3, xmm4);
        xmm4 = sat_add_i16(xmm4, xmm1);

        let (mut xmm6, mut xmm1) = (c[6], c[1]);
        let tmp = xmm1;
        xmm1 = sat_sub_i16(xmm1, xmm6);
        xmm6 = sat_add_i16(xmm6, tmp);

        let mut tm03 = sat_sub_i16(xmm7, xmm4);
        let mut tm12 = sat_sub_i16(xmm6, xmm5);
        xmm4 = sat_add_i16(xmm4, xmm4);
        xmm5 = sat_add_i16(xmm5, xmm5);
        let mut tp03 = sat_add_i16(xmm4, tm03);
        let mut tp12 = sat_add_i16(xmm5, tm12);

        xmm2 <<= SHIFT_FRW_COL + 1;
        xmm1 <<= SHIFT_FRW_COL + 1;
        tp03 <<= SHIFT_FRW_COL;
        tp12 <<= SHIFT_FRW_COL;
        tm03 <<= SHIFT_FRW_COL;
        tm12 <<= SHIFT_FRW_COL;
        xmm3 <<= SHIFT_FRW_COL;
        xmm0 <<= SHIFT_FRW_COL;

        let in4 = sat_sub_i16(tp03, tp12);
        let diff = sat_sub_i16(xmm1, xmm2);
        tp12 = sat_add_i16(tp12, tp12);
        let xmm2 = sat_add_i16(xmm2, xmm2);
        let in0 = sat_add_i16(tp12, in4);
        let sum = sat_add_i16(xmm2, diff);

        let tmp1 = mulhi_i16(FDCT_TAN2, tm03);
        let mut in6 = sat_sub_i16(tmp1, tm12);
        let tmp2 = mulhi_i16(FDCT_TAN2, tm12);
        let mut in2 = sat_add_i16(tmp2, tm03);

        let mut tp65 = mulhi_i16(sum, FDCT_SQRT2);
        in2 |= FDCT_ROUND1;
        in6 |= FDCT_ROUND1;
        let tm65 = mulhi_i16(diff, FDCT_SQRT2);
        tp65 |= FDCT_ROUND1;

        let tm465 = sat_sub_i16(xmm3, tm65);
        let tm765 = sat_sub_i16(xmm0, tp65);
        let tp765 = sat_add_i16(tp65, xmm0);
        let tp465 = sat_add_i16(tm65, xmm3);

        let mut tmp3 = mulhi_i16(tm465, FDCT_TAN3);
        let tmp4 = mulhi_i16(tp465, FDCT_TAN1);
        tmp3 = sat_add_i16(tmp3, tm465);
        let mut tmp5 = mulhi_i16(tm765, FDCT_TAN3);
        tmp5 = sat_add_i16(tmp5, tm765);
        let tmp6 = mulhi_i16(tp765, FDCT_TAN1);

        let in1 = sat_add_i16(tmp4, tp765);
        let in3 = sat_sub_i16(tm765, tmp3);
        let in5 = sat_add_i16(tm465, tmp5);
        let in7 = sat_sub_i16(tmp6, tp465);

        // store column results into rows (still column-major staging)
        cols[0][x] = in0;
        cols[1][x] = in1;
        cols[2][x] = in2;
        cols[3][x] = in3;
        cols[4][x] = in4;
        cols[5][x] = in5;
        cols[6][x] = in6;
        cols[7][x] = in7;
        let _ = (tm65, tp65);
    }
    cols
}

/// FDCT + quantize + zigzag. Writes 64 coefficients into `out` in zigzag order.
pub fn fdct_quant_zig(
    src: &[u8],
    stride: usize,
    encode_matrix: &[u16],
    add_val: i16,
    out: &mut [i16; 64],
) {
    let mut rows = [[0i16; 8]; 8];
    for y in 0..8 {
        for x in 0..8 {
            let p = src[y * stride + x] as i16;
            rows[y][x] = sat_add_i16(p, add_val);
        }
    }

    let cols = fdct_column_stage(rows);
    let ftabs = [&FTAB1_128, &FTAB2_128, &FTAB3_128, &FTAB4_128];
    // Row order uses ftab1,2,3,4,1,4,3,2 as in the C source.
    let ftab_order = [0usize, 1, 2, 3, 0, 3, 2, 1];
    let mut spatial = [0i16; 64];
    for (y, &fi) in ftab_order.iter().enumerate() {
        let row_in = cols[y];
        let row_out = fdct_row(row_in, ftabs[fi]);
        for x in 0..8 {
            spatial[y * 8 + x] = row_out[x];
        }
    }

    // Quantize in frequency/spatial order then emit in zigzag order for entropy coding.
    // Encode matrix layout: [0..64)=correction, [64..128)=reciprocal, [128..192)=scale
    for i in 0..64 {
        let zig_pos = ZIGZAG_INV[i] as usize;
        out[zig_pos] = spatial_quant(spatial[i], encode_matrix, i);
    }
}

fn spatial_quant(v: i16, encode_matrix: &[u16], i: usize) -> i16 {
    // Match libvmx `_mm_sign_epi16(quantized, original)`: a zero source lane
    // forces a zero coefficient even if correction/reciprocal would be nonzero.
    if v == 0 {
        return 0;
    }
    let abs_v = v.unsigned_abs();
    let c = encode_matrix[i];
    let recip = encode_matrix[i + 64];
    let scale = encode_matrix[i + 128];
    let mut q = abs_v.wrapping_add(c);
    q = mulhi_u16(q, recip);
    q = mulhi_u16(q, scale);
    if v < 0 { -(q as i16) } else { q as i16 }
}

/// One row of the IPP-style inverse DCT (port of the `madd`/`packs` block used
/// four times inside `VMX_ZIG_INVQUANTIZE_IDCT_8X8_128`).
fn idct_row(x: [i16; 8], tab: &[i16; 32]) -> [i16; 8] {
    #[inline]
    fn madd(a: i16, ta: i16, b: i16, tb: i16) -> i32 {
        (a as i32 * ta as i32).wrapping_add(b as i32 * tb as i32)
    }

    let mut out = [0i16; 8];
    for i in 0..4 {
        // Even part: coefficients 0/2 against tab[0..8], 4/6 against tab[8..16].
        let even = madd(x[0], tab[2 * i], x[2], tab[2 * i + 1])
            .wrapping_add(IRND_INV_ROW)
            .wrapping_add(madd(x[4], tab[8 + 2 * i], x[6], tab[8 + 2 * i + 1]));
        // Odd part: coefficients 1/3 against tab[16..24], 5/7 against tab[24..32].
        let odd = madd(x[5], tab[24 + 2 * i], x[7], tab[24 + 2 * i + 1]).wrapping_add(madd(
            x[1],
            tab[16 + 2 * i],
            x[3],
            tab[16 + 2 * i + 1],
        ));

        out[i] = sat_i32_to_i16(even.wrapping_add(odd) >> SHIFT_INV_ROW);
        out[7 - i] = sat_i32_to_i16(even.wrapping_sub(odd) >> SHIFT_INV_ROW);
    }
    out
}

#[inline]
fn sat_i32_to_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// Column pass of the inverse DCT for a single column, returning the eight
/// output samples already offset by `add_val` (still 16-bit, unclamped).
fn idct_column(col: [i16; 8], add_val: i16) -> [i16; 8] {
    let (r0, r1, r2, r3, r4, r5, r6, r7) = (
        col[0], col[1], col[2], col[3], col[4], col[5], col[6], col[7],
    );

    // Odd part
    let mut x0 = sat_add_i16(mulhi_i16(r5, IDCT_TG3), r5);
    let x1 = sat_add_i16(mulhi_i16(r3, IDCT_TG3), r3);
    x0 = sat_add_i16(x0, r3);
    let x2 = sat_sub_i16(r5, x1);
    let x5 = sat_sub_i16(mulhi_i16(r1, IDCT_TG1), r7);
    let x4 = sat_add_i16(mulhi_i16(r7, IDCT_TG1), r1);

    let temp7 = sat_add_i16(sat_add_i16(x0, x4), 1);
    let t4 = sat_sub_i16(x4, x0);
    let t5 = sat_add_i16(sat_sub_i16(x5, x2), 1);
    let temp3 = sat_add_i16(x5, x2);

    let s = sat_add_i16(t4, t5);
    let d = sat_sub_i16(t4, t5);
    let m4 = sat_add_i16(s, mulhi_i16(IDCT_COS4, s)) | 1;
    let m0 = sat_add_i16(mulhi_i16(IDCT_COS4, d), d) | 1;

    // Even part
    let e7 = sat_add_i16(mulhi_i16(r6, IDCT_TG2), r2);
    let e3 = sat_sub_i16(mulhi_i16(r2, IDCT_TG2), r6);
    let sum04 = sat_add_i16(r4, r0);
    let dif04 = sat_sub_i16(r0, r4);

    let rnd_col = IRND_INV_COL as i16;
    let rnd_corr = IRND_INV_CORR as i16;
    let b0 = sat_add_i16(sat_add_i16(sum04, e7), rnd_col);
    let b3 = sat_add_i16(sat_sub_i16(sum04, e7), rnd_corr);
    let b1 = sat_add_i16(sat_add_i16(dif04, e3), rnd_col);
    let b2 = sat_add_i16(sat_sub_i16(dif04, e3), rnd_corr);

    let fin = |v: i16| sat_add_i16(v >> SHIFT_INV_COL, add_val);
    [
        fin(sat_add_i16(temp7, b0)),
        fin(sat_add_i16(b1, m4)),
        fin(sat_add_i16(b2, m0)),
        fin(sat_add_i16(temp3, b3)),
        fin(sat_sub_i16(b3, temp3)),
        fin(sat_sub_i16(b2, m0)),
        fin(sat_sub_i16(b1, m4)),
        fin(sat_sub_i16(b0, temp7)),
    ]
}

/// Inverse: dezigzag, dequant, IDCT, write 8×8 block to dst.
///
/// Bit-compatible port of `VMX_ZIG_INVQUANTIZE_IDCT_8X8_128`. `coeffs` holds the
/// entropy-decoded coefficients in zigzag scan order; `decode_matrix` is indexed
/// in raster (spatial) order.
pub fn zig_invquant_idct(
    coeffs: &mut [i16; 64],
    decode_matrix: &[u16],
    dst: &mut [u8],
    stride: usize,
    add_val: i16,
) {
    // Inverse zigzag + dequant: spatial[i] = (coeffs[ZIGZAG_INV[i]] * matrix[i]) >> 4
    let mut rows = [[0i16; 8]; 8];
    for i in 0..64 {
        let c = coeffs[ZIGZAG_INV[i] as usize];
        rows[i / 8][i % 8] = c.wrapping_mul(decode_matrix[i] as i16) >> 4;
    }

    for (y, row) in rows.iter_mut().enumerate() {
        *row = idct_row(*row, IDCT_ROW_TABLES[y]);
    }

    for x in 0..8 {
        let col = [
            rows[0][x], rows[1][x], rows[2][x], rows[3][x], rows[4][x], rows[5][x], rows[6][x],
            rows[7][x],
        ];
        let out = idct_column(col, add_val);
        for (y, &v) in out.iter().enumerate() {
            dst[y * stride + x] = v.clamp(0, 255) as u8;
        }
    }
}

/// Broadcast DC-only 8×8 block (preview / empty AC).
///
/// Port of `VMX_BROADCAST_DC_8X8_128`: `(dc + 4) >> 3 + addVal`, saturated to u8.
pub fn broadcast_dc(dc: i16, dst: &mut [u8], stride: usize, add_val: i16) {
    let v = (dc.wrapping_add(4) >> 3).wrapping_add(add_val);
    let pix = v.clamp(0, 255) as u8;
    for y in 0..8 {
        dst[y * stride..y * stride + 8].fill(pix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::ZIGZAG;

    #[test]
    fn zigzag_roundtrip_index() {
        for i in 0..64u8 {
            assert_eq!(ZIGZAG_INV[ZIGZAG[i as usize] as usize], i);
        }
    }

    /// Floating-point 2D IDCT that the libvmx integer transform approximates.
    fn reference_idct(spatial: &[i16; 64]) -> [f64; 64] {
        use std::f64::consts::PI;
        let mut out = [0f64; 64];
        for (y, row) in out.chunks_exact_mut(8).enumerate() {
            for (x, o) in row.iter_mut().enumerate() {
                let mut acc = 0f64;
                for v in 0..8usize {
                    for u in 0..8usize {
                        let cu = if u == 0 {
                            std::f64::consts::FRAC_1_SQRT_2
                        } else {
                            1.0
                        };
                        let cv = if v == 0 {
                            std::f64::consts::FRAC_1_SQRT_2
                        } else {
                            1.0
                        };
                        acc += cu
                            * cv
                            * f64::from(spatial[v * 8 + u])
                            * (((2 * x + 1) as f64) * u as f64 * PI / 16.0).cos()
                            * (((2 * y + 1) as f64) * v as f64 * PI / 16.0).cos();
                    }
                }
                *o = acc / 4.0;
            }
        }
        out
    }

    /// With an all-16 quant matrix the dequant step is the identity, so the
    /// integer IDCT output must track the float reference within ±2.
    #[test]
    fn idct_matches_reference() {
        let matrix = [16u16; 64];
        let cases: [&[(usize, i16)]; 4] = [
            &[(0, 400)],
            &[(0, 400), (1, 120), (8, -90)],
            &[(0, 300), (2, -60), (17, 45), (63, 25)],
            &[(0, 520), (3, -200), (12, 80), (30, -40), (55, 15)],
        ];

        for case in cases {
            let mut spatial = [0i16; 64];
            for &(idx, val) in case {
                spatial[idx] = val;
            }
            // Feed the same block through the public API in zigzag order.
            let mut coeffs = [0i16; 64];
            for i in 0..64 {
                coeffs[ZIGZAG_INV[i] as usize] = spatial[i];
            }

            let mut dst = [0u8; 64];
            zig_invquant_idct(&mut coeffs, &matrix, &mut dst, 8, 128);

            let reference = reference_idct(&spatial);
            for i in 0..64 {
                let expected = (reference[i] + 128.0).round().clamp(0.0, 255.0);
                let diff = (f64::from(dst[i]) - expected).abs();
                assert!(
                    diff <= 2.0,
                    "idx {i}: got {}, expected ~{expected}, diff {diff}",
                    dst[i]
                );
            }
        }
    }

    #[test]
    fn broadcast_dc_matches_reference() {
        for dc in [-2048i16, -100, -1, 0, 1, 400, 1023, 2047] {
            let mut dst = [0u8; 64];
            broadcast_dc(dc, &mut dst, 8, 128);
            let expected = (((dc + 4) >> 3) + 128).clamp(0, 255) as u8;
            assert!(dst.iter().all(|&p| p == expected), "dc {dc}");
        }
    }

    /// A DC-only block must decode to (nearly) the same flat value through both
    /// the full IDCT and the DC broadcast shortcut.
    #[test]
    fn dc_only_block_agrees_with_broadcast() {
        let matrix = [16u16; 64];
        for dc in [-512i16, -64, 0, 64, 400, 900] {
            let mut coeffs = [0i16; 64];
            coeffs[0] = dc;
            let mut full = [0u8; 64];
            zig_invquant_idct(&mut coeffs, &matrix, &mut full, 8, 128);

            let mut flat = [0u8; 64];
            broadcast_dc(dc, &mut flat, 8, 128);

            for i in 0..64 {
                let diff = i32::from(full[i]) - i32::from(flat[i]);
                assert!(
                    diff.abs() <= 1,
                    "dc {dc} idx {i}: {} vs {}",
                    full[i],
                    flat[i]
                );
            }
        }
    }
}
