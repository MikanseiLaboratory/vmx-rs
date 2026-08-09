//! Preview (DC-only) decode helpers.

use crate::bitstream::SliceData;
use crate::codec::dct::broadcast_dc;
use crate::codec::plane::PlaneView;
use crate::types::SLICE_HEIGHT;

pub fn decode_plane_preview(plane: &mut PlaneView<'_>, dc: &mut SliceData, dc_shift: i32) {
    let height = SLICE_HEIGHT as usize;
    let add_val: i16 = if plane.index == 0 || plane.index == 3 {
        128
    } else {
        0
    };
    let mut dc_pred: i16 = 0;
    let stride = plane.stride;
    let base = plane.offset;

    // Preview writes one pixel per 8x8 block into a downscaled plane.
    // For API simplicity we still fill 8x8 with DC broadcast in full plane,
    // and callers subsample via PreviewSize.
    for y in (0..height).step_by(8) {
        for x in (0..stride).step_by(8) {
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
            let dst_off = base + y * stride + x;
            if dst_off + 7 * stride + 8 <= plane.data.len() {
                broadcast_dc(dc_val, &mut plane.data[dst_off..], stride, add_val);
            }
        }
    }
    dc.flush_remaining_read_bits();
}
