//! Portable SIMD (`std::simd`) plane encode/decode kernels.
//!
//! Enabled with `--features portable-simd` on Rust nightly. Vectorizes the
//! scalar AAN FDCT/IDCT with `Simd<i16, 8>` and sits below arch-specific
//! AVX2/SSE/NEON paths in dispatch priority.

#![cfg(feature = "portable-simd")]

use crate::bitstream::{SliceData, get_2mag_sign, get_int_from_2mag_sign};
use crate::codec::dct::broadcast_dc;
use crate::codec::plane::PlaneView;
use crate::tables::{
    FDCT_ROUND1, FDCT_SQRT2, FDCT_TAN1, FDCT_TAN2, FDCT_TAN3, FTAB1_128, FTAB2_128, FTAB3_128,
    FTAB4_128, IDCT_COS4, IDCT_ROW_TABLES, IDCT_TG1, IDCT_TG2, IDCT_TG3, IRND_INV_COL,
    IRND_INV_CORR, IRND_INV_ROW, RND_FRW_ROW, SHIFT_FRW_COL, SHIFT_FRW_ROW, SHIFT_INV_COL,
    SHIFT_INV_ROW, ZIGZAG_INV,
};
use crate::types::SLICE_HEIGHT;
use std::simd::Simd;
use std::simd::prelude::*;

type I16x8 = Simd<i16, 8>;
type I32x4 = Simd<i32, 4>;
type I32x8 = Simd<i32, 8>;
type U16x8 = Simd<u16, 8>;

#[inline(always)]
fn mulhi_i16(a: I16x8, b: I16x8) -> I16x8 {
    let prod = a.cast::<i32>() * b.cast::<i32>();
    (prod >> I32x8::splat(16)).cast::<i16>()
}

#[inline(always)]
fn mulhi_u16_lane(a: u16, b: u16) -> u16 {
    (((a as u32) * (b as u32)) >> 16) as u16
}

#[inline(always)]
fn load_i16x8(src: &[i16]) -> I16x8 {
    I16x8::from_slice(src)
}

#[inline(always)]
fn packs_i32_pair(lo: I32x4, hi: I32x4) -> I16x8 {
    let lo_c = lo.simd_clamp(Simd::splat(i16::MIN as i32), Simd::splat(i16::MAX as i32));
    let hi_c = hi.simd_clamp(Simd::splat(i16::MIN as i32), Simd::splat(i16::MAX as i32));
    let lo_a = lo_c.to_array();
    let hi_a = hi_c.to_array();
    I16x8::from_array([
        lo_a[0] as i16,
        lo_a[1] as i16,
        lo_a[2] as i16,
        lo_a[3] as i16,
        hi_a[0] as i16,
        hi_a[1] as i16,
        hi_a[2] as i16,
        hi_a[3] as i16,
    ])
}

/// Pairwise multiply-add of adjacent i16 lanes → four i32 results (SSE `madd_epi16`).
#[inline(always)]
fn madd_epi16(a: I16x8, b: I16x8) -> I32x4 {
    let prod = a.cast::<i32>() * b.cast::<i32>();
    let arr = prod.to_array();
    I32x4::from_array([
        arr[0].wrapping_add(arr[1]),
        arr[2].wrapping_add(arr[3]),
        arr[4].wrapping_add(arr[5]),
        arr[6].wrapping_add(arr[7]),
    ])
}

/// IDCT row — bit-compatible with scalar / SSE `idct_row`.
#[inline]
pub fn idct_row_portable(x: [i16; 8], tab: &[i16; 32]) -> [i16; 8] {
    let p0 = I16x8::from_array([x[0], x[2], x[0], x[2], x[0], x[2], x[0], x[2]]);
    let p1 = I16x8::from_array([x[1], x[3], x[1], x[3], x[1], x[3], x[1], x[3]]);
    let p2 = I16x8::from_array([x[4], x[6], x[4], x[6], x[4], x[6], x[4], x[6]]);
    let p3 = I16x8::from_array([x[5], x[7], x[5], x[7], x[5], x[7], x[5], x[7]]);

    let even = madd_epi16(p0, load_i16x8(&tab[0..8]))
        + madd_epi16(p2, load_i16x8(&tab[8..16]))
        + I32x4::splat(IRND_INV_ROW);
    let odd = madd_epi16(p1, load_i16x8(&tab[16..24])) + madd_epi16(p3, load_i16x8(&tab[24..32]));

    let shift = I32x4::splat(SHIFT_INV_ROW);
    let sum = (even + odd) >> shift;
    let diff = (even - odd) >> shift;
    let lo = packs_i32_pair(sum, sum).to_array();
    let hi = packs_i32_pair(diff, diff).to_array();
    [lo[0], lo[1], lo[2], lo[3], hi[3], hi[2], hi[1], hi[0]]
}

/// Eight-column IDCT in parallel (one `i16` lane per column).
#[inline]
pub fn idct_columns_8_portable(rows: [I16x8; 8], add_val: i16) -> [I16x8; 8] {
    let (r0, r1, r2, r3, r4, r5, r6, r7) = (
        rows[0], rows[1], rows[2], rows[3], rows[4], rows[5], rows[6], rows[7],
    );

    let tg1 = I16x8::splat(IDCT_TG1);
    let tg2 = I16x8::splat(IDCT_TG2);
    let tg3 = I16x8::splat(IDCT_TG3);
    let cos4 = I16x8::splat(IDCT_COS4);
    let one = I16x8::splat(1);

    let mut x0 = mulhi_i16(r5, tg3).saturating_add(r5);
    let x1 = mulhi_i16(r3, tg3).saturating_add(r3);
    x0 = x0.saturating_add(r3);
    let x2 = r5.saturating_sub(x1);
    let x5 = mulhi_i16(r1, tg1).saturating_sub(r7);
    let x4 = mulhi_i16(r7, tg1).saturating_add(r1);

    let temp7 = x0.saturating_add(x4).saturating_add(one);
    let t4 = x4.saturating_sub(x0);
    let t5 = x5.saturating_sub(x2).saturating_add(one);
    let temp3 = x5.saturating_add(x2);

    let s = t4.saturating_add(t5);
    let d = t4.saturating_sub(t5);
    let m4 = s.saturating_add(mulhi_i16(cos4, s)) | one;
    let m0 = mulhi_i16(cos4, d).saturating_add(d) | one;

    let e7 = mulhi_i16(r6, tg2).saturating_add(r2);
    let e3 = mulhi_i16(r2, tg2).saturating_sub(r6);
    let sum04 = r4.saturating_add(r0);
    let dif04 = r0.saturating_sub(r4);

    let rnd_col = I16x8::splat(IRND_INV_COL as i16);
    let rnd_corr = I16x8::splat(IRND_INV_CORR as i16);
    let b0 = sum04.saturating_add(e7).saturating_add(rnd_col);
    let b3 = sum04.saturating_sub(e7).saturating_add(rnd_corr);
    let b1 = dif04.saturating_add(e3).saturating_add(rnd_col);
    let b2 = dif04.saturating_sub(e3).saturating_add(rnd_corr);

    let add = I16x8::splat(add_val);
    let shift = I16x8::splat(SHIFT_INV_COL as i16);
    let fin = |v: I16x8| (v >> shift).saturating_add(add);
    [
        fin(temp7.saturating_add(b0)),
        fin(b1.saturating_add(m4)),
        fin(b2.saturating_add(m0)),
        fin(temp3.saturating_add(b3)),
        fin(b3.saturating_sub(temp3)),
        fin(b2.saturating_sub(m0)),
        fin(b1.saturating_sub(m4)),
        fin(b0.saturating_sub(temp7)),
    ]
}

/// Inverse zigzag + dequant + IDCT (portable). Matches scalar `zig_invquant_idct`.
pub fn zig_invquant_idct_portable(
    coeffs: &mut [i16; 64],
    decode_matrix: &[u16],
    dst: &mut [u8],
    stride: usize,
    add_val: i16,
) {
    let mut rows = [I16x8::splat(0); 8];
    for row in 0..8 {
        let base = row * 8;
        let mut spatial = [0i16; 8];
        for (i, sample) in spatial.iter_mut().enumerate() {
            let idx = base + i;
            let c = coeffs[ZIGZAG_INV[idx] as usize];
            *sample = c.wrapping_mul(decode_matrix[idx] as i16) >> 4;
        }
        rows[row] = I16x8::from_array(idct_row_portable(spatial, IDCT_ROW_TABLES[row]));
    }

    let out_rows = idct_columns_8_portable(rows, add_val);
    for (y, row) in out_rows.iter().enumerate() {
        let clamped = row.simd_clamp(I16x8::splat(0), I16x8::splat(255));
        let arr = clamped.to_array();
        let bytes = [
            arr[0] as u8,
            arr[1] as u8,
            arr[2] as u8,
            arr[3] as u8,
            arr[4] as u8,
            arr[5] as u8,
            arr[6] as u8,
            arr[7] as u8,
        ];
        dst[y * stride..y * stride + 8].copy_from_slice(&bytes);
    }
}

/// FDCT row using ftab coefficients (portable port of SSE madd path).
#[inline]
fn fdct_row_portable(input: I16x8, ftab: &[i16; 32]) -> I16x8 {
    let a = input.to_array();
    let rev = I16x8::from_array([a[7], a[6], a[5], a[4], a[3], a[2], a[1], a[0]]);
    let sums = input.saturating_add(rev);
    let diffs = input.saturating_sub(rev);
    let s = sums.to_array();
    let d = diffs.to_array();
    let full = I16x8::from_array([s[0], s[1], d[0], d[1], s[2], s[3], d[2], d[3]]);
    let shuf = I16x8::from_array([s[2], s[3], d[2], d[3], s[0], s[1], d[0], d[1]]);

    let temp4 = madd_epi16(full, load_i16x8(&ftab[0..8]));
    let temp1 = madd_epi16(shuf, load_i16x8(&ftab[8..16]));
    let temp2 = madd_epi16(full, load_i16x8(&ftab[16..24]));
    let temp3 = madd_epi16(shuf, load_i16x8(&ftab[24..32]));

    let round = I32x4::splat(RND_FRW_ROW);
    let shift = I32x4::splat(SHIFT_FRW_ROW);
    let lo = (temp4 + temp1 + round) >> shift;
    let hi = (temp3 + temp2 + round) >> shift;
    packs_i32_pair(lo, hi)
}

/// FDCT column stage + row transforms matching SSE `fdct_quant_zig_sse` layout.
fn fdct_transform_8x8(rows: [I16x8; 8]) -> [I16x8; 8] {
    let (mut xmm0, mut xmm2, mut xmm7, mut xmm5) = (rows[0], rows[2], rows[7], rows[5]);
    let xmm3_copy = xmm0;
    let xmm4_copy = xmm2;
    xmm0 = xmm0.saturating_sub(xmm7);
    xmm7 = xmm7.saturating_add(xmm3_copy);
    xmm2 = xmm2.saturating_sub(xmm5);
    xmm5 = xmm5.saturating_add(xmm4_copy);

    let (mut xmm3, mut xmm4) = (rows[3], rows[4]);
    let xmm1_copy = xmm3;
    xmm3 = xmm3.saturating_sub(xmm4);
    xmm4 = xmm4.saturating_add(xmm1_copy);
    let (mut xmm6, mut xmm1) = (rows[6], rows[1]);
    let tmp = xmm1;
    xmm1 = xmm1.saturating_sub(xmm6);
    xmm6 = xmm6.saturating_add(tmp);

    let mut tm03 = xmm7.saturating_sub(xmm4);
    let mut tm12 = xmm6.saturating_sub(xmm5);
    xmm4 = xmm4.saturating_add(xmm4);
    xmm5 = xmm5.saturating_add(xmm5);
    let mut tp03 = xmm4.saturating_add(tm03);
    let mut tp12 = xmm5.saturating_add(tm12);

    let shift1 = I16x8::splat((SHIFT_FRW_COL + 1) as i16);
    let shift0 = I16x8::splat(SHIFT_FRW_COL as i16);
    xmm2 <<= shift1;
    xmm1 <<= shift1;
    tp03 <<= shift0;
    tp12 <<= shift0;
    tm03 <<= shift0;
    tm12 <<= shift0;
    xmm3 <<= shift0;
    xmm0 <<= shift0;

    let mut in4 = tp03.saturating_sub(tp12);
    let diff = xmm1.saturating_sub(xmm2);
    tp12 = tp12.saturating_add(tp12);
    xmm2 = xmm2.saturating_add(xmm2);
    let mut in0 = tp12.saturating_add(in4);
    let sum = xmm2.saturating_add(diff);

    let tan2 = I16x8::splat(FDCT_TAN2);
    let mut in6 = mulhi_i16(tan2, tm03).saturating_sub(tm12);
    let mut in2 = mulhi_i16(tan2, tm12).saturating_add(tm03);
    let sqrt2 = I16x8::splat(FDCT_SQRT2);
    let rounder = I16x8::splat(FDCT_ROUND1);
    let tp65 = mulhi_i16(sum, sqrt2) | rounder;
    in2 |= rounder;
    in6 |= rounder;
    let tm65 = mulhi_i16(diff, sqrt2);

    let tm465 = xmm3.saturating_sub(tm65);
    let tm765 = xmm0.saturating_sub(tp65);
    let tp765 = tp65.saturating_add(xmm0);
    let tp465 = tm65.saturating_add(xmm3);
    let tan3 = I16x8::splat(FDCT_TAN3);
    let tan1 = I16x8::splat(FDCT_TAN1);
    let tmp3 = mulhi_i16(tm465, tan3).saturating_add(tm465);
    let tmp5 = mulhi_i16(tm765, tan3).saturating_add(tm765);
    let mut in1 = mulhi_i16(tp465, tan1).saturating_add(tp765);
    let mut in3 = tm765.saturating_sub(tmp3);
    let mut in5 = tm465.saturating_add(tmp5);
    let mut in7 = mulhi_i16(tp765, tan1).saturating_sub(tp465);

    in0 = fdct_row_portable(in0, &FTAB1_128);
    in1 = fdct_row_portable(in1, &FTAB2_128);
    in2 = fdct_row_portable(in2, &FTAB3_128);
    in3 = fdct_row_portable(in3, &FTAB4_128);
    in4 = fdct_row_portable(in4, &FTAB1_128);
    in5 = fdct_row_portable(in5, &FTAB4_128);
    in6 = fdct_row_portable(in6, &FTAB3_128);
    in7 = fdct_row_portable(in7, &FTAB2_128);

    [in0, in1, in2, in3, in4, in5, in6, in7]
}

#[inline]
fn spatial_quant_scalar(v: i16, encode_matrix: &[u16], i: usize) -> i16 {
    if v == 0 {
        return 0;
    }
    let abs_v = v.unsigned_abs();
    let c = encode_matrix[i];
    let recip = encode_matrix[i + 64];
    let scale = encode_matrix[i + 128];
    let mut q = abs_v.wrapping_add(c);
    q = mulhi_u16_lane(q, recip);
    q = mulhi_u16_lane(q, scale);
    if v < 0 { -(q as i16) } else { q as i16 }
}

/// FDCT + quantize + zigzag. Writes 64 coefficients in zigzag order.
pub fn fdct_quant_zig_portable(
    src: &[u8],
    stride: usize,
    encode_matrix: &[u16],
    add_val: i16,
    out: &mut [i16; 64],
) {
    let add = I16x8::splat(add_val);
    let mut rows = [I16x8::splat(0); 8];
    for y in 0..8 {
        let mut pix = [0i16; 8];
        for x in 0..8 {
            pix[x] = src[y * stride + x] as i16;
        }
        rows[y] = I16x8::from_array(pix).saturating_add(add);
    }

    let transformed = fdct_transform_8x8(rows);
    let mut spatial = [0i16; 64];
    for (y, row) in transformed.iter().enumerate() {
        spatial[y * 8..y * 8 + 8].copy_from_slice(&row.to_array());
    }

    for i in 0..64 {
        let zig_pos = ZIGZAG_INV[i] as usize;
        out[zig_pos] = spatial_quant_scalar(spatial[i], encode_matrix, i);
    }
}

fn ac_nonzero_mask(block: &[i16; 64]) -> u64 {
    let mut m = 0u64;
    for (i, coeff) in block.iter().enumerate().skip(1) {
        if *coeff != 0 {
            m |= 1u64 << i;
        }
    }
    m
}

/// Encode one plane band using portable SIMD DCT kernels.
pub fn encode_plane(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    if encode_matrix.len() < 192 {
        return crate::codec::plane::encode_plane_scalar(
            plane,
            dc,
            ac,
            encode_matrix,
            dc_shift,
            temp_block,
        );
    }

    let mut dc_pred = 0i16;
    let mut num_zeros = 0u32;
    let add_val = if plane.index == 0 || plane.index == 3 {
        -128
    } else {
        0
    };
    let dc_round = if dc_shift > 0 {
        1i16 << (dc_shift - 1)
    } else {
        0
    };
    let height = SLICE_HEIGHT as usize;

    for y in (0..height).step_by(8) {
        for x in (0..plane.stride).step_by(8) {
            let src_off = plane.offset + y * plane.stride + x;
            if src_off + 7 * plane.stride + 8 > plane.data.len() {
                continue;
            }
            fdct_quant_zig_portable(
                &plane.data[src_off..],
                plane.stride,
                encode_matrix,
                add_val,
                temp_block,
            );
            let dc_val = temp_block[0].wrapping_add(dc_round) >> dc_shift;
            let m_index = ac_nonzero_mask(temp_block);
            dc.encode_dc(dc_val.wrapping_sub(dc_pred));
            dc.emit_bits32();
            dc_pred = dc_val;
            if m_index == 0 {
                num_zeros += 64;
                continue;
            }

            let mut coded = [0u32; 64];
            for i in 0..64 {
                coded[i] = (get_2mag_sign(temp_block[i]) as u32).wrapping_add(1);
            }
            let mut m = m_index;
            let nz = m.trailing_zeros() as usize;
            num_zeros += nz as u32;
            ac.encode_zeros(&mut num_zeros);
            ac.emit_bits32();
            ac.encode_value(coded[nz]);
            let mut pos = nz + 1;
            m >>= nz + 1;
            ac.emit_bits32();
            while m != 0 {
                let zeros = m.trailing_zeros() as usize;
                ac.encode_zeros_small(zeros as u64);
                ac.encode_value(coded[pos + zeros]);
                pos += zeros + 1;
                m >>= zeros + 1;
                ac.emit_bits32();
            }
            num_zeros = (64 - pos) as u32;
        }
    }
    ac.encode_zeros(&mut num_zeros);
    ac.emit_bits32();
    ac.flush_remaining_bits();
    dc.flush_remaining_bits();
}

/// Decode one plane band using portable SIMD IDCT kernels.
pub fn decode_plane(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    let height = SLICE_HEIGHT as usize;
    let add_val: i16 = if plane.index == 0 || plane.index == 3 {
        128
    } else {
        0
    };
    let mut dc_pred: i16 = 0;
    let mut terms_to_decode: u64 = 0;
    let stride = plane.stride;
    let base = plane.offset;

    for y in (0..height).step_by(8) {
        for x in (0..stride).step_by(8) {
            temp_block.fill(0);
            let valid = terms_to_decode < 64;

            while terms_to_decode < 64 {
                let l = ac.peek_golomb_lookup();
                if l.length != 0 {
                    ac.bits_left -= l.length as i32;
                    temp_block[terms_to_decode as usize] = l.value as i16;
                    terms_to_decode += l.zeros as u64;
                } else {
                    let b = ac.get_bit_b();
                    if b != 0 {
                        let b2 = ac.get_bit_b();
                        if b2 != 0 {
                            terms_to_decode += 1;
                        } else {
                            let mut bc = ac.get_zeros_b();
                            bc += 2;
                            let val = ac.get_bits_b(bc as u32);
                            terms_to_decode += val;
                        }
                    } else {
                        let mut bc = ac.get_zeros_b();
                        bc += 2;
                        let val = ac.get_bits_b(bc as u32);
                        temp_block[terms_to_decode as usize] =
                            get_int_from_2mag_sign(val.wrapping_sub(1));
                        terms_to_decode += 1;
                    }
                }
                ac.reload_bits();
            }
            terms_to_decode -= 64;

            let b = dc.get_bit();
            if b != 0 {
                let _b2 = dc.get_bit();
            } else {
                let mut bc = dc.get_zeros();
                bc += 2;
                let val = dc.get_bits(bc as u32);
                temp_block[0] = get_int_from_2mag_sign(val.wrapping_sub(1));
                temp_block[0] <<= dc_shift;
            }
            temp_block[0] = temp_block[0].wrapping_add(dc_pred);
            dc_pred = temp_block[0];

            let dst_off = base + y * stride + x;
            if dst_off + 7 * stride + 8 > plane.data.len() {
                continue;
            }
            if valid {
                let ac_empty = temp_block[1..].iter().all(|&c| c == 0);
                if ac_empty {
                    broadcast_dc(temp_block[0], &mut plane.data[dst_off..], stride, add_val);
                } else {
                    zig_invquant_idct_portable(
                        temp_block,
                        decode_matrix,
                        &mut plane.data[dst_off..],
                        stride,
                        add_val,
                    );
                }
            } else {
                broadcast_dc(temp_block[0], &mut plane.data[dst_off..], stride, add_val);
            }
        }
    }
}

// Silence unused type alias when only IDCT is exercised in some cfgs.
#[allow(dead_code)]
type _KeepU16 = U16x8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::dct::{fdct_quant_zig, idct_row, zig_invquant_idct};

    #[test]
    fn idct_row_portable_matches_scalar() {
        for seed in 0..64i16 {
            let mut x = [0i16; 8];
            for (i, lane) in x.iter_mut().enumerate() {
                *lane = ((seed * 3 + i as i16 * 7) % 40) - 20;
            }
            for tab in &IDCT_ROW_TABLES {
                assert_eq!(idct_row_portable(x, tab), idct_row(x, tab), "seed={seed}");
            }
        }
    }

    #[test]
    fn zig_invquant_idct_portable_matches_scalar() {
        for seed in 0..32u32 {
            let mut coeffs = [0i16; 64];
            for (i, c) in coeffs.iter_mut().enumerate() {
                *c = (((i as u32).wrapping_mul(seed.wrapping_add(3))) % 17) as i16 - 8;
            }
            let mut matrix = [0u16; 64];
            for (i, m) in matrix.iter_mut().enumerate() {
                *m = ((i as u16) % 11) + 1;
            }
            for add_val in [0i16, 128] {
                let mut dst_s = [0u8; 8 * 16];
                let mut dst_p = [0u8; 8 * 16];
                let mut c_s = coeffs;
                let mut c_p = coeffs;
                zig_invquant_idct(&mut c_s, &matrix, &mut dst_s, 16, add_val);
                zig_invquant_idct_portable(&mut c_p, &matrix, &mut dst_p, 16, add_val);
                assert_eq!(dst_p, dst_s, "seed={seed} add={add_val}");
            }
        }
    }

    #[test]
    fn fdct_quant_zig_portable_matches_scalar() {
        let stride = 16;
        let mut src = vec![0u8; stride * 8];
        for (i, b) in src.iter_mut().enumerate() {
            *b = ((i * 13 + 7) % 220) as u8 + 16;
        }
        let mut matrix = vec![0u16; 192];
        for i in 0..64 {
            matrix[i] = (i as u16 % 5) + 1;
            matrix[i + 64] = 0x8000 + (i as u16 * 17);
            matrix[i + 128] = 0x4000 + (i as u16 * 3);
        }
        for add_val in [-128i16, 0] {
            let mut out_s = [0i16; 64];
            let mut out_p = [0i16; 64];
            fdct_quant_zig(&src, stride, &matrix, add_val, &mut out_s);
            fdct_quant_zig_portable(&src, stride, &matrix, add_val, &mut out_p);
            assert_eq!(out_p, out_s, "add_val={add_val}");
        }
    }

    #[test]
    fn idct_portable_faster_than_scalar_in_release() {
        if cfg!(debug_assertions) {
            return;
        }
        use std::time::Instant;

        let mut coeffs = [0i16; 64];
        for (i, c) in coeffs.iter_mut().enumerate() {
            *c = ((i as i16) % 11) - 5;
        }
        let mut matrix = [0u16; 64];
        for (i, m) in matrix.iter_mut().enumerate() {
            *m = ((i as u16) % 13) + 1;
        }
        let mut dst = [0u8; 8 * 16];
        let warmup = 80;
        let iters = 4000;

        for _ in 0..warmup {
            let mut c = coeffs;
            zig_invquant_idct(&mut c, &matrix, &mut dst, 16, 128);
            let mut c = coeffs;
            zig_invquant_idct_portable(&mut c, &matrix, &mut dst, 16, 128);
        }

        let t0 = Instant::now();
        for _ in 0..iters {
            let mut c = coeffs;
            zig_invquant_idct(&mut c, &matrix, &mut dst, 16, 128);
        }
        let scalar = t0.elapsed();

        let t0 = Instant::now();
        for _ in 0..iters {
            let mut c = coeffs;
            zig_invquant_idct_portable(&mut c, &matrix, &mut dst, 16, 128);
        }
        let portable = t0.elapsed();

        eprintln!(
            "idct 8x8 scalar={:.3}us portable={:.3}us ({:.2}x)",
            scalar.as_secs_f64() * 1e6 / iters as f64,
            portable.as_secs_f64() * 1e6 / iters as f64,
            scalar.as_secs_f64() / portable.as_secs_f64().max(1e-12)
        );
        assert!(
            portable < scalar && portable * 5 < scalar * 4,
            "portable IDCT should be faster than scalar (scalar={scalar:?} portable={portable:?})"
        );
    }
}
