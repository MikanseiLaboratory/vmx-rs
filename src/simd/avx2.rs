//! AVX2+BMI2 path — currently delegates to scalar with feature gating.
//!
//! Disabled automatically when chroma width % 16 != 0 (see Codec::new).

#![allow(dead_code)]

use crate::bitstream::SliceData;
use crate::codec::plane::{decode_plane_scalar, encode_plane_scalar, PlaneView};

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
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("bmi2") {
            return decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
        }
    }
    decode_plane_scalar(plane, dc, ac, decode_matrix, dc_shift, temp_block);
}
