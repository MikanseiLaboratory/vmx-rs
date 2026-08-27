//! Compare CPU BGRA I/O with GPU texture decode/encode.
//!
//! Usage:
//!   cargo run --release --example gpu_bench --features wgpu
//!   cargo run --release --example gpu_bench --features wgpu -- 1920 1080 20

use std::time::Instant;

use vmx::{Codec, Config, Profile, gpu};

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

fn wait_idle(device: &wgpu::Device) {
    let _ = device.poll(wgpu::PollType::Wait);
}

fn bench_resolution(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: i32,
    height: i32,
    iters: usize,
) {
    let stride = (width as usize) * 4;
    let mut bgra = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let o = y * stride + x * 4;
            bgra[o] = (x % 256) as u8;
            bgra[o + 1] = (y % 256) as u8;
            bgra[o + 2] = 128;
            bgra[o + 3] = 255;
        }
    }

    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtSq,
        color_space: Default::default(),
    })
    .expect("codec");
    enc.encode_bgra(&bgra, stride).expect("encode");
    let mut bitstream = vec![0u8; 16 << 20];
    let len = enc.save_to(&mut bitstream).expect("save");
    let encoded = bitstream[..len].to_vec();

    let mut cpu_dec = Codec::new(Config::new(width, height)).expect("cpu");
    cpu_dec.load_from(&encoded).expect("load");
    let mut cpu_out = vec![0u8; stride * height as usize];
    cpu_dec
        .decode_bgra(&mut cpu_out, stride)
        .expect("seed decode");

    let mut gpu_dec = Codec::new(Config::new(width, height)).expect("gpu codec");
    gpu_dec.load_from(&encoded).expect("load");
    let _ = gpu_dec
        .decode_to_texture(device, queue)
        .expect("gpu warmup decode");

    let src_tex = gpu::upload_bgra_texture(device, queue, width as u32, height as u32, &bgra);
    wait_idle(device);

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

    let warmup = (iters / 5).clamp(2, 8);

    report(
        "CPU decode_bgra",
        timed_iters(warmup, iters, || {
            cpu_dec
                .decode_bgra(&mut cpu_out, stride)
                .expect("cpu decode");
        }),
    );

    report(
        "CPU decode_bgra + write_texture",
        timed_iters(warmup, iters, || {
            cpu_dec
                .decode_bgra(&mut cpu_out, stride)
                .expect("cpu decode");
            let _tex =
                gpu::upload_bgra_texture(device, queue, width as u32, height as u32, &cpu_out);
            wait_idle(device);
        }),
    );

    report(
        "GPU decode_to_texture",
        timed_iters(warmup, iters, || {
            let _frame = gpu_dec
                .decode_to_texture(device, queue)
                .expect("gpu decode");
        }),
    );

    report(
        "GPU decode_preview_to_texture",
        timed_iters(warmup, iters, || {
            let _frame = gpu_dec
                .decode_preview_to_texture(device, queue)
                .expect("gpu preview");
        }),
    );

    report(
        "CPU encode_bgra",
        timed_iters(warmup, iters, || {
            enc.encode_bgra(&bgra, stride).expect("cpu encode");
        }),
    );

    let mut gpu_enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtSq,
        color_space: Default::default(),
    })
    .expect("gpu enc");
    gpu_enc
        .encode_from_texture(device, queue, &src_tex)
        .expect("gpu encode warmup");

    report(
        "GPU encode_from_texture",
        timed_iters(warmup, iters, || {
            gpu_enc
                .encode_from_texture(device, queue, &src_tex)
                .expect("gpu encode");
        }),
    );
}

fn main() {
    let Some((_inst, adapter, device, queue)) = gpu::request_headless_device() else {
        eprintln!("no wgpu adapter");
        std::process::exit(1);
    };

    let info = adapter.get_info();
    println!(
        "adapter={} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );

    let mut args = std::env::args().skip(1);
    let width: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1920);
    let height: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1080);
    let iters: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20)
        .max(1);

    println!("gpu_bench iters={iters} (median over timed samples, warm-up excluded)");
    bench_resolution(&device, &queue, width, height, iters);

    if width == 1920 && height == 1080 {
        println!();
        bench_resolution(&device, &queue, 3840, 2160, (iters / 2).max(4));
    }
}
