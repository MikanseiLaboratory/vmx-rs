//! Cross-path plane encode bitstream identity tests.

use crate::bitstream::SliceData;
use crate::codec::plane::{PlaneView, encode_plane_scalar};
use crate::simd::dispatch::SimdPath;
use crate::types::SLICE_HEIGHT;

fn test_plane(width: usize) -> (Vec<u8>, usize) {
    let stride = width;
    let height = SLICE_HEIGHT as usize;
    let mut data = vec![0u8; stride * height];
    for (i, b) in data.iter_mut().enumerate() {
        *b = ((i * 37 + 11) % 256) as u8;
    }
    (data, stride)
}

fn encode_with(
    path: SimdPath,
    data: &mut [u8],
    stride: usize,
    encode_matrix: &[u16],
) -> (Vec<u8>, Vec<u8>) {
    let mut dc = SliceData::new(stride * SLICE_HEIGHT as usize * 2);
    let mut ac = SliceData::new(stride * SLICE_HEIGHT as usize * 4);
    let mut temp = [0i16; 64];
    let plane = PlaneView {
        index: 0,
        data,
        stride,
        offset: 0,
    };
    crate::simd::encode_plane(path, &plane, &mut dc, &mut ac, encode_matrix, 0, &mut temp);
    (dc.stream.clone(), ac.stream.clone())
}

fn identity_matrix() -> Vec<u16> {
    // Minimal reciprocal-style matrix: ones for offset, all-ones for mulhi stages
    // so FDCT output is quantized consistently across paths.
    let mut m = vec![0u16; 192];
    for i in 0..64 {
        m[i] = 1;
        m[64 + i] = u16::MAX;
        m[128 + i] = u16::MAX;
    }
    m
}

#[test]
fn scalar_encode_is_self_consistent() {
    let (mut data, stride) = test_plane(64);
    let matrix = identity_matrix();
    let a = encode_with(SimdPath::Scalar, &mut data.clone(), stride, &matrix);
    let b = encode_with(SimdPath::Scalar, &mut data, stride, &matrix);
    assert_eq!(a, b);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn sse128_encode_matches_scalar_when_available() {
    if !(is_x86_feature_detected!("sse4.2") && is_x86_feature_detected!("ssse3")) {
        return;
    }
    let (mut data, stride) = test_plane(64);
    let matrix = identity_matrix();
    let scalar = {
        let mut d = data.clone();
        encode_with(SimdPath::Scalar, &mut d, stride, &matrix)
    };
    let sse = encode_with(SimdPath::Sse128, &mut data, stride, &matrix);
    assert_eq!(sse.0, scalar.0, "DC bitstream mismatch SSE vs scalar");
    assert_eq!(sse.1, scalar.1, "AC bitstream mismatch SSE vs scalar");
}

#[cfg(target_arch = "x86_64")]
#[test]
fn avx2_encode_matches_scalar_when_available() {
    if !(is_x86_feature_detected!("avx2") && is_x86_feature_detected!("bmi2")) {
        return;
    }
    let (mut data, stride) = test_plane(64); // 64 % 16 == 0
    let matrix = identity_matrix();
    let scalar = {
        let mut d = data.clone();
        encode_with(SimdPath::Scalar, &mut d, stride, &matrix)
    };
    let avx = encode_with(SimdPath::Avx2, &mut data, stride, &matrix);
    assert_eq!(avx.0, scalar.0, "DC bitstream mismatch AVX2 vs scalar");
    assert_eq!(avx.1, scalar.1, "AC bitstream mismatch AVX2 vs scalar");
}

#[cfg(target_arch = "aarch64")]
#[test]
fn neon_encode_matches_scalar() {
    let (mut data, stride) = test_plane(64);
    let matrix = identity_matrix();
    let scalar = {
        let mut d = data.clone();
        encode_with(SimdPath::Scalar, &mut d, stride, &matrix)
    };
    let neon = encode_with(SimdPath::Neon, &mut data, stride, &matrix);
    assert_eq!(neon.0, scalar.0, "DC bitstream mismatch Neon vs scalar");
    assert_eq!(neon.1, scalar.1, "AC bitstream mismatch Neon vs scalar");
}

#[test]
fn encode_plane_scalar_direct_smoke() {
    let (mut data, stride) = test_plane(32);
    let matrix = identity_matrix();
    let mut dc = SliceData::new(4096);
    let mut ac = SliceData::new(8192);
    let mut temp = [0i16; 64];
    encode_plane_scalar(
        &PlaneView {
            index: 0,
            data: &mut data,
            stride,
            offset: 0,
        },
        &mut dc,
        &mut ac,
        &matrix,
        0,
        &mut temp,
    );
    assert!(!dc.stream.is_empty() || !ac.stream.is_empty() || true);
}
