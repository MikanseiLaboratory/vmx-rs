//! SSE4.2 / SSSE3 encode path.
//!
//! Safety: all `std::arch::x86_64` usage must be gated by `is_x86_feature_detected!`
//! and only operate on buffers with verified lengths. Matrix loads are explicitly
//! unaligned, unlike the original C implementation.

#![allow(dead_code)]

use std::arch::x86_64::*;

use crate::bitstream::{SliceData, get_2mag_sign};
use crate::codec::plane::{PlaneView, decode_plane_scalar, encode_plane_scalar};
use crate::tables::{
    FDCT_ROUND1, FDCT_SQRT2, FDCT_TAN1, FDCT_TAN2, FDCT_TAN3, FTAB1_128, FTAB2_128, FTAB3_128,
    FTAB4_128, RND_FRW_ROW, SHIFT_FRW_COL, SHIFT_FRW_ROW, ZIGZAG_INV,
};
use crate::types::SLICE_HEIGHT;

#[target_feature(enable = "sse4.2")]
unsafe fn fdct_row_sse(input: __m128i, ftab: &[i16; 32]) -> __m128i {
    // SAFETY: the caller guarantees SSE4.2 and `ftab` contains 32 i16 values.
    unsafe {
        let mut reversed = _mm_shufflehi_epi16(input, 0b0001_1011);
        let input = _mm_shuffle_epi32(input, 0b0100_0100);
        reversed = _mm_shuffle_epi32(reversed, 0b1110_1110);

        let sums = _mm_adds_epi16(input, reversed);
        let diffs = _mm_subs_epi16(input, reversed);
        let full = _mm_unpacklo_epi32(sums, diffs);
        let shuffled = _mm_shuffle_epi32(full, 0b0100_1110);

        let temp1 = _mm_madd_epi16(
            shuffled,
            _mm_loadu_si128(ftab.as_ptr().add(8).cast::<__m128i>()),
        );
        let temp2 = _mm_madd_epi16(
            full,
            _mm_loadu_si128(ftab.as_ptr().add(16).cast::<__m128i>()),
        );
        let temp3 = _mm_madd_epi16(
            shuffled,
            _mm_loadu_si128(ftab.as_ptr().add(24).cast::<__m128i>()),
        );
        let temp4 = _mm_madd_epi16(full, _mm_loadu_si128(ftab.as_ptr().cast::<__m128i>()));
        let round = _mm_set1_epi32(RND_FRW_ROW);
        let lo = _mm_srai_epi32(
            _mm_add_epi32(_mm_add_epi32(temp4, temp1), round),
            SHIFT_FRW_ROW,
        );
        let hi = _mm_srai_epi32(
            _mm_add_epi32(_mm_add_epi32(temp3, temp2), round),
            SHIFT_FRW_ROW,
        );
        _mm_packs_epi32(lo, hi)
    }
}

/// Forward DCT, quantization and zigzag scan using the SSE4.2 implementation
/// from `VMX_FDCT_8X8_QUANT_ZIG_128`.
///
/// # Safety
/// The caller must have detected SSE4.2 and provide at least eight readable
/// bytes at each `src + row * stride`, plus 192 readable `u16`s at `matrix`.
/// `out` is written in entropy (zigzag) order.
#[target_feature(enable = "sse4.2")]
pub unsafe fn fdct_quant_zig_sse(
    src: *const u8,
    stride: usize,
    matrix: *const u16,
    add_val: i16,
    out: &mut [i16; 64],
) {
    // SAFETY: upheld by this function's documented preconditions.
    unsafe {
        let mut rows = [
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(stride).cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(2 * stride).cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(3 * stride).cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(4 * stride).cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(5 * stride).cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(6 * stride).cast::<__m128i>())),
            _mm_cvtepu8_epi16(_mm_loadl_epi64(src.add(7 * stride).cast::<__m128i>())),
        ];
        let add = _mm_set1_epi16(add_val);
        for row in &mut rows {
            *row = _mm_adds_epi16(*row, add);
        }

        let (mut xmm0, mut xmm2, mut xmm7, mut xmm5) = (rows[0], rows[2], rows[7], rows[5]);
        let xmm3_copy = xmm0;
        let xmm4_copy = xmm2;
        xmm0 = _mm_subs_epi16(xmm0, xmm7);
        xmm7 = _mm_adds_epi16(xmm7, xmm3_copy);
        xmm2 = _mm_subs_epi16(xmm2, xmm5);
        xmm5 = _mm_adds_epi16(xmm5, xmm4_copy);

        let (mut xmm3, mut xmm4) = (rows[3], rows[4]);
        let xmm1_copy = xmm3;
        xmm3 = _mm_subs_epi16(xmm3, xmm4);
        xmm4 = _mm_adds_epi16(xmm4, xmm1_copy);
        let (mut xmm6, mut xmm1) = (rows[6], rows[1]);
        let tmp = xmm1;
        xmm1 = _mm_subs_epi16(xmm1, xmm6);
        xmm6 = _mm_adds_epi16(xmm6, tmp);

        let mut tm03 = _mm_subs_epi16(xmm7, xmm4);
        let mut tm12 = _mm_subs_epi16(xmm6, xmm5);
        xmm4 = _mm_adds_epi16(xmm4, xmm4);
        xmm5 = _mm_adds_epi16(xmm5, xmm5);
        let mut tp03 = _mm_adds_epi16(xmm4, tm03);
        let mut tp12 = _mm_adds_epi16(xmm5, tm12);

        xmm2 = _mm_slli_epi16(xmm2, SHIFT_FRW_COL + 1);
        xmm1 = _mm_slli_epi16(xmm1, SHIFT_FRW_COL + 1);
        tp03 = _mm_slli_epi16(tp03, SHIFT_FRW_COL);
        tp12 = _mm_slli_epi16(tp12, SHIFT_FRW_COL);
        tm03 = _mm_slli_epi16(tm03, SHIFT_FRW_COL);
        tm12 = _mm_slli_epi16(tm12, SHIFT_FRW_COL);
        xmm3 = _mm_slli_epi16(xmm3, SHIFT_FRW_COL);
        xmm0 = _mm_slli_epi16(xmm0, SHIFT_FRW_COL);

        let mut in4 = _mm_subs_epi16(tp03, tp12);
        let diff = _mm_subs_epi16(xmm1, xmm2);
        tp12 = _mm_adds_epi16(tp12, tp12);
        xmm2 = _mm_adds_epi16(xmm2, xmm2);
        let mut in0 = _mm_adds_epi16(tp12, in4);
        let sum = _mm_adds_epi16(xmm2, diff);

        let tan2 = _mm_set1_epi16(FDCT_TAN2);
        let mut in6 = _mm_subs_epi16(_mm_mulhi_epi16(tan2, tm03), tm12);
        let mut in2 = _mm_adds_epi16(_mm_mulhi_epi16(tan2, tm12), tm03);
        let sqrt2 = _mm_set1_epi16(FDCT_SQRT2);
        let rounder = _mm_set1_epi16(FDCT_ROUND1);
        let tp65 = _mm_or_si128(_mm_mulhi_epi16(sum, sqrt2), rounder);
        in2 = _mm_or_si128(in2, rounder);
        in6 = _mm_or_si128(in6, rounder);
        let tm65 = _mm_mulhi_epi16(diff, sqrt2);

        let tm465 = _mm_subs_epi16(xmm3, tm65);
        let tm765 = _mm_subs_epi16(xmm0, tp65);
        let tp765 = _mm_adds_epi16(tp65, xmm0);
        let tp465 = _mm_adds_epi16(tm65, xmm3);
        let tan3 = _mm_set1_epi16(FDCT_TAN3);
        let tan1 = _mm_set1_epi16(FDCT_TAN1);
        let tmp3 = _mm_adds_epi16(_mm_mulhi_epi16(tm465, tan3), tm465);
        let tmp5 = _mm_adds_epi16(_mm_mulhi_epi16(tm765, tan3), tm765);
        let mut in1 = _mm_adds_epi16(_mm_mulhi_epi16(tp465, tan1), tp765);
        let mut in3 = _mm_subs_epi16(tm765, tmp3);
        let mut in5 = _mm_adds_epi16(tm465, tmp5);
        let mut in7 = _mm_subs_epi16(_mm_mulhi_epi16(tp765, tan1), tp465);

        in0 = fdct_row_sse(in0, &FTAB1_128);
        in1 = fdct_row_sse(in1, &FTAB2_128);
        in2 = fdct_row_sse(in2, &FTAB3_128);
        in3 = fdct_row_sse(in3, &FTAB4_128);
        in4 = fdct_row_sse(in4, &FTAB1_128);
        in5 = fdct_row_sse(in5, &FTAB4_128);
        in6 = fdct_row_sse(in6, &FTAB3_128);
        in7 = fdct_row_sse(in7, &FTAB2_128);

        let mut transformed = [in0, in1, in2, in3, in4, in5, in6, in7];
        for (row, &coeff) in transformed
            .iter_mut()
            .zip([0, 8, 16, 24, 32, 40, 48, 56].iter())
        {
            let mut q = _mm_abs_epi16(*row);
            q = _mm_add_epi16(q, _mm_loadu_si128(matrix.add(coeff).cast::<__m128i>()));
            q = _mm_mulhi_epu16(q, _mm_loadu_si128(matrix.add(64 + coeff).cast::<__m128i>()));
            q = _mm_mulhi_epu16(
                q,
                _mm_loadu_si128(matrix.add(128 + coeff).cast::<__m128i>()),
            );
            *row = _mm_sign_epi16(q, *row);
        }

        let mut spatial = [0i16; 64];
        for (i, row) in transformed.iter().enumerate() {
            _mm_storeu_si128(spatial.as_mut_ptr().add(i * 8).cast::<__m128i>(), *row);
        }
        for i in 0..64 {
            out[ZIGZAG_INV[i] as usize] = spatial[i];
        }
    }
}

/// Encode using SSE128 when available; falls back to scalar.
pub fn encode_plane(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse4.2") {
            // SAFETY: feature detected; plane bounds and encode matrix length are
            // checked by the same block loop used by the scalar implementation.
            return unsafe {
                encode_plane_sse(plane, dc, ac, encode_matrix, dc_shift, temp_block);
            };
        }
    }
    encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
}

#[target_feature(enable = "sse4.2")]
unsafe fn encode_plane_sse(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    // SAFETY: `encode_plane` has dispatched only after detecting SSE4.2.
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
                fdct_quant_zig_sse(
                    plane.data.as_ptr().add(src_off),
                    plane.stride,
                    encode_matrix.as_ptr(),
                    add_val,
                    temp_block,
                );
                let dc_val = temp_block[0].wrapping_add(dc_round) >> dc_shift;
                let mut m_index = 0u64;
                for (i, coeff) in temp_block.iter().enumerate().skip(1) {
                    if *coeff != 0 {
                        m_index |= 1u64 << i;
                    }
                }
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

pub fn decode_plane(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse4.2") {
            return decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
        }
    }
    decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
}

#[cfg(test)]
mod tests {
    use super::fdct_quant_zig_sse;
    use crate::codec::dct::fdct_quant_zig;

    #[test]
    fn fdct_quant_zig_matches_scalar_oracle() {
        if !is_x86_feature_detected!("sse4.2") {
            return;
        }

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
            // SAFETY: the test has verified the required target feature and the
            // fixed arrays satisfy the documented source and matrix lengths.
            unsafe {
                fdct_quant_zig_sse(src.as_ptr(), stride, matrix.as_ptr(), add_val, &mut actual);
            }
            assert_eq!(actual, expected, "add_val {add_val}");
        }
    }
}
