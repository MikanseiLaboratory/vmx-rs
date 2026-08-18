//! Plane encode/decode entry points that honor a pre-selected [`SimdPath`].
//!
//! Callers (slice parallel loops) must pass the path stored on the [`crate::Codec`]
//! instance — they must not re-detect CPU features on the hot path.

use crate::bitstream::SliceData;
use crate::codec::plane::PlaneView;
use crate::simd::dispatch::SimdPath;
use crate::simd::{avx2, neon, scalar, sse128};

/// Encode one plane band using the codec's selected SIMD path.
#[inline]
pub fn encode_plane(
    path: SimdPath,
    plane: &PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    encode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    match path {
        SimdPath::Avx2 => avx2::encode_plane(plane, dc, ac, encode_matrix, dc_shift, temp_block),
        SimdPath::Sse128 => {
            sse128::encode_plane(plane, dc, ac, encode_matrix, dc_shift, temp_block)
        }
        SimdPath::Neon => neon::encode_plane(plane, dc, ac, encode_matrix, dc_shift, temp_block),
        #[cfg(feature = "portable-simd")]
        SimdPath::Portable => {
            crate::simd::portable::encode_plane(plane, dc, ac, encode_matrix, dc_shift, temp_block)
        }
        SimdPath::Scalar => {
            scalar::encode_plane_scalar(plane, dc, ac, encode_matrix, dc_shift, temp_block)
        }
    }
}

/// Decode one plane band using the codec's selected SIMD path.
#[inline]
pub fn decode_plane(
    path: SimdPath,
    plane: &mut PlaneView<'_>,
    dc: &mut SliceData,
    ac: &mut SliceData,
    decode_matrix: &[u16],
    dc_shift: i32,
    temp_block: &mut [i16; 64],
) {
    match path {
        SimdPath::Avx2 => avx2::decode_plane(plane, dc, ac, decode_matrix, dc_shift, temp_block),
        SimdPath::Sse128 => {
            sse128::decode_plane(plane, dc, ac, decode_matrix, dc_shift, temp_block)
        }
        SimdPath::Neon => neon::decode_plane(plane, dc, ac, decode_matrix, dc_shift, temp_block),
        #[cfg(feature = "portable-simd")]
        SimdPath::Portable => {
            crate::simd::portable::decode_plane(plane, dc, ac, decode_matrix, dc_shift, temp_block)
        }
        SimdPath::Scalar => {
            scalar::decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block)
        }
    }
}
