//! SSE4.2 / SSSE3 encode and SSE4.1 / SSSE3 decode path (x86_64 only; other
//! targets use scalar via the public `encode_plane` / `decode_plane` entry points).
//!
//! Encode: FDCT + quant + zigzag (SSE4.2). Decode: inverse zigzag + dequant +
//! IDCT + packus (SSSE3 + SSE4.1).
//!
//! Safety: all `std::arch::x86_64` usage must be gated by `is_x86_feature_detected!`
//! and only operate on buffers with verified lengths. Matrix loads are explicitly
//! unaligned, unlike the original C implementation.

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::{PlaneView, decode_plane_scalar, encode_plane_scalar};

#[cfg(target_arch = "x86_64")]
use crate::bitstream::get_2mag_sign;
#[cfg(target_arch = "x86_64")]
use crate::tables::{
    FDCT_ROUND1, FDCT_SQRT2, FDCT_TAN1, FDCT_TAN2, FDCT_TAN3, FTAB1_128, FTAB2_128, FTAB3_128,
    FTAB4_128, RND_FRW_ROW, SHIFT_FRW_COL, SHIFT_FRW_ROW, ZIGZAG_INV,
};
#[cfg(target_arch = "x86_64")]
use crate::types::SLICE_HEIGHT;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
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
#[cfg(target_arch = "x86_64")]
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

/// Build a 64-bit mask of nonzero AC coefficients (bit `i` set when `coeffs[i] != 0`).
/// Bit 0 (DC) is always clear.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
unsafe fn ac_nonzero_mask_sse(coeffs: &[i16; 64]) -> u64 {
    unsafe {
        let zero = _mm_setzero_si128();
        let mut mask = 0u64;
        for chunk in 0..8 {
            let v = _mm_loadu_si128(coeffs.as_ptr().add(chunk * 8).cast());
            let eq = _mm_cmpeq_epi16(v, zero);
            let packed = _mm_packs_epi16(eq, zero);
            let is_zero = _mm_movemask_epi8(packed) as u8;
            mask |= u64::from(!is_zero) << (chunk * 8);
        }
        mask & !1
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

#[cfg(target_arch = "x86_64")]
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
                let m_index = ac_nonzero_mask_sse(temp_block);
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
        if is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("ssse3") {
            return decode_plane_sse(plane, dc, ac, decode_matrix, dc_shift, temp_block);
        }
    }
    decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
}

#[cfg(target_arch = "x86_64")]
fn decode_plane_sse(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    use crate::bitstream::get_int_from_2mag_sign;
    use crate::codec::dct::broadcast_dc;
    use crate::types::SLICE_HEIGHT;

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
                // SAFETY: SSE4.1 detected by caller.
                unsafe {
                    zig_invquant_idct_sse(
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

/// Inverse zigzag (entropy → spatial) for one 8×8 block.
///
/// Port of libvmx `VMX_ZIG_INVQUANTIZE_IDCT_8X8_128` shuffle/blend (~2327–2404).
/// Bit-identical to scalar gather via [`crate::tables::ZIGZAG_INV`].
///
/// # Safety
/// Caller must enable SSSE3+SSE4.1.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3,sse4.1")]
unsafe fn inverse_zigzag_8x8_sse(coeffs: &[i16; 64]) -> [__m128i; 8] {
    // SAFETY: SSSE3+SSE4.1 enabled; coeffs is 64 i16.
    unsafe {
        let mut a0 = _mm_loadu_si128(coeffs.as_ptr().cast());
        let mut a1 = _mm_loadu_si128(coeffs.as_ptr().add(8).cast());
        let mut a2 = _mm_loadu_si128(coeffs.as_ptr().add(16).cast());
        let mut a3 = _mm_loadu_si128(coeffs.as_ptr().add(24).cast());
        let mut a4 = _mm_loadu_si128(coeffs.as_ptr().add(32).cast());
        let mut a5 = _mm_loadu_si128(coeffs.as_ptr().add(40).cast());
        let a6_src = _mm_loadu_si128(coeffs.as_ptr().add(48).cast());
        let a7_src = _mm_loadu_si128(coeffs.as_ptr().add(56).cast());

        let mut v0 = _mm_shuffle_epi8(
            a0,
            _mm_set_epi8(7, 6, 15, 14, 9, 8, 5, 4, 13, 12, 11, 10, 3, 2, 1, 0),
        );
        let v1 = _mm_shuffle_epi8(
            a1,
            _mm_set_epi8(7, 6, 3, 2, 15, 14, 13, 12, 11, 10, 9, 8, 1, 0, 5, 4),
        );
        let mut v3 = _mm_shuffle_epi8(
            a3,
            _mm_set_epi8(9, 8, 7, 6, 13, 12, 3, 2, 11, 10, 5, 4, 15, 14, 1, 0),
        );

        a0 = _mm_blend_epi16::<0x30>(v0, v1);
        a0 = _mm_blend_epi16::<0xC0>(a0, v3);

        let mut v2 = _mm_shuffle_epi8(
            a2,
            _mm_set_epi8(5, 4, 13, 12, 9, 8, 1, 0, 3, 2, 15, 14, 7, 6, 11, 10),
        );
        let mut v5 = _mm_shuffle_epi8(
            a5,
            _mm_set_epi8(7, 6, 3, 2, 11, 10, 13, 12, 15, 14, 5, 4, 9, 8, 1, 0),
        );

        a2 = _mm_srli_si128::<14>(v0);
        a2 = _mm_blend_epi16::<0x30>(a2, v3);
        a2 = _mm_blend_epi16::<0x6>(a2, v1);
        a2 = _mm_blend_epi16::<0x8>(a2, v2);
        a2 = _mm_blend_epi16::<0xC0>(a2, v5);

        v3 = _mm_slli_si128::<6>(v3);
        let mut v4 = _mm_shuffle_epi8(
            a4,
            _mm_set_epi8(13, 12, 3, 2, 5, 4, 15, 14, 1, 0, 11, 10, 9, 8, 7, 6),
        );

        v0 = _mm_srli_si128::<8>(v0);
        a1 = _mm_blend_epi16::<0x8>(v0, v1);
        a1 = _mm_blend_epi16::<0x10>(a1, v2);
        a1 = _mm_blend_epi16::<0x60>(a1, v3);

        let v6 = _mm_shuffle_epi8(
            a6_src,
            _mm_set_epi8(13, 12, 9, 8, -1, -1, 5, 4, 3, 2, 1, 0, -1, -1, -1, -1),
        );
        let v7 = _mm_shuffle_epi8(
            a7_src,
            _mm_set_epi8(15, 14, 13, 12, 5, 4, 3, 2, 11, 10, 7, 6, 1, 0, 9, 8),
        );

        a4 = _mm_blend_epi16::<0xC0>(v1, v6);
        a4 = _mm_blend_epi16::<0x6>(a4, v2);
        a4 = _mm_blend_epi16::<0x18>(a4, v4);
        a4 = _mm_blend_epi16::<0x20>(a4, v5);

        let x6 = _mm_shuffle_epi8(
            a6_src,
            _mm_set_epi8(11, 10, 15, 14, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1),
        );

        a3 = _mm_srli_si128::<12>(v1);
        a3 = _mm_blend_epi16::<0x18>(a3, v3);
        a3 = _mm_blend_epi16::<0x80>(a3, x6);

        let mut a7 = _mm_blend_epi16::<0x3>(v7, v4);
        a7 = _mm_blend_epi16::<0xC>(a7, v6);

        let mut a6 = _mm_slli_si128::<8>(v7);
        a6 = _mm_blend_epi16::<0x4>(a6, v4);
        a6 = _mm_blend_epi16::<0x10>(a6, v6);
        a6 = _mm_blend_epi16::<0x8>(a6, v5);
        a6 = _mm_blend_epi16::<0x1>(a6, v2);

        v4 = _mm_srli_si128::<8>(v4);

        v2 = _mm_srli_si128::<10>(v2);
        a5 = _mm_slli_si128::<14>(v7);
        a5 = _mm_blend_epi16::<0xC>(a5, v4);
        a5 = _mm_blend_epi16::<0x3>(a5, v2);
        a5 = _mm_blend_epi16::<0x10>(a5, v5);
        a5 = _mm_blend_epi16::<0x60>(a5, x6);

        a6 = _mm_blend_epi16::<0x2>(a6, v4);
        a3 = _mm_blend_epi16::<0x4>(a3, v2);

        v5 = _mm_slli_si128::<10>(v5);
        a3 = _mm_blend_epi16::<0x60>(a3, v5);
        a1 = _mm_blend_epi16::<0x80>(a1, v5);

        [a0, a1, a2, a3, a4, a5, a6, a7]
    }
}

/// SSE4.1/SSSE3 inverse zigzag + dequant + IDCT (matches scalar `zig_invquant_idct`).
///
/// # Safety
/// Caller must have detected SSSE3 and SSE4.1. `dst` must cover an 8×8 block at `stride`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3,sse4.1")]
pub unsafe fn zig_invquant_idct_sse(
    coeffs: &mut [i16; 64],
    decode_matrix: &[u16],
    dst: *mut u8,
    stride: usize,
    add_val: i16,
) {
    use crate::tables::IDCT_ROW_TABLES;

    // SAFETY: SSSE3+SSE4.1 enabled; decode_matrix has 64 entries from codec presets.
    unsafe {
        let spatial = inverse_zigzag_8x8_sse(coeffs);

        // Dequant: 8 lanes × 8 rows via mullo + srai, then IDCT rows.
        let mut rows = [_mm_setzero_si128(); 8];
        for row in 0..8 {
            let m = _mm_loadu_si128(decode_matrix.as_ptr().add(row * 8).cast::<__m128i>());
            let q = _mm_srai_epi16(_mm_mullo_epi16(spatial[row], m), 4);
            rows[row] = idct_row_sse_vec(q, IDCT_ROW_TABLES[row]);
        }

        let out_rows = idct_columns_8_sse(rows, add_val);
        for (y, row) in out_rows.iter().enumerate() {
            let bytes = _mm_packus_epi16(*row, *row);
            _mm_storel_epi64(dst.add(y * stride).cast::<__m128i>(), bytes);
        }
    }
}

/// SSE4.1 IDCT row — bit-compatible with scalar `idct_row` via the same madd layout.
///
/// # Safety
/// Caller must enable SSE4.1; `tab` must contain 32 `i16` values.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn idct_row_sse(x: [i16; 8], tab: &[i16; 32]) -> [i16; 8] {
    // SAFETY: target_feature enables SSE4.1 for this function body.
    let input = unsafe { _mm_loadu_si128(x.as_ptr().cast::<__m128i>()) };
    let v = unsafe { idct_row_sse_vec(input, tab) };
    let mut out = [0i16; 8];
    unsafe {
        _mm_storeu_si128(out.as_mut_ptr().cast::<__m128i>(), v);
    }
    out
}

/// Vector form of [`idct_row_sse`].
///
/// # Safety
/// Caller must enable SSE4.1; `tab` must contain 32 `i16` values.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn idct_row_sse_vec(input: __m128i, tab: &[i16; 32]) -> __m128i {
    use crate::tables::{IRND_INV_ROW, SHIFT_INV_ROW};

    // SAFETY: caller enabled SSE4.1; tab length 32.
    unsafe {
        // Broadcast (x0,x2), (x1,x3), (x4,x6), (x5,x7) pairs for madd, matching
        // the AVX2 `idct_row_avx2` shuffle layout (and scalar `idct_row`).
        let r = _mm_shufflelo_epi16(input, 0xd8);
        let p0 = _mm_shuffle_epi32(r, 0x00);
        let p1 = _mm_shuffle_epi32(r, 0x55);
        let r = _mm_shufflehi_epi16(r, 0xd8);
        let p2 = _mm_shuffle_epi32(r, 0xaa);
        let p3 = _mm_shuffle_epi32(r, 0xff);

        let even = _mm_add_epi32(
            _mm_add_epi32(
                _mm_madd_epi16(p0, _mm_loadu_si128(tab.as_ptr().cast::<__m128i>())),
                _mm_madd_epi16(p2, _mm_loadu_si128(tab.as_ptr().add(8).cast::<__m128i>())),
            ),
            _mm_set1_epi32(IRND_INV_ROW),
        );
        let odd = _mm_add_epi32(
            _mm_madd_epi16(p1, _mm_loadu_si128(tab.as_ptr().add(16).cast::<__m128i>())),
            _mm_madd_epi16(p3, _mm_loadu_si128(tab.as_ptr().add(24).cast::<__m128i>())),
        );

        let sum = _mm_srai_epi32(_mm_add_epi32(even, odd), SHIFT_INV_ROW);
        let diff = _mm_srai_epi32(_mm_sub_epi32(even, odd), SHIFT_INV_ROW);
        // out = [s0,s1,s2,s3, d3,d2,d1,d0] because out[7-i] = diff[i]
        let lo = _mm_packs_epi32(sum, sum);
        let hi = _mm_packs_epi32(diff, diff);
        let rev = _mm_shufflelo_epi16(hi, 0b00_01_10_11);
        _mm_unpacklo_epi64(lo, rev)
    }
}

/// SSE4.1 IDCT column pass over eight columns in parallel (each lane is one column).
///
/// Matches scalar `idct_column(col, add_val)` lane-wise, including saturating
/// arithmetic, `| 1`, and asymmetric rounding constants.
///
/// # Safety
/// Caller must enable SSE4.1.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn idct_columns_8_sse(rows: [__m128i; 8], add_val: i16) -> [__m128i; 8] {
    use crate::tables::{
        IDCT_COS4, IDCT_TG1, IDCT_TG2, IDCT_TG3, IRND_INV_COL, IRND_INV_CORR, SHIFT_INV_COL,
    };

    // SAFETY: SSE4.1 enabled; rows cover a full 8×8 block.
    let (r0, r1, r2, r3, r4, r5, r6, r7) = (
        rows[0], rows[1], rows[2], rows[3], rows[4], rows[5], rows[6], rows[7],
    );

    let tg1 = _mm_set1_epi16(IDCT_TG1);
    let tg2 = _mm_set1_epi16(IDCT_TG2);
    let tg3 = _mm_set1_epi16(IDCT_TG3);
    let cos4 = _mm_set1_epi16(IDCT_COS4);
    let one = _mm_set1_epi16(1);

    // Odd part
    let mut x0 = _mm_adds_epi16(_mm_mulhi_epi16(r5, tg3), r5);
    let x1 = _mm_adds_epi16(_mm_mulhi_epi16(r3, tg3), r3);
    x0 = _mm_adds_epi16(x0, r3);
    let x2 = _mm_subs_epi16(r5, x1);
    let x5 = _mm_subs_epi16(_mm_mulhi_epi16(r1, tg1), r7);
    let x4 = _mm_adds_epi16(_mm_mulhi_epi16(r7, tg1), r1);

    let temp7 = _mm_adds_epi16(_mm_adds_epi16(x0, x4), one);
    let t4 = _mm_subs_epi16(x4, x0);
    let t5 = _mm_adds_epi16(_mm_subs_epi16(x5, x2), one);
    let temp3 = _mm_adds_epi16(x5, x2);

    let s = _mm_adds_epi16(t4, t5);
    let d = _mm_subs_epi16(t4, t5);
    let m4 = _mm_or_si128(_mm_adds_epi16(s, _mm_mulhi_epi16(cos4, s)), one);
    let m0 = _mm_or_si128(_mm_adds_epi16(_mm_mulhi_epi16(cos4, d), d), one);

    // Even part
    let e7 = _mm_adds_epi16(_mm_mulhi_epi16(r6, tg2), r2);
    let e3 = _mm_subs_epi16(_mm_mulhi_epi16(r2, tg2), r6);
    let sum04 = _mm_adds_epi16(r4, r0);
    let dif04 = _mm_subs_epi16(r0, r4);

    let rnd_col = _mm_set1_epi16(IRND_INV_COL as i16);
    let rnd_corr = _mm_set1_epi16(IRND_INV_CORR as i16);
    let b0 = _mm_adds_epi16(_mm_adds_epi16(sum04, e7), rnd_col);
    let b3 = _mm_adds_epi16(_mm_subs_epi16(sum04, e7), rnd_corr);
    let b1 = _mm_adds_epi16(_mm_adds_epi16(dif04, e3), rnd_col);
    let b2 = _mm_adds_epi16(_mm_subs_epi16(dif04, e3), rnd_corr);

    let add = _mm_set1_epi16(add_val);
    let fin = |v: __m128i| _mm_adds_epi16(_mm_srai_epi16(v, SHIFT_INV_COL), add);
    [
        fin(_mm_adds_epi16(temp7, b0)),
        fin(_mm_adds_epi16(b1, m4)),
        fin(_mm_adds_epi16(b2, m0)),
        fin(_mm_adds_epi16(temp3, b3)),
        fin(_mm_subs_epi16(b3, temp3)),
        fin(_mm_subs_epi16(b2, m0)),
        fin(_mm_subs_epi16(b1, m4)),
        fin(_mm_subs_epi16(b0, temp7)),
    ]
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::{ac_nonzero_mask_sse, fdct_quant_zig_sse, idct_row_sse, zig_invquant_idct_sse};
    use crate::codec::dct::{fdct_quant_zig, idct_row, zig_invquant_idct};
    use crate::tables::IDCT_ROW_TABLES;

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
    fn ac_nonzero_mask_sse_matches_scalar() {
        if !is_x86_feature_detected!("sse4.2") {
            return;
        }
        let mut coeffs = [0i16; 64];
        for (i, c) in coeffs.iter_mut().enumerate() {
            *c = if (i * 17 + 3) % 11 == 0 {
                0
            } else {
                ((i as i16) - 32) * 3
            };
        }
        let expected = ac_nonzero_mask_scalar(&coeffs);
        let actual = unsafe { ac_nonzero_mask_sse(&coeffs) };
        assert_eq!(actual, expected);

        coeffs.fill(0);
        coeffs[0] = 42;
        assert_eq!(
            unsafe { ac_nonzero_mask_sse(&coeffs) },
            ac_nonzero_mask_scalar(&coeffs)
        );

        coeffs.fill(1);
        assert_eq!(
            unsafe { ac_nonzero_mask_sse(&coeffs) },
            ac_nonzero_mask_scalar(&coeffs)
        );

        coeffs.fill(0);
        coeffs[63] = -7;
        assert_eq!(
            unsafe { ac_nonzero_mask_sse(&coeffs) },
            ac_nonzero_mask_scalar(&coeffs)
        );
    }

    #[test]
    fn idct_row_sse_matches_scalar() {
        if !is_x86_feature_detected!("sse4.1") {
            return;
        }
        for (ti, tab) in IDCT_ROW_TABLES.iter().enumerate() {
            for seed in 0..32i16 {
                let mut x = [0i16; 8];
                for (i, slot) in x.iter_mut().enumerate() {
                    *slot = ((i as i16 + seed) * 17) - 40;
                }
                let expected = idct_row(x, tab);
                let actual = unsafe { idct_row_sse(x, tab) };
                assert_eq!(actual, expected, "table {ti} seed {seed}");
            }
        }
    }

    #[test]
    fn zig_invquant_idct_sse_matches_scalar() {
        if !is_x86_feature_detected!("sse4.1") || !is_x86_feature_detected!("ssse3") {
            return;
        }
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

                let mut coeffs_v = coeffs;
                let mut dst_v = [0u8; 8 * 16];
                unsafe {
                    zig_invquant_idct_sse(&mut coeffs_v, &matrix, dst_v.as_mut_ptr(), 16, add_val);
                }
                assert_eq!(dst_v, dst_s, "add_val {add_val} seed {seed}");
            }
        }
    }

    #[test]
    fn idct_sse_faster_than_scalar_in_release() {
        if cfg!(debug_assertions)
            || !is_x86_feature_detected!("sse4.1")
            || !is_x86_feature_detected!("ssse3")
        {
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
        let warmup = 50;
        let iters = 2000;

        for _ in 0..warmup {
            let mut c = coeffs;
            zig_invquant_idct(&mut c, &matrix, &mut dst, 16, 128);
            let mut c = coeffs;
            unsafe {
                zig_invquant_idct_sse(&mut c, &matrix, dst.as_mut_ptr(), 16, 128);
            }
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
            unsafe {
                zig_invquant_idct_sse(&mut c, &matrix, dst.as_mut_ptr(), 16, 128);
            }
        }
        let sse = t0.elapsed();
        eprintln!(
            "idct 8x8 scalar={:.3}us sse={:.3}us ({:.2}x)",
            scalar.as_secs_f64() * 1e6 / iters as f64,
            sse.as_secs_f64() * 1e6 / iters as f64,
            scalar.as_secs_f64() / sse.as_secs_f64().max(1e-12)
        );
        assert!(
            sse * 5 < scalar * 4,
            "SSE IDCT should be faster than scalar (scalar={scalar:?} sse={sse:?})"
        );
    }
}
