//! ARM64 NEON path — native NEON (not sse2neon). Currently scalar fallback.

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::{PlaneView, decode_plane_scalar, encode_plane_scalar};

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
        // NEON is baseline on aarch64.
        encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block);
    }
    #[cfg(not(target_arch = "aarch64"))]
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
    #[cfg(target_arch = "aarch64")]
    {
        decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
    }
    #[cfg(not(target_arch = "aarch64"))]
    decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
}
