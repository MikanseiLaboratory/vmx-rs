//! Print the selected SIMD path and host capabilities.

use std::time::Instant;
use vmx::{Codec, Config, Profile};

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

fn main() {
    let width = 1920;
    let height = 1080;
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::Hq,
        color_space: Default::default(),
    })
    .expect("create");
    let caps = enc.simd_capabilities();
    println!(
        "path={} caps={{ssse3:{},sse42:{},avx2:{},bmi2:{},neon:{}}}",
        enc.simd_path(),
        caps.ssse3,
        caps.sse42,
        caps.avx2,
        caps.bmi2,
        caps.neon
    );

    let (frame, stride) = make_uyvy(width, height);
    // Warmup
    enc.encode_uyvy(&frame, stride).expect("encode warmup");
    let mut buf = vec![0u8; 8 << 20];
    let _ = enc.save_to(&mut buf).expect("save warmup");

    let iters = 20;
    let t0 = Instant::now();
    for _ in 0..iters {
        enc.encode_uyvy(&frame, stride).expect("encode");
        let _ = enc.save_to(&mut buf).expect("save");
    }
    let elapsed = t0.elapsed();
    let ms = elapsed.as_secs_f64() * 1000.0 / f64::from(iters);
    println!(
        "encode+save avg {ms:.2} ms/frame ({:.1} fps) over {iters} iters @ {width}x{height}",
        1000.0 / ms
    );
}
