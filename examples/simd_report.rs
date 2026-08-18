//! Report SIMD path and measure public-API encode/decode workloads.
//!
//! Usage:
//!   cargo run --release --example simd_report
//!   cargo run --release --example simd_report -- 1920 1080 40
//!   cargo run --release --example simd_report -- 3840 2160 20
//!
//! Optional path overrides (4th / 5th args):
//!   cargo +nightly run --release --features portable-simd --example simd_report -- \
//!     1920 1080 16 portable portable

use std::time::Instant;
use vmx::{Codec, ColorSimdPath, Config, Profile, SimdPath};

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

fn parse_simd_path(s: &str) -> Option<SimdPath> {
    match s.to_ascii_lowercase().as_str() {
        "scalar" => Some(SimdPath::Scalar),
        "sse128" | "sse" => Some(SimdPath::Sse128),
        "avx2" => Some(SimdPath::Avx2),
        "avx512" => Some(SimdPath::Avx512),
        "neon" => Some(SimdPath::Neon),
        #[cfg(feature = "portable-simd")]
        "portable" => Some(SimdPath::Portable),
        "auto" | "" => None,
        _ => None,
    }
}

fn parse_color_path(s: &str) -> Option<ColorSimdPath> {
    match s.to_ascii_lowercase().as_str() {
        "scalar" => Some(ColorSimdPath::Scalar),
        "sse2" | "sse" => Some(ColorSimdPath::Sse2),
        "avx2" => Some(ColorSimdPath::Avx2),
        "avx512" => Some(ColorSimdPath::Avx512),
        "neon" => Some(ColorSimdPath::Neon),
        #[cfg(feature = "portable-simd")]
        "portable" => Some(ColorSimdPath::Portable),
        "auto" | "" => None,
        _ => None,
    }
}

fn simd_path_supported(path: SimdPath) -> bool {
    match path {
        SimdPath::Scalar => true,
        #[cfg(feature = "portable-simd")]
        SimdPath::Portable => true,
        SimdPath::Sse128 => {
            #[cfg(target_arch = "x86_64")]
            {
                is_x86_feature_detected!("sse4.2") && is_x86_feature_detected!("ssse3")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        SimdPath::Avx2 => {
            #[cfg(target_arch = "x86_64")]
            {
                is_x86_feature_detected!("avx2") && is_x86_feature_detected!("bmi2")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        SimdPath::Avx512 => {
            #[cfg(target_arch = "x86_64")]
            {
                is_x86_feature_detected!("avx512f")
                    && is_x86_feature_detected!("avx512bw")
                    && is_x86_feature_detected!("bmi2")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        SimdPath::Neon => {
            #[cfg(target_arch = "aarch64")]
            {
                true
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                false
            }
        }
    }
}

fn color_path_supported(path: ColorSimdPath) -> bool {
    match path {
        ColorSimdPath::Scalar => true,
        #[cfg(feature = "portable-simd")]
        ColorSimdPath::Portable => true,
        ColorSimdPath::Sse2 => {
            #[cfg(target_arch = "x86_64")]
            {
                is_x86_feature_detected!("sse2")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        ColorSimdPath::Avx2 => {
            #[cfg(target_arch = "x86_64")]
            {
                is_x86_feature_detected!("avx2")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        ColorSimdPath::Avx512 => {
            #[cfg(target_arch = "x86_64")]
            {
                is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        ColorSimdPath::Neon => {
            #[cfg(target_arch = "aarch64")]
            {
                true
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                false
            }
        }
    }
}

fn bench_resolution(
    width: i32,
    height: i32,
    iters: usize,
    force_simd: Option<SimdPath>,
    force_color: Option<ColorSimdPath>,
) {
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

    if let Some(p) = force_simd {
        if !simd_path_supported(p) {
            eprintln!("skip: dct path {p} not supported on this CPU");
            return;
        }
        enc.set_simd_path(p);
        dec.set_simd_path(p);
    }
    if let Some(p) = force_color {
        if !color_path_supported(p) {
            eprintln!("skip: color path {p} not supported on this CPU");
            return;
        }
        enc.set_color_simd_path(p);
        dec.set_color_simd_path(p);
    }

    let caps = enc.simd_capabilities();
    println!(
        "=== {width}x{height} path={} color={} caps={{ssse3:{},sse42:{},avx2:{},bmi2:{},avx512:{},neon:{}}} ===",
        enc.simd_path(),
        enc.color_simd_path(),
        caps.ssse3,
        caps.sse42,
        caps.avx2,
        caps.bmi2,
        caps.avx512,
        caps.neon
    );

    let (uyvy, uyvy_stride) = make_uyvy(width, height);
    let (bgra, bgra_stride) = make_bgra(width, height);
    let mut bitstream = vec![0u8; 16 << 20];
    let mut uyvy_out = vec![0u8; uyvy.len()];
    let mut bgra_out = vec![0u8; bgra.len()];

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
    let force_simd = args.next().as_deref().and_then(parse_simd_path);
    let force_color = args.next().as_deref().and_then(parse_color_path);

    println!("simd_report iters={iters} (median over timed samples, warm-up excluded)");
    if force_simd.is_some() || force_color.is_some() {
        println!("forced path: dct={force_simd:?} color={force_color:?}");
    }
    bench_resolution(width, height, iters, force_simd, force_color);

    if width == 1920 && height == 1080 && force_simd.is_none() && force_color.is_none() {
        let four_k_iters = (iters / 2).max(4);
        println!();
        bench_resolution(3840, 2160, four_k_iters, None, None);
    }
}
