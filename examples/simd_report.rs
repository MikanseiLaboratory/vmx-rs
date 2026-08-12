//! Report SIMD path and measure public-API encode/decode workloads.
//!
//! Usage:
//!   cargo run --release --example simd_report
//!   cargo run --release --example simd_report -- 1920 1080 40
//!   cargo run --release --example simd_report -- 3840 2160 20

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

fn make_bgra(width: i32, height: i32) -> (Vec<u8>, usize) {
    let stride = (width as usize) * 4;
    let mut frame = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let o = y * stride + x * 4;
            frame[o] = ((x * 3 + y) % 256) as u8;
            frame[o + 1] = ((x + y * 5) % 256) as u8;
            frame[o + 2] = ((x * 7 + y * 2) % 256) as u8;
            frame[o + 3] = 255;
        }
    }
    (frame, stride)
}

fn median_ms(samples: &mut [f64]) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        samples[n / 2]
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) * 0.5
    }
}

fn report(label: &str, mut samples_ms: Vec<f64>) {
    let med = median_ms(&mut samples_ms);
    let avg = samples_ms.iter().sum::<f64>() / samples_ms.len() as f64;
    println!(
        "{label}: median={med:.3} ms/frame ({:.1} fps) avg={avg:.3} ms over {} iters",
        1000.0 / med.max(1e-9),
        samples_ms.len()
    );
}

fn timed_iters<F: FnMut()>(warmup: usize, iters: usize, mut body: F) -> Vec<f64> {
    for _ in 0..warmup {
        body();
    }
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        body();
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

fn bench_resolution(width: i32, height: i32, iters: usize) {
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::Hq,
        color_space: Default::default(),
    })
    .expect("create encoder");
    let mut dec = Codec::new(Config {
        width,
        height,
        profile: Profile::Hq,
        color_space: Default::default(),
    })
    .expect("create decoder");

    let caps = enc.simd_capabilities();
    println!(
        "=== {width}x{height} path={} color={} caps={{ssse3:{},sse42:{},avx2:{},bmi2:{},neon:{}}} ===",
        enc.simd_path(),
        enc.color_simd_path(),
        caps.ssse3,
        caps.sse42,
        caps.avx2,
        caps.bmi2,
        caps.neon
    );

    let (uyvy, uyvy_stride) = make_uyvy(width, height);
    let (bgra, bgra_stride) = make_bgra(width, height);
    let mut bitstream = vec![0u8; 16 << 20];
    let mut uyvy_out = vec![0u8; uyvy.len()];
    let mut bgra_out = vec![0u8; bgra.len()];

    // Seed bitstream for decode benches.
    enc.encode_uyvy(&uyvy, uyvy_stride).expect("seed encode");
    let encoded_len = enc.save_to(&mut bitstream).expect("seed save");
    let encoded = bitstream[..encoded_len].to_vec();

    let warmup = (iters / 5).clamp(2, 8);

    report(
        "encode_uyvy",
        timed_iters(warmup, iters, || {
            enc.encode_uyvy(&uyvy, uyvy_stride).expect("encode_uyvy");
        }),
    );

    report(
        "encode_bgra",
        timed_iters(warmup, iters, || {
            enc.encode_bgra(&bgra, bgra_stride).expect("encode_bgra");
        }),
    );

    report(
        "save_to",
        timed_iters(warmup, iters, || {
            enc.encode_uyvy(&uyvy, uyvy_stride)
                .expect("encode before save");
            let _ = enc.save_to(&mut bitstream).expect("save_to");
        }),
    );

    report(
        "load_from+decode_uyvy",
        timed_iters(warmup, iters, || {
            dec.load_from(&encoded).expect("load");
            dec.decode_uyvy(&mut uyvy_out, uyvy_stride)
                .expect("decode_uyvy");
        }),
    );

    report(
        "load_from+decode_bgra",
        timed_iters(warmup, iters, || {
            dec.load_from(&encoded).expect("load");
            dec.decode_bgra(&mut bgra_out, bgra_stride)
                .expect("decode_bgra");
        }),
    );

    // Combined encode+save (historical metric).
    report(
        "encode_uyvy+save",
        timed_iters(warmup, iters, || {
            enc.encode_uyvy(&uyvy, uyvy_stride).expect("encode");
            let _ = enc.save_to(&mut bitstream).expect("save");
        }),
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let width: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1920);
    let height: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1080);
    let iters: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20)
        .max(1);

    println!("simd_report iters={iters} (median over timed samples, warm-up excluded)");
    bench_resolution(width, height, iters);

    // Also emit a 4K sample when the primary size is 1080p.
    if width == 1920 && height == 1080 {
        let four_k_iters = (iters / 2).max(4);
        println!();
        bench_resolution(3840, 2160, four_k_iters);
    }
}
