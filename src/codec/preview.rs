//! Preview (DC-only) decode helpers.

use crate::bitstream::SliceData;
use crate::codec::plane::PlaneView;
use crate::types::SLICE_HEIGHT;

/// Decode one plane's DC bitstream into the sparse 1/8 preview layout.
pub fn decode_plane_preview(plane: &mut PlaneView<'_>, dc: &mut SliceData, dc_shift: i32) {
    let add_val: i16 = if plane.index == 0 || plane.index == 3 {
        128
    } else {
        0
    };
    let mut dc_pred: i16 = 0;
    let stride = plane.stride;
    // One DC per 8×8 block across the full stride.
    let width = stride >> 3;
    let height = (SLICE_HEIGHT as usize) >> 3;
    // Preview rows start at offset/8.
    let mut p_dst = plane.offset >> 3;

    for _y in 0..height {
        for dst in p_dst..p_dst + width {
            let b = dc.get_bit();
            let mut dc_val = 0i16;
            if b != 0 {
                let _ = dc.get_bit();
            } else {
                let mut bc = dc.get_zeros();
                bc += 2;
                let val = dc.get_bits(bc as u32);
                dc_val = crate::bitstream::get_int_from_2mag_sign(val.wrapping_sub(1));
                dc_val <<= dc_shift;
            }
            dc_val = dc_val.wrapping_add(dc_pred);
            dc_pred = dc_val;

            let pix = (dc_val.wrapping_add(4) >> 3)
                .wrapping_add(add_val)
                .clamp(0, 255) as u8;
            if dst < plane.data.len() {
                plane.data[dst] = pix;
            }
        }
        p_dst += stride;
    }
    dc.flush_remaining_read_bits();
}
