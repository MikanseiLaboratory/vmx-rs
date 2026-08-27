//! Plane encode/decode orchestration (scalar path).

use crate::bitstream::{SliceData, get_2mag_sign, get_int_from_2mag_sign};
use crate::codec::dct::{broadcast_dc, fdct_quant_zig, zig_invquant_idct};
use crate::types::SLICE_HEIGHT;

pub struct PlaneView<'a> {
    pub index: usize,
    pub data: &'a mut [u8],
    pub stride: usize,
    pub offset: usize,
}

pub fn encode_plane_scalar(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    let height = SLICE_HEIGHT as usize;
    let mut dc_pred: i16 = 0;
    let mut num_zeros: u32 = 0;
    let add_val: i16 = if plane.index == 0 || plane.index == 3 {
        -128
    } else {
        0
    };
    let dc_round = if dc_shift > 0 {
        1i16 << (dc_shift - 1)
    } else {
        0
    };

    let base = plane.offset;
    let stride = plane.stride;

    for y in (0..height).step_by(8) {
        for x in (0..stride).step_by(8) {
            let src_off = base + y * stride + x;
            if src_off + 7 * stride + 8 > plane.data.len() {
                continue;
            }
            fdct_quant_zig(
                &plane.data[src_off..],
                stride,
                encode_matrix,
                add_val,
                temp_block,
            );

            let mut dc_val = temp_block[0];
            dc_val = dc_val.wrapping_add(dc_round) >> dc_shift;

            // Build nonzero mask (skip DC bit 0)
            let mut m_index: u64 = 0;
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

            // Convert AC to 2mag+1 in place for coding
            let mut coded = [0u32; 64];
            for i in 0..64 {
                coded[i] = (get_2mag_sign(temp_block[i]) as u32).wrapping_add(1);
            }

            let mut pos = 0usize;
            let end = 64usize;
            let mut m = m_index;
            if m != 0 {
                let nz = m.trailing_zeros() as u64;
                num_zeros += nz as u32;
                ac.encode_zeros(&mut num_zeros);
                ac.emit_bits32();
                // after trailing zeros, bit at position nz is set
                ac.encode_value(coded[nz as usize]);
                pos = nz as usize + 1;
                m >>= nz;
                m >>= 1;
                ac.emit_bits32();

                loop {
                    if m != 0 {
                        let nz = m.trailing_zeros() as u64;
                        ac.encode_zeros_small(nz);
                        ac.encode_value(coded[pos + nz as usize]);
                        pos += nz as usize + 1;
                        m >>= nz + 1;
                        ac.emit_bits32();
                    } else {
                        num_zeros = (end - pos) as u32;
                        break;
                    }
                }
            } else {
                num_zeros += (end - pos) as u32;
            }
        }
    }
    ac.encode_zeros(&mut num_zeros);
    ac.emit_bits32();
    ac.flush_remaining_bits();
    dc.flush_remaining_bits();
}

pub fn decode_plane_scalar(
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

            // DC
            let b = dc.get_bit();
            if b != 0 {
                let _b2 = dc.get_bit();
                // zero DC residual
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
                zig_invquant_idct(
                    temp_block,
                    decode_matrix,
                    &mut plane.data[dst_off..],
                    stride,
                    add_val,
                );
            } else {
                broadcast_dc(temp_block[0], &mut plane.data[dst_off..], stride, add_val);
            }
            let _ = valid;
        }
    }

    ac.rewind_overread(terms_to_decode);
    dc.flush_remaining_read_bits();
    ac.flush_remaining_read_bits();
}

/// Entropy-decode one slice plane into 32-byte GPU records (i16 DC + i8 AC[1..30]).
#[cfg(feature = "wgpu")]
#[inline(always)]
pub fn decode_plane_coeffs(
    plane_index: usize,
    stride: usize,
    dc: &mut SliceData,
    ac: &mut SliceData,
    dc_shift: i32,
    temp_block: &mut [i16; 64],
    out: &mut [u8],
) {
    let height = SLICE_HEIGHT as usize;
    let mut dc_pred: i16 = 0;
    let mut terms_to_decode: u64 = 0;
    let _ = plane_index;
    let mut bi = 0usize;

    for _y in (0..height).step_by(8) {
        for _x in (0..stride).step_by(8) {
            temp_block.fill(0);

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

            let o = bi * 32;
            out[o..o + 2].copy_from_slice(&temp_block[0].to_le_bytes());
            for i in 1..=30 {
                out[o + 1 + i] = temp_block[i].clamp(-128, 127) as i8 as u8;
            }
            bi += 1;
        }
    }

    ac.rewind_overread(terms_to_decode);
    dc.flush_remaining_read_bits();
    ac.flush_remaining_read_bits();
}

/// Golomb-encode zigzag quantized blocks (no FDCT). Same scan as [`encode_plane_scalar`].
#[cfg(feature = "wgpu")]
pub fn encode_plane_from_blocks(
    plane_index: usize,
    stride: usize,
    blocks: &[[i16; 64]],
    dc: &mut SliceData,
    ac: &mut SliceData,
    dc_shift: i32,
) {
    let height = SLICE_HEIGHT as usize;
    let mut dc_pred: i16 = 0;
    let mut num_zeros: u32 = 0;
    let dc_round = if dc_shift > 0 {
        1i16 << (dc_shift - 1)
    } else {
        0
    };
    let mut bi = 0usize;
    let _ = plane_index;

    for _y in (0..height).step_by(8) {
        for _x in (0..stride).step_by(8) {
            let zero = [0i16; 64];
            let temp_block = if bi < blocks.len() {
                &blocks[bi]
            } else {
                &zero
            };
            bi += 1;

            let mut dc_val = temp_block[0];
            dc_val = dc_val.wrapping_add(dc_round) >> dc_shift;

            let mut m_index: u64 = 0;
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

            let mut pos = 0usize;
            let end = 64usize;
            let mut m = m_index;
            if m != 0 {
                let nz = m.trailing_zeros() as u64;
                num_zeros += nz as u32;
                ac.encode_zeros(&mut num_zeros);
                ac.emit_bits32();
                ac.encode_value(coded[nz as usize]);
                pos = nz as usize + 1;
                m >>= nz;
                m >>= 1;
                ac.emit_bits32();

                loop {
                    if m != 0 {
                        let nz = m.trailing_zeros() as u64;
                        ac.encode_zeros_small(nz);
                        ac.encode_value(coded[pos + nz as usize]);
                        pos += nz as usize + 1;
                        m >>= nz + 1;
                        ac.emit_bits32();
                    } else {
                        num_zeros = (end - pos) as u32;
                        break;
                    }
                }
            } else {
                num_zeros += (end - pos) as u32;
            }
        }
    }
    ac.encode_zeros(&mut num_zeros);
    ac.emit_bits32();
    ac.flush_remaining_bits();
    dc.flush_remaining_bits();
}
