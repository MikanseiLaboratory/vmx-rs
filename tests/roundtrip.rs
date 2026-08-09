use vmx::{Codec, Config, Profile};

#[test]
fn uyvy_encode_decode_smoke() {
    let mut enc = Codec::new(Config {
        width: 64,
        height: 64,
        profile: Profile::Hq,
        color_space: Default::default(),
    })
    .expect("create encoder");

    let stride = 64 * 2;
    let mut frame = vec![128u8; stride * 64];
    for y in 0..64 {
        for x in 0..64 {
            let o = y * stride + x * 2;
            if x % 2 == 0 {
                frame[o] = 128; // U
                frame[o + 1] = ((x + y) % 256) as u8; // Y
            } else {
                frame[o] = 128; // V written on odd in UYVY pairs — adjust
            }
        }
    }
    // Proper UYVY pattern
    for y in 0..64 {
        for x in (0..64).step_by(2) {
            let o = y * stride + x * 2;
            frame[o] = 128;
            frame[o + 1] = ((x + y) % 220 + 16) as u8;
            frame[o + 2] = 128;
            frame[o + 3] = ((x + 1 + y) % 220 + 16) as u8;
        }
    }

    enc.encode_uyvy(&frame, stride).expect("encode");
    let mut bitstream = vec![0u8; 2 << 20];
    let len = enc.save_to(&mut bitstream).expect("save");
    assert!(len > 3, "bitstream too short: {len}");

    let mut dec = Codec::new(Config::new(64, 64)).expect("create decoder");
    dec.load_from(&bitstream[..len]).expect("load");
    let mut out = vec![0u8; stride * 64];
    dec.decode_uyvy(&mut out, stride).expect("decode");

    // Ensure we produced something non-trivial
    let mean: f32 = out.iter().map(|&b| b as f32).sum::<f32>() / out.len() as f32;
    assert!(mean > 1.0, "decoded frame looks empty (mean={mean})");
}

#[test]
fn mag_sign_in_bitstream_module() {
    // Covered by bitstream unit tests; keep integration crate link.
    let _ = Codec::new(Config::new(32, 32));
}
