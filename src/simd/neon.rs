//! AArch64 NEON 8-bit plane encode/decode (native NEON, not sse2neon).
//!
//! Encode: native NEON FDCT + quant + zigzag (`fdct_quant_zig_neon`), mirroring
//! `sse128::fdct_quant_zig_sse`. Decode: NEON inverse zigzag + dequant + IDCT
//! row/column + pack.

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::PlaneView;
#[cfg(target_arch = "aarch64")]
use crate::codec::plane::encode_plane_scalar;
#[cfg(not(target_arch = "aarch64"))]
use crate::codec::plane::{decode_plane_scalar, encode_plane_scalar};

#[cfg(target_arch = "aarch64")]
use crate::bitstream::get_2mag_sign;
#[cfg(target_arch = "aarch64")]
use crate::codec::dct::broadcast_dc;
#[cfg(target_arch = "aarch64")]
use crate::tables::{
    FDCT_ROUND1, FDCT_SQRT2, FDCT_TAN1, FDCT_TAN2, FDCT_TAN3, FTAB1_128, FTAB2_128, FTAB3_128,
    FTAB4_128, IDCT_COS4, IDCT_ROW_TABLES, IDCT_TG1, IDCT_TG2, IDCT_TG3, IRND_INV_COL,
    IRND_INV_CORR, IRND_INV_ROW, RND_FRW_ROW, SHIFT_FRW_COL, SHIFT_FRW_ROW, SHIFT_INV_COL,
    SHIFT_INV_ROW,
};
#[cfg(target_arch = "aarch64")]
use crate::types::SLICE_HEIGHT;
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn madd_epi16_neon(a: int16x8_t, b: int16x8_t) -> int32x4_t {
    unsafe {
        let lo = vmull_s16(vget_low_s16(a), vget_low_s16(b));
        let hi = vmull_s16(vget_high_s16(a), vget_high_s16(b));
        let lo_sum = vpaddq_s32(lo, lo);
        let hi_sum = vpaddq_s32(hi, hi);
        vcombine_s32(vget_low_s32(lo_sum), vget_low_s32(hi_sum))
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn mulhi_epi16_neon(a: int16x8_t, b: int16x8_t) -> int16x8_t {
    unsafe {
        let lo = vmull_s16(vget_low_s16(a), vget_low_s16(b));
        let hi = vmull_s16(vget_high_s16(a), vget_high_s16(b));
        vcombine_s16(vshrn_n_s32(lo, 16), vshrn_n_s32(hi, 16))
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn mulhi_epu16_neon(a: int16x8_t, b: int16x8_t) -> int16x8_t {
    unsafe {
        let a_u = vreinterpretq_u16_s16(a);
        let b_u = vreinterpretq_u16_s16(b);
        let lo = vmull_u16(vget_low_u16(a_u), vget_low_u16(b_u));
        let hi = vmull_u16(vget_high_u16(a_u), vget_high_u16(b_u));
        vreinterpretq_s16_u16(vcombine_u16(vshrn_n_u32(lo, 16), vshrn_n_u32(hi, 16)))
    }
}

/// SSE `_mm_sign_epi16(val, signs)`: negate `val` where `signs < 0`, zero where
/// `signs == 0`, otherwise keep `val`.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn sign_epi16_neon(val: int16x8_t, signs: int16x8_t) -> int16x8_t {
    unsafe {
        let neg = vcltzq_s16(signs);
        let zero = vceqzq_s16(signs);
        let flipped = vnegq_s16(val);
        let signed = vbslq_s16(neg, flipped, val);
        vbslq_s16(zero, vdupq_n_s16(0), signed)
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn shuffle_epi32_neon(v: int16x8_t, imm: u8) -> int16x8_t {
    unsafe {
        let s32 = vreinterpretq_s32_s16(v);
        let lanes = [
            vgetq_lane_s32(s32, 0),
            vgetq_lane_s32(s32, 1),
            vgetq_lane_s32(s32, 2),
            vgetq_lane_s32(s32, 3),
        ];
        vreinterpretq_s16_s32(vld1q_s32(
            [
                lanes[(imm & 3) as usize],
                lanes[((imm >> 2) & 3) as usize],
                lanes[((imm >> 4) & 3) as usize],
                lanes[((imm >> 6) & 3) as usize],
            ]
            .as_ptr(),
        ))
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn fdct_row_neon(input: int16x8_t, ftab: &[i16; 32]) -> int16x8_t {
    unsafe {
        let high = vget_high_s16(input);
        let rev_high = vld1_s16(
            [
                vget_lane_s16(high, 3),
                vget_lane_s16(high, 2),
                vget_lane_s16(high, 1),
                vget_lane_s16(high, 0),
            ]
            .as_ptr(),
        );
        let mut reversed = vcombine_s16(vget_low_s16(input), rev_high);

        let input = shuffle_epi32_neon(input, 0x44);
        reversed = shuffle_epi32_neon(reversed, 0xee);

        let sums = vqaddq_s16(input, reversed);
        let diffs = vqsubq_s16(input, reversed);

        let sums_s32 = vreinterpretq_s32_s16(sums);
        let diffs_s32 = vreinterpretq_s32_s16(diffs);
        let full = vreinterpretq_s16_s32(vzip1q_s32(sums_s32, diffs_s32));
        let shuffled = shuffle_epi32_neon(full, 0x4e);

        let temp1 = madd_epi16_neon(shuffled, vld1q_s16(ftab.as_ptr().add(8)));
        let temp2 = madd_epi16_neon(full, vld1q_s16(ftab.as_ptr().add(16)));
        let temp3 = madd_epi16_neon(shuffled, vld1q_s16(ftab.as_ptr().add(24)));
        let temp4 = madd_epi16_neon(full, vld1q_s16(ftab.as_ptr()));

        let round = vdupq_n_s32(RND_FRW_ROW);
        let lo = vshrq_n_s32::<SHIFT_FRW_ROW>(vaddq_s32(vaddq_s32(temp4, temp1), round));
        let hi = vshrq_n_s32::<SHIFT_FRW_ROW>(vaddq_s32(vaddq_s32(temp3, temp2), round));

        vcombine_s16(vqmovn_s32(lo), vqmovn_s32(hi))
    }
}

/// Byte indices used to gather raster-ordered coefficients into zigzag order.
///
/// `A` selects bytes from rows 0..3, `B` from rows 4..7; `0xFF` marks a byte
/// taken from the other half (`vqtbl4q_u8` yields 0 for it).
#[cfg(target_arch = "aarch64")]
const ZIG_IDX_A: [[u8; 16]; 8] = [
    [0, 1, 2, 3, 16, 17, 32, 33, 18, 19, 4, 5, 6, 7, 20, 21],
    [
        34, 35, 48, 49, 255, 255, 50, 51, 36, 37, 22, 23, 8, 9, 10, 11,
    ],
    [
        24, 25, 38, 39, 52, 53, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        54, 55, 40, 41, 26, 27, 12, 13, 14, 15, 28, 29, 42, 43, 56, 57,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        58, 59, 44, 45, 30, 31, 46, 47, 60, 61, 255, 255, 255, 255, 255, 255,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 62, 63, 255, 255, 255, 255,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
];

#[cfg(target_arch = "aarch64")]
const ZIG_IDX_B: [[u8; 16]; 8] = [
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        255, 255, 255, 255, 0, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        255, 255, 255, 255, 255, 255, 2, 3, 16, 17, 32, 33, 18, 19, 4, 5,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [6, 7, 20, 21, 34, 35, 48, 49, 50, 51, 36, 37, 22, 23, 8, 9],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 11, 24, 25, 38, 39,
    ],
    [
        52, 53, 54, 55, 40, 41, 26, 27, 12, 13, 255, 255, 14, 15, 28, 29,
    ],
    [
        42, 43, 56, 57, 58, 59, 44, 45, 30, 31, 46, 47, 60, 61, 62, 63,
    ],
];

/// Permute eight raster-ordered coefficient rows into zigzag order and return
/// the non-zero mask (bit `i` set when zigzag position `i` is non-zero).
#[cfg(target_arch = "aarch64")]
#[inline]
#[target_feature(enable = "neon")]
unsafe fn zigzag_permute_neon(rows: &[int16x8_t; 8], out: *mut i16) -> u64 {
    unsafe {
        const WEIGHTS: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        let weights = vld1_u8(WEIGHTS.as_ptr());
        let mut mask = 0u64;
        let a = uint8x16x4_t(
            vreinterpretq_u8_s16(rows[0]),
            vreinterpretq_u8_s16(rows[1]),
            vreinterpretq_u8_s16(rows[2]),
            vreinterpretq_u8_s16(rows[3]),
        );
        let b = uint8x16x4_t(
            vreinterpretq_u8_s16(rows[4]),
            vreinterpretq_u8_s16(rows[5]),
            vreinterpretq_u8_s16(rows[6]),
            vreinterpretq_u8_s16(rows[7]),
        );
        for k in 0..8 {
            let idx_a = vld1q_u8(ZIG_IDX_A[k].as_ptr());
            let idx_b = vld1q_u8(ZIG_IDX_B[k].as_ptr());
            let v = vorrq_u8(vqtbl4q_u8(a, idx_a), vqtbl4q_u8(b, idx_b));
            let coeffs = vreinterpretq_s16_u8(v);
            vst1q_s16(out.add(k * 8), coeffs);
            // Non-zero lanes -> 0xFFFF; narrow to 0/1 bytes, weight them and
            // reduce so each group of eight coefficients becomes one byte.
            let nz = vreinterpretq_s16_u16(vtstq_s16(coeffs, coeffs));
            let bits = vreinterpret_u8_s8(vmovn_s16(nz));
            mask |= (vaddv_u8(vand_u8(bits, weights)) as u64) << (8 * k);
        }
        mask & !1
    }
}

/// Forward DCT, quantization and zigzag scan (NEON port of `fdct_quant_zig_sse`).
///
/// # Safety
/// Caller must enable NEON. `src` must cover 8 rows at `stride`, `matrix` holds 192
/// `u16`s, and `out` receives zigzag-ordered coefficients.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn fdct_quant_zig_neon(
    src: *const u8,
    stride: usize,
    matrix: *const u16,
    add_val: i16,
    out: &mut [i16; 64],
) -> u64 {
    unsafe {
        let mut rows = [
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(stride)))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(2 * stride)))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(3 * stride)))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(4 * stride)))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(5 * stride)))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(6 * stride)))),
            vreinterpretq_s16_u16(vmovl_u8(vld1_u8(src.add(7 * stride)))),
        ];
        let add = vdupq_n_s16(add_val);
        for row in &mut rows {
            *row = vqaddq_s16(*row, add);
        }

        let (mut xmm0, mut xmm2, mut xmm7, mut xmm5) = (rows[0], rows[2], rows[7], rows[5]);
        let xmm3_copy = xmm0;
        let xmm4_copy = xmm2;
        xmm0 = vqsubq_s16(xmm0, xmm7);
        xmm7 = vqaddq_s16(xmm7, xmm3_copy);
        xmm2 = vqsubq_s16(xmm2, xmm5);
        xmm5 = vqaddq_s16(xmm5, xmm4_copy);

        let (mut xmm3, mut xmm4) = (rows[3], rows[4]);
        let xmm1_copy = xmm3;
        xmm3 = vqsubq_s16(xmm3, xmm4);
        xmm4 = vqaddq_s16(xmm4, xmm1_copy);
        let (mut xmm6, mut xmm1) = (rows[6], rows[1]);
        let tmp = xmm1;
        xmm1 = vqsubq_s16(xmm1, xmm6);
        xmm6 = vqaddq_s16(xmm6, tmp);

        let mut tm03 = vqsubq_s16(xmm7, xmm4);
        let mut tm12 = vqsubq_s16(xmm6, xmm5);
        xmm4 = vqaddq_s16(xmm4, xmm4);
        xmm5 = vqaddq_s16(xmm5, xmm5);
        let mut tp03 = vqaddq_s16(xmm4, tm03);
        let mut tp12 = vqaddq_s16(xmm5, tm12);

        xmm2 = vshlq_n_s16(xmm2, SHIFT_FRW_COL + 1);
        xmm1 = vshlq_n_s16(xmm1, SHIFT_FRW_COL + 1);
        tp03 = vshlq_n_s16(tp03, SHIFT_FRW_COL);
        tp12 = vshlq_n_s16(tp12, SHIFT_FRW_COL);
        tm03 = vshlq_n_s16(tm03, SHIFT_FRW_COL);
        tm12 = vshlq_n_s16(tm12, SHIFT_FRW_COL);
        xmm3 = vshlq_n_s16(xmm3, SHIFT_FRW_COL);
        xmm0 = vshlq_n_s16(xmm0, SHIFT_FRW_COL);

        let mut in4 = vqsubq_s16(tp03, tp12);
        let diff = vqsubq_s16(xmm1, xmm2);
        tp12 = vqaddq_s16(tp12, tp12);
        xmm2 = vqaddq_s16(xmm2, xmm2);
        let mut in0 = vqaddq_s16(tp12, in4);
        let sum = vqaddq_s16(xmm2, diff);

        let tan2 = vdupq_n_s16(FDCT_TAN2);
        let mut in6 = vqsubq_s16(mulhi_epi16_neon(tan2, tm03), tm12);
        let mut in2 = vqaddq_s16(mulhi_epi16_neon(tan2, tm12), tm03);
        let sqrt2 = vdupq_n_s16(FDCT_SQRT2);
        let rounder = vdupq_n_s16(FDCT_ROUND1);
        let tp65 = vorrq_s16(mulhi_epi16_neon(sum, sqrt2), rounder);
        in2 = vorrq_s16(in2, rounder);
        in6 = vorrq_s16(in6, rounder);
        let tm65 = mulhi_epi16_neon(diff, sqrt2);

        let tm465 = vqsubq_s16(xmm3, tm65);
        let tm765 = vqsubq_s16(xmm0, tp65);
        let tp765 = vqaddq_s16(tp65, xmm0);
        let tp465 = vqaddq_s16(tm65, xmm3);
        let tan3 = vdupq_n_s16(FDCT_TAN3);
        let tan1 = vdupq_n_s16(FDCT_TAN1);
        let tmp3 = vqaddq_s16(mulhi_epi16_neon(tm465, tan3), tm465);
        let tmp5 = vqaddq_s16(mulhi_epi16_neon(tm765, tan3), tm765);
        let mut in1 = vqaddq_s16(mulhi_epi16_neon(tp465, tan1), tp765);
        let mut in3 = vqsubq_s16(tm765, tmp3);
        let mut in5 = vqaddq_s16(tm465, tmp5);
        let mut in7 = vqsubq_s16(mulhi_epi16_neon(tp765, tan1), tp465);

        in0 = fdct_row_neon(in0, &FTAB1_128);
        in1 = fdct_row_neon(in1, &FTAB2_128);
        in2 = fdct_row_neon(in2, &FTAB3_128);
        in3 = fdct_row_neon(in3, &FTAB4_128);
        in4 = fdct_row_neon(in4, &FTAB1_128);
        in5 = fdct_row_neon(in5, &FTAB4_128);
        in6 = fdct_row_neon(in6, &FTAB3_128);
        in7 = fdct_row_neon(in7, &FTAB2_128);

        let mut transformed = [in0, in1, in2, in3, in4, in5, in6, in7];
        for (row, &coeff) in transformed
            .iter_mut()
            .zip([0, 8, 16, 24, 32, 40, 48, 56].iter())
        {
            let mut q = vabsq_s16(*row);
            q = vaddq_s16(q, vreinterpretq_s16_u16(vld1q_u16(matrix.add(coeff))));
            q = mulhi_epu16_neon(q, vreinterpretq_s16_u16(vld1q_u16(matrix.add(64 + coeff))));
            q = mulhi_epu16_neon(q, vreinterpretq_s16_u16(vld1q_u16(matrix.add(128 + coeff))));
            *row = sign_epi16_neon(q, *row);
        }

        zigzag_permute_neon(&transformed, out.as_mut_ptr())
    }
}

/// Two's-magnitude-plus-one symbol for one coefficient.
#[cfg(target_arch = "aarch64")]
#[inline]
fn symbol(coeff: i16) -> u32 {
    (get_2mag_sign(coeff) as u32).wrapping_add(1)
}

/// Encode using NEON on aarch64; scalar fallback elsewhere.
pub fn encode_plane(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON/ASIMD is baseline on Rust aarch64 targets.
        unsafe {
            encode_plane_neon(plane, dc, ac, encode_matrix, dc_shift, temp_block);
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn encode_plane_neon(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    unsafe {
        if encode_matrix.len() < 192 {
            return encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
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
                let m_index = fdct_quant_zig_neon(
                    plane.data.as_ptr().add(src_off),
                    plane.stride,
                    encode_matrix.as_ptr(),
                    add_val,
                    temp_block,
                );
                let dc_val = temp_block[0].wrapping_add(dc_round) >> dc_shift;

                dc.encode_dc(dc_val.wrapping_sub(dc_pred));
                dc.emit_bits32();
                dc_pred = dc_val;
                if m_index == 0 {
                    num_zeros += 64;
                    continue;
                }

                let mut m = m_index;
                let nz = m.trailing_zeros() as usize;
                num_zeros += nz as u32;
                ac.encode_zeros(&mut num_zeros);
                ac.emit_bits32();
                ac.encode_value(symbol(temp_block[nz]));
                let mut pos = nz + 1;
                m >>= nz + 1;
                ac.emit_bits32();
                while m != 0 {
                    let zeros = m.trailing_zeros() as usize;
                    ac.encode_zeros_small(zeros as u64);
                    ac.encode_value(symbol(temp_block[pos + zeros]));
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
}

/// Decode using NEON on aarch64; scalar fallback elsewhere.
pub fn decode_plane(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON/ASIMD is baseline on Rust aarch64 targets.
        unsafe {
            decode_plane_neon(plane, dc, ac, decode_matrix, dc_shift, temp_block);
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn decode_plane_neon(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    use crate::bitstream::get_int_from_2mag_sign;

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
                unsafe {
                    zig_invquant_idct_neon(
                        temp_block,
                        decode_matrix,
                        plane.data.as_mut_ptr().add(dst_off),
                        stride,
                        add_val,
                    );
                }
            } else {
                broadcast_dc(temp_block[0], &mut plane.data[dst_off..], stride, add_val);
            }
        }
    }

    ac.rewind_overread(terms_to_decode);
    dc.flush_remaining_read_bits();
    ac.flush_remaining_read_bits();
}

/// Byte indices used to gather the entropy (zigzag) array back into raster
/// rows. `A` selects bytes from coefficient rows 0..3, `B` from rows 4..7.
#[cfg(target_arch = "aarch64")]
const DEC_IDX_A: [[u8; 16]; 8] = [
    [0, 1, 2, 3, 10, 11, 12, 13, 28, 29, 30, 31, 54, 55, 56, 57],
    [4, 5, 8, 9, 14, 15, 26, 27, 32, 33, 52, 53, 58, 59, 255, 255],
    [
        6, 7, 16, 17, 24, 25, 34, 35, 50, 51, 60, 61, 255, 255, 255, 255,
    ],
    [
        18, 19, 22, 23, 36, 37, 48, 49, 62, 63, 255, 255, 255, 255, 255, 255,
    ],
    [
        20, 21, 38, 39, 46, 47, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        40, 41, 44, 45, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        42, 43, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
];

#[cfg(target_arch = "aarch64")]
const DEC_IDX_B: [[u8; 16]; 8] = [
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 20, 21,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 18, 19, 22, 23,
    ],
    [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 16, 17, 24, 25, 42, 43,
    ],
    [
        255, 255, 255, 255, 255, 255, 0, 1, 14, 15, 26, 27, 40, 41, 44, 45,
    ],
    [
        255, 255, 255, 255, 2, 3, 12, 13, 28, 29, 38, 39, 46, 47, 56, 57,
    ],
    [
        255, 255, 4, 5, 10, 11, 30, 31, 36, 37, 48, 49, 54, 55, 58, 59,
    ],
    [6, 7, 8, 9, 32, 33, 34, 35, 50, 51, 52, 53, 60, 61, 62, 63],
];

/// NEON inverse zigzag (entropy → spatial) for one 8×8 block.
///
/// Bit-identical to the scalar gather via [`ZIGZAG_INV`]: two `vqtbl4q_u8`
/// gathers per output row cover the whole 128-byte coefficient array.
///
/// # Safety
/// Caller must enable NEON.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn inverse_zigzag_8x8_neon(coeffs: &[i16; 64]) -> [int16x8_t; 8] {
    unsafe {
        let p = coeffs.as_ptr();
        let a = uint8x16x4_t(
            vreinterpretq_u8_s16(vld1q_s16(p)),
            vreinterpretq_u8_s16(vld1q_s16(p.add(8))),
            vreinterpretq_u8_s16(vld1q_s16(p.add(16))),
            vreinterpretq_u8_s16(vld1q_s16(p.add(24))),
        );
        let b = uint8x16x4_t(
            vreinterpretq_u8_s16(vld1q_s16(p.add(32))),
            vreinterpretq_u8_s16(vld1q_s16(p.add(40))),
            vreinterpretq_u8_s16(vld1q_s16(p.add(48))),
            vreinterpretq_u8_s16(vld1q_s16(p.add(56))),
        );
        let mut rows = [vdupq_n_s16(0); 8];
        for (k, row) in rows.iter_mut().enumerate() {
            let idx_a = vld1q_u8(DEC_IDX_A[k].as_ptr());
            let idx_b = vld1q_u8(DEC_IDX_B[k].as_ptr());
            *row = vreinterpretq_s16_u8(vorrq_u8(vqtbl4q_u8(a, idx_a), vqtbl4q_u8(b, idx_b)));
        }
        rows
    }
}

/// NEON inverse zigzag + dequant + IDCT + pack/store.
///
/// # Safety
/// Caller must enable NEON. `dst` must cover an 8×8 block at `stride`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn zig_invquant_idct_neon(
    coeffs: &mut [i16; 64],
    decode_matrix: &[u16],
    dst: *mut u8,
    stride: usize,
    add_val: i16,
) {
    unsafe {
        let spatial = inverse_zigzag_8x8_neon(coeffs);

        let mut rows = [vdupq_n_s16(0); 8];
        for row in 0..8 {
            let m = vreinterpretq_s16_u16(vld1q_u16(decode_matrix.as_ptr().add(row * 8)));
            let q = vshrq_n_s16(vmulq_s16(spatial[row], m), 4);
            let mut row_arr = [0i16; 8];
            vst1q_s16(row_arr.as_mut_ptr(), q);
            rows[row] = idct_row_neon_vec(row_arr, IDCT_ROW_TABLES[row]);
        }

        let out_rows = idct_columns_8_neon(rows, add_val);
        for (y, row) in out_rows.iter().enumerate() {
            let bytes = vqmovun_s16(*row);
            vst1_u8(dst.add(y * stride), bytes);
        }
    }
}

/// NEON IDCT row — bit-compatible with scalar `idct_row`.
///
/// # Safety
/// Caller must enable NEON; `tab` must contain 32 `i16` values.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn idct_row_neon(x: [i16; 8], tab: &[i16; 32]) -> [i16; 8] {
    unsafe {
        let v = idct_row_neon_vec(x, tab);
        let mut out = [0i16; 8];
        vst1q_s16(out.as_mut_ptr(), v);
        out
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn idct_row_neon_vec(x: [i16; 8], tab: &[i16; 32]) -> int16x8_t {
    unsafe {
        let a02 = vld1q_s16([x[0], x[2], x[0], x[2], x[0], x[2], x[0], x[2]].as_ptr());
        let a46 = vld1q_s16([x[4], x[6], x[4], x[6], x[4], x[6], x[4], x[6]].as_ptr());
        let a13 = vld1q_s16([x[1], x[3], x[1], x[3], x[1], x[3], x[1], x[3]].as_ptr());
        let a57 = vld1q_s16([x[5], x[7], x[5], x[7], x[5], x[7], x[5], x[7]].as_ptr());

        let even = vaddq_s32(
            vaddq_s32(
                madd_epi16_neon(a02, vld1q_s16(tab.as_ptr())),
                madd_epi16_neon(a46, vld1q_s16(tab.as_ptr().add(8))),
            ),
            vdupq_n_s32(IRND_INV_ROW),
        );
        let odd = vaddq_s32(
            madd_epi16_neon(a13, vld1q_s16(tab.as_ptr().add(16))),
            madd_epi16_neon(a57, vld1q_s16(tab.as_ptr().add(24))),
        );

        let sum = vshrq_n_s32(vaddq_s32(even, odd), SHIFT_INV_ROW);
        let diff = vshrq_n_s32(vsubq_s32(even, odd), SHIFT_INV_ROW);
        // Saturating pack i32→i16 for low/high halves.
        let lo = vcombine_s16(vqmovn_s32(sum), vqmovn_s32(sum));
        let hi = vcombine_s16(vqmovn_s32(diff), vqmovn_s32(diff));
        // Reverse diff lanes for out[7-i]: [d0,d1,d2,d3] → [d3,d2,d1,d0]
        let rev = vrev64_s16(vget_low_s16(hi));
        vcombine_s16(vget_low_s16(lo), rev)
    }
}

/// NEON IDCT column pass over eight columns in parallel.
///
/// # Safety
/// Caller must enable NEON.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn idct_columns_8_neon(rows: [int16x8_t; 8], add_val: i16) -> [int16x8_t; 8] {
    unsafe {
        let (r0, r1, r2, r3, r4, r5, r6, r7) = (
            rows[0], rows[1], rows[2], rows[3], rows[4], rows[5], rows[6], rows[7],
        );

        let tg1 = vdupq_n_s16(IDCT_TG1);
        let tg2 = vdupq_n_s16(IDCT_TG2);
        let tg3 = vdupq_n_s16(IDCT_TG3);
        let cos4 = vdupq_n_s16(IDCT_COS4);
        let one = vdupq_n_s16(1);

        let mut x0 = vqaddq_s16(mulhi_epi16_neon(r5, tg3), r5);
        let x1 = vqaddq_s16(mulhi_epi16_neon(r3, tg3), r3);
        x0 = vqaddq_s16(x0, r3);
        let x2 = vqsubq_s16(r5, x1);
        let x5 = vqsubq_s16(mulhi_epi16_neon(r1, tg1), r7);
        let x4 = vqaddq_s16(mulhi_epi16_neon(r7, tg1), r1);

        let temp7 = vqaddq_s16(vqaddq_s16(x0, x4), one);
        let t4 = vqsubq_s16(x4, x0);
        let t5 = vqaddq_s16(vqsubq_s16(x5, x2), one);
        let temp3 = vqaddq_s16(x5, x2);

        let s = vqaddq_s16(t4, t5);
        let d = vqsubq_s16(t4, t5);
        let m4 = vorrq_s16(vqaddq_s16(s, mulhi_epi16_neon(cos4, s)), one);
        let m0 = vorrq_s16(vqaddq_s16(mulhi_epi16_neon(cos4, d), d), one);

        let e7 = vqaddq_s16(mulhi_epi16_neon(r6, tg2), r2);
        let e3 = vqsubq_s16(mulhi_epi16_neon(r2, tg2), r6);
        let sum04 = vqaddq_s16(r4, r0);
        let dif04 = vqsubq_s16(r0, r4);

        let rnd_col = vdupq_n_s16(IRND_INV_COL as i16);
        let rnd_corr = vdupq_n_s16(IRND_INV_CORR as i16);
        let b0 = vqaddq_s16(vqaddq_s16(sum04, e7), rnd_col);
        let b3 = vqaddq_s16(vqsubq_s16(sum04, e7), rnd_corr);
        let b1 = vqaddq_s16(vqaddq_s16(dif04, e3), rnd_col);
        let b2 = vqaddq_s16(vqsubq_s16(dif04, e3), rnd_corr);

        let add = vdupq_n_s16(add_val);
        let fin = |v: int16x8_t| vqaddq_s16(vshrq_n_s16(v, SHIFT_INV_COL), add);
        [
            fin(vqaddq_s16(temp7, b0)),
            fin(vqaddq_s16(b1, m4)),
            fin(vqaddq_s16(b2, m0)),
            fin(vqaddq_s16(temp3, b3)),
            fin(vqsubq_s16(b3, temp3)),
            fin(vqsubq_s16(b2, m0)),
            fin(vqsubq_s16(b1, m4)),
            fin(vqsubq_s16(b0, temp7)),
        ]
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::{fdct_quant_zig_neon, idct_row_neon, zig_invquant_idct_neon};
    use crate::codec::dct::{fdct_quant_zig, idct_row, zig_invquant_idct};
    use crate::tables::IDCT_ROW_TABLES;

    #[test]
    fn fdct_quant_zig_neon_matches_scalar_oracle() {
        let stride = 13;
        let mut src = [0u8; 8 * 13];
        for (i, pixel) in src.iter_mut().enumerate() {
            *pixel = ((i * 73 + 19) % 256) as u8;
        }
        let mut matrix = [0u16; 192];
        for i in 0..64 {
            matrix[i] = (i as u16 % 17) + 1;
            matrix[64 + i] = u16::MAX;
            matrix[128 + i] = u16::MAX;
        }

        for add_val in [-128, 0] {
            let mut expected = [0i16; 64];
            let mut actual = [0i16; 64];
            fdct_quant_zig(&src, stride, &matrix, add_val, &mut expected);
            unsafe {
                let _mask = fdct_quant_zig_neon(
                    src.as_ptr(),
                    stride,
                    matrix.as_ptr(),
                    add_val,
                    &mut actual,
                );
            }
            assert_eq!(actual, expected, "add_val {add_val}");
        }
    }

    #[test]
    fn idct_row_neon_matches_scalar() {
        for (ti, tab) in IDCT_ROW_TABLES.iter().enumerate() {
            for seed in 0..32i16 {
                let mut x = [0i16; 8];
                for (i, slot) in x.iter_mut().enumerate() {
                    *slot = ((i as i16 + seed) * 17) - 40;
                }
                let expected = idct_row(x, tab);
                let actual = unsafe { idct_row_neon(x, tab) };
                assert_eq!(actual, expected, "table {ti} seed {seed}");
            }
        }
    }

    #[test]
    fn zig_invquant_idct_neon_matches_scalar() {
        for add_val in [0i16, 128, -128] {
            for seed in 0..16usize {
                let mut coeffs = [0i16; 64];
                for (i, c) in coeffs.iter_mut().enumerate() {
                    *c = (((i + seed) as i16) % 11) - 5;
                }
                let mut matrix = [0u16; 64];
                for (i, m) in matrix.iter_mut().enumerate() {
                    *m = ((i as u16 + seed as u16) % 13) + 1;
                }

                let mut coeffs_s = coeffs;
                let mut dst_s = [0u8; 8 * 16];
                zig_invquant_idct(&mut coeffs_s, &matrix, &mut dst_s, 16, add_val);

                let mut coeffs_n = coeffs;
                let mut dst_n = [0u8; 8 * 16];
                unsafe {
                    zig_invquant_idct_neon(&mut coeffs_n, &matrix, dst_n.as_mut_ptr(), 16, add_val);
                }
                assert_eq!(dst_n, dst_s, "add_val {add_val} seed {seed}");
            }
        }
    }
}
