//! AArch64 NEON 8-bit plane encode/decode (native NEON, not sse2neon).
//!
//! Encode: native NEON FDCT + quant + zigzag (`fdct_quant_zig_neon`), mirroring
//! `sse128::fdct_quant_zig_sse`. Decode: NEON dequant + pack/store with scalar
//! IDCT row/column passes (same hybrid strategy as the SSE path).

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
use crate::codec::dct::{broadcast_dc, idct_column, idct_row};
#[cfg(target_arch = "aarch64")]
use crate::tables::{
    FDCT_ROUND1, FDCT_SQRT2, FDCT_TAN1, FDCT_TAN2, FDCT_TAN3, FTAB1_128, FTAB2_128, FTAB3_128,
    FTAB4_128, IDCT_ROW_TABLES, RND_FRW_ROW, SHIFT_FRW_COL, SHIFT_FRW_ROW, ZIGZAG_INV,
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

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn sign_epi16_neon(a: int16x8_t, b: int16x8_t) -> int16x8_t {
    unsafe {
        let zero = vceqzq_s16(a);
        let nonzero = vorrq_s16(a, vdupq_n_s16(1));
        let sign = vshrq_n_s16(nonzero, 15);
        let masked = vmulq_s16(b, sign);
        vbslq_s16(zero, vdupq_n_s16(0), masked)
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
) {
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
            *row = sign_epi16_neon(*row, q);
        }

        let mut spatial = [0i16; 64];
        for (i, row) in transformed.iter().enumerate() {
            vst1q_s16(spatial.as_mut_ptr().add(i * 8), *row);
        }
        for i in 0..64 {
            out[ZIGZAG_INV[i] as usize] = spatial[i];
        }
    }
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
                fdct_quant_zig_neon(
                    plane.data.as_ptr().add(src_off),
                    plane.stride,
                    encode_matrix.as_ptr(),
                    add_val,
                    temp_block,
                );
                let dc_val = temp_block[0].wrapping_add(dc_round) >> dc_shift;
                let m_index = ac_nonzero_mask_neon(temp_block);

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
}

/// Build a 64-bit mask of nonzero AC coefficients (bit `i` for `coeffs[i]`, `i >= 1`).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn ac_nonzero_mask_neon(coeffs: &[i16; 64]) -> u64 {
    unsafe {
        let zero = vdupq_n_s16(0);
        let mut mask = 0u64;
        if coeffs[1] != 0 {
            mask |= 1u64 << 1;
        }
        let mut idx = 2usize;
        while idx + 8 <= 64 {
            let v = vld1q_s16(coeffs.as_ptr().add(idx));
            let eq = vceqq_s16(v, zero);
            let mut eq_u = [0u16; 8];
            vst1q_u16(eq_u.as_mut_ptr(), eq);
            for (lane, &e) in eq_u.iter().enumerate() {
                if e == 0 {
                    mask |= 1u64 << (idx + lane);
                }
            }
            idx += 8;
        }
        while idx < 64 {
            if coeffs[idx] != 0 {
                mask |= 1u64 << idx;
            }
            idx += 1;
        }
        mask
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

/// NEON inverse zigzag + dequant + scalar IDCT + NEON pack/store.
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
        let mut spatial = [0i16; 64];
        for i in 0..64 {
            spatial[i] = coeffs[ZIGZAG_INV[i] as usize];
        }

        for row in 0..8 {
            let c = vld1q_s16(spatial.as_ptr().add(row * 8));
            let m = vreinterpretq_s16_u16(vld1q_u16(decode_matrix.as_ptr().add(row * 8)));
            let q = vshrq_n_s16(vmulq_s16(c, m), 4);
            vst1q_s16(spatial.as_mut_ptr().add(row * 8), q);
        }

        let mut rows = [[0i16; 8]; 8];
        for y in 0..8 {
            let mut row = [0i16; 8];
            row.copy_from_slice(&spatial[y * 8..y * 8 + 8]);
            rows[y] = idct_row(row, IDCT_ROW_TABLES[y]);
        }

        let mut out_rows = [[0i16; 8]; 8];
        for x in 0..8 {
            let col = [
                rows[0][x], rows[1][x], rows[2][x], rows[3][x], rows[4][x], rows[5][x], rows[6][x],
                rows[7][x],
            ];
            let out = idct_column(col, 0);
            for y in 0..8 {
                out_rows[y][x] = out[y];
            }
        }

        let add = vdupq_n_s16(add_val);
        for (y, row) in out_rows.iter().enumerate() {
            let v = vqaddq_s16(vld1q_s16(row.as_ptr()), add);
            let bytes = vqmovun_s16(v);
            vst1_u8(dst.add(y * stride), bytes);
        }
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::{ac_nonzero_mask_neon, fdct_quant_zig_neon, zig_invquant_idct_neon};
    use crate::codec::dct::{fdct_quant_zig, zig_invquant_idct};

    fn ac_nonzero_mask_scalar(coeffs: &[i16; 64]) -> u64 {
        let mut mask = 0u64;
        for (i, coeff) in coeffs.iter().enumerate().skip(1) {
            if *coeff != 0 {
                mask |= 1u64 << i;
            }
        }
        mask
    }

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
                fdct_quant_zig_neon(src.as_ptr(), stride, matrix.as_ptr(), add_val, &mut actual);
            }
            assert_eq!(actual, expected, "add_val {add_val}");
        }
    }

    #[test]
    fn ac_nonzero_mask_neon_matches_scalar() {
        let mut coeffs = [0i16; 64];
        for (i, c) in coeffs.iter_mut().enumerate() {
            *c = if (i * 17 + 3) % 11 == 0 {
                0
            } else {
                ((i as i16) - 32) * 3
            };
        }
        let expected = ac_nonzero_mask_scalar(&coeffs);
        let actual = unsafe { ac_nonzero_mask_neon(&coeffs) };
        assert_eq!(actual, expected);
    }

    #[test]
    fn zig_invquant_idct_neon_matches_scalar() {
        let mut coeffs = [0i16; 64];
        for (i, c) in coeffs.iter_mut().enumerate() {
            *c = ((i as i16) % 7) - 3;
        }
        let mut matrix = [0u16; 64];
        for (i, m) in matrix.iter_mut().enumerate() {
            *m = (i as u16 % 13) + 1;
        }

        let mut coeffs_s = coeffs;
        let mut dst_s = [0u8; 8 * 16];
        zig_invquant_idct(&mut coeffs_s, &matrix, &mut dst_s, 16, 128);

        let mut coeffs_n = coeffs;
        let mut dst_n = [0u8; 8 * 16];
        unsafe {
            zig_invquant_idct_neon(&mut coeffs_n, &matrix, dst_n.as_mut_ptr(), 16, 128);
        }
        assert_eq!(dst_n, dst_s);
    }
}
