//! AArch64 SVE / SVE2 plane path (nightly `sve` feature).
//!
//! 8×8 FDCT/IDCT stays on the NEON kernels: fixed 8×8 blocks map poorly to
//! scalable SVE length. Selecting [`crate::simd::SimdPath::Sve`] still prefers
//! an SVE-capable host (and pairs with the real SVE YUV→BGRA color path).
//! This module does **not** fall through to any x86 path.

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::PlaneView;

/// Encode one plane band. On AArch64 this uses the NEON 8×8 transform path.
pub fn encode_plane(
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    crate::simd::neon::encode_plane(plane, dc, ac, encode_matrix, dc_shift, temp_block)
}

/// Decode one plane band. On AArch64 this uses the NEON 8×8 transform path.
pub fn decode_plane(
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    crate::simd::neon::decode_plane(plane, dc, ac, decode_matrix, dc_shift, temp_block)
}
