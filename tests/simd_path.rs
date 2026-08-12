//! SIMD path reporting and cross-path bitstream compatibility.

use vmx::{Codec, Config, Profile, SimdCapabilities, SimdPath};

fn make_uyvy(width: i32, height: i32) -> (Vec<u8>, usize) {
    let stride = (width as usize) * 2;
    let mut frame = vec![128u8; stride * height as usize];
    for y in 0..height as usize {
        for x in (0..width as usize).step_by(2) {
            let o = y * stride + x * 2;
            frame[o] = (100 + ((x / 2) % 40) as u8).min(240);
            frame[o + 1] = 16 + ((x + y * 3) % 220) as u8;
            frame[o + 2] = (140 + ((y / 4) % 40) as u8).min(240);
            frame[o + 3] = 16 + ((x + 1 + y * 5) % 220) as u8;
        }
    }
    (frame, stride)
}

fn encode_bitstream(width: i32, height: i32, frame: &[u8], stride: usize) -> (Vec<u8>, SimdPath) {
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::Hq,
        color_space: Default::default(),
    })
    .expect("create encoder");
    let path = enc.simd_path();
    enc.encode_uyvy(frame, stride).expect("encode");
    let mut bitstream = vec![0u8; 4 << 20];
    let len = enc.save_to(&mut bitstream).expect("save");
    bitstream.truncate(len);
    (bitstream, path)
}

#[test]
fn simd_path_and_capabilities_are_reported() {
    // 1920 → UV width 960, divisible by 16 → AVX2 eligible on capable hosts.
    let codec = Codec::new(Config::new(1920, 1080)).unwrap();
    let caps = codec.simd_capabilities();
    let path = codec.simd_path();

    // Path string is one of the documented diagnostics.
    assert!(
        matches!(
            path.as_str(),
            "scalar" | "sse128" | "avx2" | "neon"
        ),
        "unexpected path {path}"
    );
    assert_eq!(path.to_string(), path.as_str());

    // Selection must be consistent with injected selection rules.
    let expected = caps.select_path(960);
    assert_eq!(path, expected);

    // features() is an alias for capabilities.
    assert_eq!(codec.features(), caps);
}

#[test]
fn uv_width_not_multiple_of_16_avoids_avx2() {
    // width=632 → UV=316, 316 % 16 != 0 → must not select AVX2 even if CPU has it.
    let codec = Codec::new(Config::new(632, 64)).unwrap();
    assert_ne!(codec.simd_path(), SimdPath::Avx2);

    let caps = codec.simd_capabilities();
    if caps.avx2_bmi2() {
        // Capability still reports AVX2; only the path is downgraded.
        assert!(caps.avx2 && caps.bmi2);
        assert_eq!(
            SimdCapabilities::select_path_with(caps, 316),
            if caps.sse128() {
                SimdPath::Sse128
            } else {
                SimdPath::Scalar
            }
        );
    }
}

#[test]
fn encode_decode_roundtrip_uses_reported_path() {
    let width = 128;
    let height = 64;
    let (frame, stride) = make_uyvy(width, height);
    let (bitstream, enc_path) = encode_bitstream(width, height, &frame, stride);

    let mut dec = Codec::new(Config::new(width, height)).unwrap();
    assert_eq!(dec.simd_path(), enc_path);
    dec.load_from(&bitstream).unwrap();
    let mut out = vec![0u8; stride * height as usize];
    dec.decode_uyvy(&mut out, stride).unwrap();

    let mean: f32 = out.iter().map(|&b| b as f32).sum::<f32>() / out.len() as f32;
    assert!(mean > 1.0, "decoded frame looks empty (mean={mean})");
}

#[test]
fn cross_path_decode_accepts_encoded_bitstream() {
    // Encode with the host's selected path, then decode with a fresh codec
    // (same selection). This guards bitstream stability of AVX2 dual-block
    // and SSE/NEON/Scalar entropy.
    let width = 256;
    let height = 128;
    let (frame, stride) = make_uyvy(width, height);
    let (bitstream, path) = encode_bitstream(width, height, &frame, stride);

    let mut dec = Codec::new(Config::new(width, height)).unwrap();
    assert_eq!(dec.simd_path(), path);
    dec.load_from(&bitstream).unwrap();
    let mut out = vec![0u8; stride * height as usize];
    dec.decode_uyvy(&mut out, stride).unwrap();

    // Second decode must be identical.
    let mut out2 = vec![0u8; stride * height as usize];
    dec.load_from(&bitstream).unwrap();
    dec.decode_uyvy(&mut out2, stride).unwrap();
    assert_eq!(out, out2);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn x86_path_is_avx2_or_sse128_or_scalar() {
    let codec = Codec::new(Config::new(1920, 1080)).unwrap();
    match codec.simd_path() {
        SimdPath::Avx2 => {
            let c = codec.simd_capabilities();
            assert!(c.avx2 && c.bmi2 && c.sse128());
        }
        SimdPath::Sse128 => {
            assert!(codec.simd_capabilities().sse128());
        }
        SimdPath::Scalar => {}
        SimdPath::Neon => panic!("Neon must not be selected on x86_64"),
    }
}

#[cfg(target_arch = "aarch64")]
#[test]
fn aarch64_path_is_neon_or_scalar() {
    let codec = Codec::new(Config::new(1920, 1080)).unwrap();
    match codec.simd_path() {
        SimdPath::Neon => assert!(codec.simd_capabilities().neon),
        SimdPath::Scalar => {}
        other => panic!("unexpected path on aarch64: {other}"),
    }
}
