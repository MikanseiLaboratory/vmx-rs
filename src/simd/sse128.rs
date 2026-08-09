//! SSE4.2 / SSSE3 path — currently delegates to scalar; intrinsics port in progress.
//!
//! Safety: all `std::arch::x86_64` usage must be gated by `is_x86_feature_detected!`
//! and only operate on buffers with verified lengths/alignment.

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::{decode_plane_scalar, encode_plane_scalar, PlaneView};

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
            // SAFETY: feature detected; scalar algorithm is bit-compatible stand-in until
            // full VMX_FDCT_8X8_QUANT_ZIG_128 intrinsics port is completed.
            return encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
        }
    }
    encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
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
