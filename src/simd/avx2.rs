//! AVX2+BMI2 encode/decode path (x86_64).
//!
//! Matches libvmx dual 8×8 block iteration (`x += 16` when stride allows).
//! FDCT uses the proven SSE4.2 kernel twice per pair for bit-exactness; AVX2
//! accelerates AC nonzero mask construction. Decode reuses the SSE entropy
//! loop and `zig_invquant_idct_sse` per block.
//!
//! Disabled automatically when chroma width % 16 != 0 (see `Codec::new`).

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::{PlaneView, decode_plane_scalar, encode_plane_scalar};

#[cfg(target_arch = "x86_64")]
use crate::bitstream::get_2mag_sign;
#[cfg(target_arch = "x86_64")]
use crate::types::SLICE_HEIGHT;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Encode using AVX2+BMI2 when available; falls back to scalar.
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
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("bmi2") {
            // SAFETY: features detected; same buffer contracts as scalar path.
            return unsafe {
                encode_plane_avx2(plane, dc, ac, encode_matrix, dc_shift, temp_block);
            };
        }
    }
    encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
}

/// Decode using AVX2+BMI2 when available; falls back to scalar.
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
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("bmi2") {
            return unsafe { decode_plane_avx2(plane, dc, ac, decode_matrix, dc_shift, temp_block) };
        }
    }
    decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi2")]
unsafe fn encode_plane_avx2(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    use crate::simd::sse128::fdct_quant_zig_sse;

    // SAFETY: caller verified AVX2+BMI2.
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
        let stride = plane.stride;
        let base = plane.offset;
        let mut temp_block2 = [0i16; 64];
        let mut used_avx256 = false;

        for y in (0..height).step_by(8) {
            let mut x = 0usize;
            while x < stride {
                let src_off = base + y * stride + x;
                let dual = x + 16 <= stride && src_off + 7 * stride + 16 <= plane.data.len();

                if dual {
                    fdct_quant_zig_sse(
                        plane.data.as_ptr().add(src_off),
                        stride,
                        encode_matrix.as_ptr(),
                        add_val,
                        temp_block,
                    );
                    fdct_quant_zig_sse(
                        plane.data.as_ptr().add(src_off + 8),
                        stride,
                        encode_matrix.as_ptr(),
                        add_val,
                        &mut temp_block2,
                    );
                    encode_fdct_block_avx2(
                        temp_block,
                        dc,
                        ac,
                        dc_shift,
                        dc_round,
                        &mut dc_pred,
                        &mut num_zeros,
                        &mut used_avx256,
                    );
                    encode_fdct_block_avx2(
                        &temp_block2,
                        dc,
                        ac,
                        dc_shift,
                        dc_round,
                        &mut dc_pred,
                        &mut num_zeros,
                        &mut used_avx256,
                    );
                    x += 16;
                } else {
                    if src_off + 7 * stride + 8 > plane.data.len() {
                        x += 8;
                        continue;
                    }
                    fdct_quant_zig_sse(
                        plane.data.as_ptr().add(src_off),
                        stride,
                        encode_matrix.as_ptr(),
                        add_val,
                        temp_block,
                    );
                    encode_fdct_block_avx2(
                        temp_block,
                        dc,
                        ac,
                        dc_shift,
                        dc_round,
                        &mut dc_pred,
                        &mut num_zeros,
                        &mut used_avx256,
                    );
                    x += 8;
                }
            }
        }

        ac.encode_zeros(&mut num_zeros);
        ac.emit_bits32();
        ac.flush_remaining_bits();
        dc.flush_remaining_bits();

        if used_avx256 {
            _mm256_zeroupper();
        }
    }
}

/// Build a 64-bit mask of nonzero AC coefficients (bit `i` set when `coeffs[i] != 0`, `i >= 1`).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi2")]
unsafe fn ac_nonzero_mask_avx2(coeffs: &[i16; 64]) -> u64 {
    // SAFETY: AVX2 enabled; `coeffs` is 64 elements.
    unsafe {
        let zero = _mm256_setzero_si256();
        let base = coeffs.as_ptr().add(1);
        let mut mask = 0u64;

        // Indices 1..48 (three full 16-lane chunks).
        for chunk in 0..3 {
            let offset = chunk * 16;
            let v = _mm256_loadu_si256(base.add(offset).cast());
            let eq = _mm256_cmpeq_epi16(v, zero);
            let eq_mask = _mm256_movemask_epi8(eq) as u32;
            for i in 0..16 {
                if (eq_mask & (3u32 << (2 * i))) != (3u32 << (2 * i)) {
                    mask |= 1u64 << (1 + offset + i);
                }
            }
        }

        // Indices 49..63 (tail — avoid loading past the array end).
        for i in 49..64 {
            if coeffs[i] != 0 {
                mask |= 1u64 << i;
            }
        }

        mask
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi2")]
unsafe fn encode_fdct_block_avx2(
    temp_block: &[i16; 64],
    dc: &mut SliceData,
    ac: &mut SliceData,
    dc_shift: i32,
    dc_round: i16,
    dc_pred: &mut i16,
    num_zeros: &mut u32,
    used_avx256: &mut bool,
) {
    // SAFETY: AVX2+BMI2 enabled.
    unsafe {
        let dc_val = temp_block[0].wrapping_add(dc_round) >> dc_shift;
        let m_index = ac_nonzero_mask_avx2(temp_block);
        *used_avx256 = true;

        dc.encode_dc(dc_val.wrapping_sub(*dc_pred));
        dc.emit_bits32();
        *dc_pred = dc_val;

        if m_index == 0 {
            *num_zeros += 64;
            return;
        }

        let mut coded = [0u32; 64];
        for i in 0..64 {
            coded[i] = (get_2mag_sign(temp_block[i]) as u32).wrapping_add(1);
        }

        let mut m = m_index;
        let nz = m.trailing_zeros() as usize;
        *num_zeros += nz as u32;
        ac.encode_zeros(num_zeros);
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
        *num_zeros = (64 - pos) as u32;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi2")]
unsafe fn decode_plane_avx2(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    use crate::codec::dct::broadcast_dc;
    use crate::simd::sse128::zig_invquant_idct_sse;

    // SAFETY: caller verified AVX2+BMI2.
    unsafe {
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
        let mut temp_block2 = [0i16; 64];

        for y in (0..height).step_by(8) {
            let mut x = 0usize;
            while x < stride {
                let dst_off = base + y * stride + x;
                let dual = x + 16 <= stride && dst_off + 7 * stride + 16 <= plane.data.len();

                if dual {
                    let valid0 = decode_entropy_block_avx2(
                        temp_block,
                        dc,
                        ac,
                        &mut dc_pred,
                        &mut terms_to_decode,
                        dc_shift,
                    );
                    let valid1 = decode_entropy_block_avx2(
                        &mut temp_block2,
                        dc,
                        ac,
                        &mut dc_pred,
                        &mut terms_to_decode,
                        dc_shift,
                    );

                    if valid0 {
                        zig_invquant_idct_sse(
                            temp_block,
                            decode_matrix,
                            plane.data.as_mut_ptr().add(dst_off),
                            stride,
                            add_val,
                        );
                    } else {
                        broadcast_dc(
                            temp_block[0],
                            &mut plane.data[dst_off..],
                            stride,
                            add_val,
                        );
                    }

                    if valid1 {
                        zig_invquant_idct_sse(
                            &mut temp_block2,
                            decode_matrix,
                            plane.data.as_mut_ptr().add(dst_off + 8),
                            stride,
                            add_val,
                        );
                    } else {
                        broadcast_dc(
                            temp_block2[0],
                            &mut plane.data[dst_off + 8..],
                            stride,
                            add_val,
                        );
                    }

                    x += 16;
                } else {
                    if dst_off + 7 * stride + 8 > plane.data.len() {
                        x += 8;
                        continue;
                    }

                    let valid = decode_entropy_block_avx2(
                        temp_block,
                        dc,
                        ac,
                        &mut dc_pred,
                        &mut terms_to_decode,
                        dc_shift,
                    );

                    if valid {
                        zig_invquant_idct_sse(
                            temp_block,
                            decode_matrix,
                            plane.data.as_mut_ptr().add(dst_off),
                            stride,
                            add_val,
                        );
                    } else {
                        broadcast_dc(
                            temp_block[0],
                            &mut plane.data[dst_off..],
                            stride,
                            add_val,
                        );
                    }
                    x += 8;
                }
            }
        }

        ac.rewind_overread(terms_to_decode);
        dc.flush_remaining_read_bits();
        ac.flush_remaining_read_bits();
    }
}

/// Decode AC/DC entropy for one 8×8 block into `temp_block`.
///
/// Returns `terms_to_decode < 64` before this block (i.e. slice still valid).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi2")]
unsafe fn decode_entropy_block_avx2(
    temp_block: &mut [i16; 64],
    dc: &mut SliceData,
    ac: &mut SliceData,
    dc_pred: &mut i16,
    terms_to_decode: &mut u64,
    dc_shift: i32,
) -> bool {
    use crate::bitstream::get_int_from_2mag_sign;

    // SAFETY: mirrors `decode_plane_sse` / scalar decode.
    temp_block.fill(0);
    let valid = *terms_to_decode < 64;

    while *terms_to_decode < 64 {
        let l = ac.peek_golomb_lookup();
        if l.length != 0 {
            ac.bits_left -= l.length as i32;
            temp_block[*terms_to_decode as usize] = l.value as i16;
            *terms_to_decode += l.zeros as u64;
        } else {
            let b = ac.get_bit_b();
            if b != 0 {
                let b2 = ac.get_bit_b();
                if b2 != 0 {
                    *terms_to_decode += 1;
                } else {
                    let mut bc = ac.get_zeros_b();
                    bc += 2;
                    let val = ac.get_bits_b(bc as u32);
                    *terms_to_decode += val;
                }
            } else {
                let mut bc = ac.get_zeros_b();
                bc += 2;
                let val = ac.get_bits_b(bc as u32);
                temp_block[*terms_to_decode as usize] =
                    get_int_from_2mag_sign(val.wrapping_sub(1));
                *terms_to_decode += 1;
            }
        }
        ac.reload_bits();
    }
    *terms_to_decode -= 64;

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
    temp_block[0] = temp_block[0].wrapping_add(*dc_pred);
    *dc_pred = temp_block[0];

    valid
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::ac_nonzero_mask_avx2;

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
    fn ac_nonzero_mask_avx2_matches_scalar() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("bmi2") {
            return;
        }

        let mut coeffs = [0i16; 64];
        for (i, c) in coeffs.iter_mut().enumerate() {
            *c = if (i * 17 + 3) % 11 == 0 { 0 } else { ((i as i16) - 32) * 3 };
        }

        let expected = ac_nonzero_mask_scalar(&coeffs);
        // SAFETY: test verified AVX2+BMI2.
        let actual = unsafe { ac_nonzero_mask_avx2(&coeffs) };
        assert_eq!(actual, expected);

        // All-zero AC
        coeffs.fill(0);
        coeffs[0] = 42;
        assert_eq!(
            unsafe { ac_nonzero_mask_avx2(&coeffs) },
            ac_nonzero_mask_scalar(&coeffs)
        );
    }

    // Bitstream identity vs scalar is covered by `simd::path_tests::avx2_encode_matches_scalar_when_available`.
}
