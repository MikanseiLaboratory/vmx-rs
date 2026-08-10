use vmx::{Codec, Config, Profile};

fn make_uyvy(width: i32, height: i32) -> (Vec<u8>, usize) {
    let stride = (width as usize) * 2;
    let mut frame = vec![128u8; stride * height as usize];
    for y in 0..height as usize {
        for x in (0..width as usize).step_by(2) {
            let o = y * stride + x * 2;
            frame[o] = (100 + ((x / 2) % 40) as u8).min(240);
            frame[o + 1] = (16 + ((x + y * 3) % 220) as u8);
            frame[o + 2] = (140 + ((y / 4) % 40) as u8).min(240);
            frame[o + 3] = (16 + ((x + 1 + y * 5) % 220) as u8);
        }
    }
    (frame, stride)
}

#[test]
fn uyvy_encode_decode_smoke() {
    let mut enc = Codec::new(Config {
        width: 64,
        height: 64,
        profile: Profile::Hq,
        color_space: Default::default(),
    })
    .expect("create encoder");

    let (frame, stride) = make_uyvy(64, 64);
    enc.encode_uyvy(&frame, stride).expect("encode");
    let mut bitstream = vec![0u8; 2 << 20];
    let len = enc.save_to(&mut bitstream).expect("save");
    assert!(len > 3, "bitstream too short: {len}");

    let mut dec = Codec::new(Config::new(64, 64)).expect("create decoder");
    dec.load_from(&bitstream[..len]).expect("load");
    let mut out = vec![0u8; stride * 64];
    dec.decode_uyvy(&mut out, stride).expect("decode");

    let mean: f32 = out.iter().map(|&b| b as f32).sum::<f32>() / out.len() as f32;
    assert!(mean > 1.0, "decoded frame looks empty (mean={mean})");
}

#[test]
fn uyvy_1080p_psnr_roundtrip() {
    let width = 1920;
    let height = 1080;
    let (frame, stride) = make_uyvy(width, height);
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtHq,
        color_space: Default::default(),
    })
    .unwrap();
    enc.encode_uyvy(&frame, stride).unwrap();
    let mut bitstream = vec![0u8; 8 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut dec = Codec::new(Config::new(width, height)).unwrap();
    dec.load_from(&bitstream[..len]).unwrap();
    let mut out = vec![0u8; stride * height as usize];
    dec.decode_uyvy(&mut out, stride).unwrap();

    let psnr = dec.calculate_psnr(&frame, &out, stride, 2);
    assert!(
        psnr >= 25.0,
        "1080p UYVY PSNR too low: {psnr} (need ≥ 25 dB at OmtHq lossy)"
    );
}

#[test]
fn bgra_decode_stride_and_size_checks() {
    let mut enc = Codec::new(Config::new(64, 64)).unwrap();
    let (frame, stride) = make_uyvy(64, 64);
    enc.encode_uyvy(&frame, stride).unwrap();
    let mut bitstream = vec![0u8; 1 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut dec = Codec::new(Config::new(64, 64)).unwrap();
    dec.load_from(&bitstream[..len]).unwrap();

    let mut too_small = vec![0u8; 64 * 4 * 32];
    assert!(dec.decode_bgra(&mut too_small, 64 * 4).is_err());

    let mut bad_stride = vec![0u8; 32 * 4 * 64];
    assert!(dec.decode_bgra(&mut bad_stride, 32 * 4).is_err());

    let mut ok = vec![0u8; 64 * 4 * 64];
    dec.decode_bgra(&mut ok, 64 * 4).unwrap();
}

#[test]
fn repeat_decode_same_bitstream() {
    let (frame, stride) = make_uyvy(128, 128);
    let mut enc = Codec::new(Config::new(128, 128)).unwrap();
    enc.encode_uyvy(&frame, stride).unwrap();
    let mut bitstream = vec![0u8; 2 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut dec = Codec::new(Config::new(128, 128)).unwrap();
    let mut out_a = vec![0u8; stride * 128];
    let mut out_b = vec![0u8; stride * 128];
    for i in 0..100 {
        dec.load_from(&bitstream[..len]).unwrap();
        let dst = if i % 2 == 0 {
            &mut out_a
        } else {
            &mut out_b
        };
        dec.decode_uyvy(dst, stride).unwrap();
    }
    assert_eq!(out_a, out_b);
}

#[test]
fn truncated_bitstream_rejects() {
    let (frame, stride) = make_uyvy(64, 64);
    let mut enc = Codec::new(Config::new(64, 64)).unwrap();
    enc.encode_uyvy(&frame, stride).unwrap();
    let mut bitstream = vec![0u8; 1 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();
    assert!(len > 16);

    let mut dec = Codec::new(Config::new(64, 64)).unwrap();
    assert!(dec.load_from(&bitstream[..8]).is_err());
    assert!(dec.load_from(&bitstream[..len / 2]).is_err());
}

#[test]
fn mag_sign_in_bitstream_module() {
    let _ = Codec::new(Config::new(32, 32));
}
