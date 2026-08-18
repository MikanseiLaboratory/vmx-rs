//! AVX-512F/BW plane path (x86_64): four adjacent 8×8 blocks per call.
//!
//! Selected when `avx512f && avx512bw && bmi2` and chroma width % 32 == 0.
//! Does **not** call into the AVX2 plane path — edges use SSE/scalar kernels.

#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::bitstream::SliceData;
use crate::codec::plane::PlaneView;

#[cfg(target_arch = "x86_64")]
use crate::bitstream::{get_2mag_sign, get_int_from_2mag_sign};
#[cfg(target_arch = "x86_64")]
use crate::types::SLICE_HEIGHT;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Encode using AVX-512 when available (else SSE128 / scalar — never AVX2).
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
        if is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("bmi2")
        {
            return unsafe {
                encode_plane_avx512(plane, dc, ac, encode_matrix, dc_shift, temp_block)
            };
        }
        // Prefer SSE over AVX2 when this module was selected but CPU lost AVX-512 mid-flight.
        return crate::simd::sse128::encode_plane(
            plane,
            dc,
            ac,
            encode_matrix,
            dc_shift,
            temp_block,
        );
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        crate::codec::plane::encode_plane_scalar(
            plane,
            dc,
            ac,
            encode_matrix,
            dc_shift,
            temp_block,
        );
    }
}

/// Decode using AVX-512 when available (else SSE128 / scalar — never AVX2).
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
        if is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("bmi2")
        {
            return unsafe {
                decode_plane_avx512(plane, dc, ac, decode_matrix, dc_shift, temp_block)
            };
        }
        return crate::simd::sse128::decode_plane(
            plane,
            dc,
            ac,
            decode_matrix,
            dc_shift,
            temp_block,
        );
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        crate::codec::plane::decode_plane_scalar(
            plane,
            dc,
            ac,
            decode_matrix,
            dc_shift,
            temp_block,
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn idct_columns_8_avx512(rows: [__m512i; 8], add_val: i16) -> [__m512i; 8] {
    use crate::tables::{
        IDCT_COS4, IDCT_TG1, IDCT_TG2, IDCT_TG3, IRND_INV_COL, IRND_INV_CORR, SHIFT_INV_COL,
    };

    let (r0, r1, r2, r3, r4, r5, r6, r7) = (
        rows[0], rows[1], rows[2], rows[3], rows[4], rows[5], rows[6], rows[7],
    );

    let tg1 = _mm512_set1_epi16(IDCT_TG1);
    let tg2 = _mm512_set1_epi16(IDCT_TG2);
    let tg3 = _mm512_set1_epi16(IDCT_TG3);
    let cos4 = _mm512_set1_epi16(IDCT_COS4);
    let one = _mm512_set1_epi16(1);

    let mut x0 = _mm512_adds_epi16(_mm512_mulhi_epi16(r5, tg3), r5);
    let x1 = _mm512_adds_epi16(_mm512_mulhi_epi16(r3, tg3), r3);
    x0 = _mm512_adds_epi16(x0, r3);
    let x2 = _mm512_subs_epi16(r5, x1);
    let x5 = _mm512_subs_epi16(_mm512_mulhi_epi16(r1, tg1), r7);
    let x4 = _mm512_adds_epi16(_mm512_mulhi_epi16(r7, tg1), r1);

    let temp7 = _mm512_adds_epi16(_mm512_adds_epi16(x0, x4), one);
    let t4 = _mm512_subs_epi16(x4, x0);
    let t5 = _mm512_adds_epi16(_mm512_subs_epi16(x5, x2), one);
    let temp3 = _mm512_adds_epi16(x5, x2);

    let s = _mm512_adds_epi16(t4, t5);
    let d = _mm512_subs_epi16(t4, t5);
    let m4 = _mm512_or_si512(_mm512_adds_epi16(s, _mm512_mulhi_epi16(cos4, s)), one);
    let m0 = _mm512_or_si512(_mm512_adds_epi16(_mm512_mulhi_epi16(cos4, d), d), one);

    let e7 = _mm512_adds_epi16(_mm512_mulhi_epi16(r6, tg2), r2);
    let e3 = _mm512_subs_epi16(_mm512_mulhi_epi16(r2, tg2), r6);
    let sum04 = _mm512_adds_epi16(r4, r0);
    let dif04 = _mm512_subs_epi16(r0, r4);

    let rnd_col = _mm512_set1_epi16(IRND_INV_COL as i16);
    let rnd_corr = _mm512_set1_epi16(IRND_INV_CORR as i16);
    let b0 = _mm512_adds_epi16(_mm512_adds_epi16(sum04, e7), rnd_col);
    let b3 = _mm512_adds_epi16(_mm512_subs_epi16(sum04, e7), rnd_corr);
    let b1 = _mm512_adds_epi16(_mm512_adds_epi16(dif04, e3), rnd_col);
    let b2 = _mm512_adds_epi16(_mm512_subs_epi16(dif04, e3), rnd_corr);

    let add = _mm512_set1_epi16(add_val);
    let fin = |v: __m512i| _mm512_adds_epi16(_mm512_srai_epi16(v, SHIFT_INV_COL as u32), add);
    [
        fin(_mm512_adds_epi16(temp7, b0)),
        fin(_mm512_adds_epi16(b1, m4)),
        fin(_mm512_adds_epi16(b2, m0)),
        fin(_mm512_adds_epi16(temp3, b3)),
        fin(_mm512_subs_epi16(b3, temp3)),
        fin(_mm512_subs_epi16(b2, m0)),
        fin(_mm512_subs_epi16(b1, m4)),
        fin(_mm512_subs_epi16(b0, temp7)),
    ]
}

/// Dezigzag + dequant + IDCT for four adjacent 8×8 blocks (32 px wide).
///
/// # Safety
/// AVX-512F+BW required. `dst` covers 8 rows × 32 bytes at `stride`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn zig_invquant_idct_avx512_x4(
    blocks: &mut [[i16; 64]; 4],
    decode_matrix: &[u16],
    dst: *mut u8,
    stride: usize,
    add_val: i16,
) {
    use crate::codec::dct::idct_row;
    use crate::tables::{IDCT_ROW_TABLES, ZIGZAG_INV};

    unsafe {
        let mut rows = [_mm512_setzero_si512(); 8];
        for row in 0..8 {
            let mut lane = [0i16; 32];
            for b in 0..4 {
                for i in 0..8 {
                    let idx = row * 8 + i;
                    let c = blocks[b][ZIGZAG_INV[idx] as usize];
                    lane[b * 8 + i] = c.wrapping_mul(decode_matrix[idx] as i16) >> 4;
                }
            }
            for b in 0..4 {
                let mut x = [0i16; 8];
                x.copy_from_slice(&lane[b * 8..b * 8 + 8]);
                let y = idct_row(x, IDCT_ROW_TABLES[row]);
                lane[b * 8..b * 8 + 8].copy_from_slice(&y);
            }
            rows[row] = _mm512_loadu_si512(lane.as_ptr().cast());
        }

        let out = idct_columns_8_avx512(rows, add_val);
        for (y, row) in out.iter().enumerate() {
            let zero = _mm512_setzero_si512();
            let packed = _mm512_packus_epi16(*row, zero);
            let mut tmp = [0u8; 64];
            _mm512_storeu_si512(tmp.as_mut_ptr().cast(), packed);
            let mut out32 = [0u8; 32];
            for lane in 0..4 {
                out32[lane * 8..lane * 8 + 8].copy_from_slice(&tmp[lane * 16..lane * 16 + 8]);
            }
            std::ptr::copy_nonoverlapping(out32.as_ptr(), dst.add(y * stride), 32);
        }
    }
}

/// Quad-block FDCT+quant+zigzag using AVX-512 loads and SSE4.2 row kernels (not AVX2).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,sse4.2")]
unsafe fn fdct_quant_zig_avx512_x4(
    src: *const u8,
    stride: usize,
    encode_matrix: *const u16,
    add_val: i16,
    outs: &mut [[i16; 64]; 4],
) {
    use crate::simd::sse128::fdct_quant_zig_sse;

    // Four independent SSE FDCT calls on adjacent 8×8 tiles. This is intentional:
    // the AVX-512 path must not call AVX2 dual-block kernels.
    unsafe {
        for b in 0..4 {
            fdct_quant_zig_sse(
                src.add(b * 8),
                stride,
                encode_matrix,
                add_val,
                &mut outs[b],
            );
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn ac_nonzero_mask(coeffs: &[i16; 64]) -> u64 {
    let mut mask = 0u64;
    for (i, coeff) in coeffs.iter().enumerate().skip(1) {
        if *coeff != 0 {
            mask |= 1u64 << i;
        }
    }
    mask
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,bmi2,sse4.2")]
unsafe fn encode_fdct_block(
    temp_block: &[i16; 64],
    dc: &mut SliceData,
    ac: &mut SliceData,
    dc_shift: i32,
    dc_round: i16,
    dc_pred: &mut i16,
    num_zeros: &mut u32,
) {
    let dc_val = temp_block[0].wrapping_add(dc_round) >> dc_shift;
    let m_index = ac_nonzero_mask(temp_block);

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

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,bmi2,sse4.2")]
unsafe fn encode_plane_avx512(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    use crate::simd::sse128::fdct_quant_zig_sse;

    unsafe {
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
        let stride = plane.stride;
        let base = plane.offset;
        let mut blocks = [[0i16; 64]; 4];

        for y in (0..height).step_by(8) {
            let mut x = 0usize;
            while x < stride {
                let src_off = base + y * stride + x;
                let quad = x + 32 <= stride && src_off + 7 * stride + 32 <= plane.data.len();
                if quad {
                    fdct_quant_zig_avx512_x4(
                        plane.data.as_ptr().add(src_off),
                        stride,
                        encode_matrix.as_ptr(),
                        add_val,
                        &mut blocks,
                    );
                    for b in 0..4 {
                        encode_fdct_block(
                            &blocks[b],
                            dc,
                            ac,
                            dc_shift,
                            dc_round,
                            &mut dc_pred,
                            &mut num_zeros,
                        );
                    }
                    x += 32;
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
                    encode_fdct_block(
                        temp_block,
                        dc,
                        ac,
                        dc_shift,
                        dc_round,
                        &mut dc_pred,
                        &mut num_zeros,
                    );
                    x += 8;
                }
            }
        }
        ac.encode_zeros(&mut num_zeros);
        ac.emit_bits32();
        ac.flush_remaining_bits();
        dc.flush_remaining_bits();
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn decode_entropy_block(
    temp_block: &mut [i16; 64],
    dc: &mut SliceData,
    ac: &mut SliceData,
    dc_pred: &mut i16,
    terms_to_decode: &mut u64,
    dc_shift: i32,
) -> bool {
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
                temp_block[*terms_to_decode as usize] = get_int_from_2mag_sign(val.wrapping_sub(1));
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

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,bmi2,sse4.1,ssse3")]
unsafe fn decode_plane_avx512(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    use crate::codec::dct::broadcast_dc;
    use crate::simd::sse128::zig_invquant_idct_sse;

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
        let mut blocks = [[0i16; 64]; 4];

        for y in (0..height).step_by(8) {
            let mut x = 0usize;
            while x < stride {
                let dst_off = base + y * stride + x;
                let quad = x + 32 <= stride && dst_off + 7 * stride + 32 <= plane.data.len();

                if quad {
                    let mut valids = [false; 4];
                    for b in 0..4 {
                        valids[b] = decode_entropy_block(
                            &mut blocks[b],
                            dc,
                            ac,
                            &mut dc_pred,
                            &mut terms_to_decode,
                            dc_shift,
                        );
                    }
                    if valids.iter().all(|&v| v) {
                        zig_invquant_idct_avx512_x4(
                            &mut blocks,
                            decode_matrix,
                            plane.data.as_mut_ptr().add(dst_off),
                            stride,
                            add_val,
                        );
                    } else {
                        for b in 0..4 {
                            let off = dst_off + b * 8;
                            if valids[b] {
                                zig_invquant_idct_sse(
                                    &mut blocks[b],
                                    decode_matrix,
                                    plane.data.as_mut_ptr().add(off),
                                    stride,
                                    add_val,
                                );
                            } else {
                                broadcast_dc(
                                    blocks[b][0],
                                    &mut plane.data[off..],
                                    stride,
                                    add_val,
                                );
                            }
                        }
                    }
                    x += 32;
                } else {
                    let valid = decode_entropy_block(
                        temp_block,
                        dc,
                        ac,
                        &mut dc_pred,
                        &mut terms_to_decode,
                        dc_shift,
                    );
                    if dst_off + 7 * stride + 8 <= plane.data.len() {
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
                    }
                    x += 8;
                }
            }
        }

        // Match SSE/AVX2: undo AC over-read and byte-align both streams.
        ac.rewind_overread(terms_to_decode);
        dc.flush_remaining_read_bits();
        ac.flush_remaining_read_bits();
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::codec::dct::zig_invquant_idct;

    #[test]
    fn avx512_idct_x4_matches_scalar() {
        if !(is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw")) {
            return;
        }
        let mut blocks = [[0i16; 64]; 4];
        for b in 0..4 {
            for (i, c) in blocks[b].iter_mut().enumerate() {
                *c = ((i as i16 + b as i16 * 3) % 11) - 5;
            }
        }
        let mut matrix = [0u16; 64];
        for (i, m) in matrix.iter_mut().enumerate() {
            *m = ((i as u16) % 13) + 1;
        }
        let mut dst_v = [0u8; 8 * 64];
        let mut bcopy = blocks;
        unsafe {
            zig_invquant_idct_avx512_x4(&mut bcopy, &matrix, dst_v.as_mut_ptr(), 64, 128);
        }
        for b in 0..4 {
            let mut c = blocks[b];
            let mut tile = [0u8; 8 * 64];
            zig_invquant_idct(&mut c, &matrix, &mut tile, 64, 128);
            for y in 0..8 {
                assert_eq!(
                    &dst_v[y * 64 + b * 8..y * 64 + b * 8 + 8],
                    &tile[y * 64..y * 64 + 8],
                    "block {b} row {y}"
                );
            }
        }
    }

    #[test]
    fn avx512_idct_faster_than_scalar_in_release() {
        if cfg!(debug_assertions)
            || !(is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw"))
        {
            return;
        }
        use std::time::Instant;
        let mut blocks = [[0i16; 64]; 4];
        for b in 0..4 {
            for (i, c) in blocks[b].iter_mut().enumerate() {
                *c = ((i as i16) % 11) - 5;
            }
        }
        let mut matrix = [0u16; 64];
        for (i, m) in matrix.iter_mut().enumerate() {
            *m = ((i as u16) % 13) + 1;
        }
        let mut dst = [0u8; 8 * 64];
        let warmup = 80;
        let iters = 2000;
        for _ in 0..warmup {
            let mut b = blocks;
            for blk in &mut b {
                zig_invquant_idct(blk, &matrix, &mut dst, 64, 128);
            }
            let mut b = blocks;
            unsafe {
                zig_invquant_idct_avx512_x4(&mut b, &matrix, dst.as_mut_ptr(), 64, 128);
            }
        }
        let t0 = Instant::now();
        for _ in 0..iters {
            let mut b = blocks;
            for blk in &mut b {
                zig_invquant_idct(blk, &matrix, &mut dst, 64, 128);
            }
        }
        let scalar = t0.elapsed();
        let t0 = Instant::now();
        for _ in 0..iters {
            let mut b = blocks;
            unsafe {
                zig_invquant_idct_avx512_x4(&mut b, &matrix, dst.as_mut_ptr(), 64, 128);
            }
        }
        let avx512 = t0.elapsed();
        eprintln!(
            "idct 4×8x8 scalar={:.3}us avx512={:.3}us ({:.2}x)",
            scalar.as_secs_f64() * 1e6 / iters as f64,
            avx512.as_secs_f64() * 1e6 / iters as f64,
            scalar.as_secs_f64() / avx512.as_secs_f64().max(1e-12)
        );
        assert!(
            avx512 < scalar,
            "AVX-512 IDCT should beat 4× scalar (scalar={scalar:?} avx512={avx512:?})"
        );
    }

    #[test]
    fn avx512_plane_encode_matches_scalar_when_available() {
        if !(is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("bmi2")
            && is_x86_feature_detected!("sse4.2"))
        {
            return;
        }
        use crate::bitstream::SliceData;
        use crate::codec::plane::{PlaneView, encode_plane_scalar};
        use crate::types::SLICE_HEIGHT;

        let stride = 64usize;
        let height = SLICE_HEIGHT as usize;
        let mut data = vec![0u8; stride * height];
        for (i, b) in data.iter_mut().enumerate() {
            *b = ((i * 37 + 11) % 220) as u8 + 16;
        }
        let mut matrix = vec![0u16; 192];
        for i in 0..64 {
            matrix[i] = (i as u16 % 5) + 1;
            matrix[i + 64] = 0x8000 + (i as u16 * 17);
            matrix[i + 128] = 0x4000 + (i as u16 * 3);
        }

        let encode = |path_avx512: bool| {
            let mut d = data.clone();
            let mut dc = SliceData::new(stride * height * 2);
            let mut ac = SliceData::new(stride * height * 4);
            let mut temp = [0i16; 64];
            let plane = PlaneView {
                index: 0,
                data: &mut d,
                stride,
                offset: 0,
            };
            if path_avx512 {
                encode_plane(&plane, &mut dc, &mut ac, &matrix, 0, &mut temp);
            } else {
                encode_plane_scalar(&plane, &mut dc, &mut ac, &matrix, 0, &mut temp);
            }
            (dc.stream, ac.stream)
        };

        let scalar = encode(false);
        let avx = encode(true);
        assert_eq!(avx.0, scalar.0, "DC bitstream mismatch AVX-512 vs scalar");
        assert_eq!(avx.1, scalar.1, "AC bitstream mismatch AVX-512 vs scalar");
    }
}
