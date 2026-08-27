//! Compare GPU texture decode/encode with the CPU work that replaces each GPU path.
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
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
}

fn psnr_bgra(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mut sse = 0.0f64;
    let n = (a.len() / 4) * 3;
    if n == 0 {
        return 100.0;
    }
    for i in (0..a.len()).step_by(4) {
        for c in 0..3 {
            let d = f64::from(a[i + c]) - f64::from(b[i + c]);
            sse += d * d;
        }
    }
    if sse == 0.0 {
        return 100.0;
    }
    10.0 * (255.0 * 255.0 * n as f64 / sse).log10()
}

fn rgb_err(a: &[u8], b: &[u8]) -> (f64, u8) {
    let mut sum = 0.0f64;
    let mut max = 0u8;
    let mut n = 0.0f64;
    for i in (0..a.len()).step_by(4) {
        for c in 0..3 {
            let d = a[i + c].abs_diff(b[i + c]);
            sum += f64::from(d);
            max = max.max(d);
            n += 1.0;
        }
    }
    (sum / n.max(1.0), max)
}

fn print_quality(label: &str, cpu: &[u8], gpu: &[u8]) {
    let (mae, max_abs) = rgb_err(cpu, gpu);
    println!(
        "quality {label}: PSNR={:.2} dB  MAE={mae:.2}  max_abs={max_abs}",
        psnr_bgra(cpu, gpu)
    );
}

fn quality_report(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: i32,
    height: i32,
    encoded: &[u8],
    src_bgra: &[u8],
    src_tex: &wgpu::Texture,
) {
    let stride = width as usize * 4;

    let mut cpu_dec = Codec::new(Config::new(width, height)).expect("cpu quality");
    cpu_dec.load_from(encoded).expect("load");
    let mut cpu_out = vec![0u8; stride * height as usize];
    cpu_dec
        .decode_bgra(&mut cpu_out, stride)
        .expect("cpu decode");

    let mut gpu_dec = Codec::new(Config::new(width, height)).expect("gpu quality");
    gpu_dec.load_from(encoded).expect("load");
    let gpu_frame = gpu_dec
        .decode_to_texture(device, queue)
        .expect("gpu decode");
    let gpu_out = gpu::read_texture_bgra(
        device,
        queue,
        &gpu_frame.texture,
        gpu_frame.width,
        gpu_frame.height,
    )
    .expect("read decode");
    print_quality("GPU decode vs CPU decode", &cpu_out, &gpu_out);

    let ps = cpu_dec.preview_size();
    let pstride = ps.width as usize * 4;
    let mut cpu_prev = vec![0u8; pstride * ps.height as usize];
    cpu_dec
        .decode_preview_bgra(&mut cpu_prev, pstride)
        .expect("cpu preview");
    let gpu_prev = gpu_dec
        .decode_preview_to_texture(device, queue)
        .expect("gpu preview");
    let gpu_prev_px = gpu::read_texture_bgra(
        device,
        queue,
        &gpu_prev.texture,
        gpu_prev.width,
        gpu_prev.height,
    )
    .expect("read preview");
    print_quality("GPU preview vs CPU preview", &cpu_prev, &gpu_prev_px);

    let mut cpu_enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtSq,
        color_space: Default::default(),
    })
    .expect("cpu enc");
    cpu_enc.encode_bgra(src_bgra, stride).expect("cpu encode");
    let mut cpu_bs = vec![0u8; 16 << 20];
    let cpu_len = cpu_enc.save_to(&mut cpu_bs).expect("save cpu");
    let mut cpu_rt = Codec::new(Config::new(width, height)).expect("cpu rt");
    cpu_rt.load_from(&cpu_bs[..cpu_len]).expect("load");
    let mut cpu_enc_out = vec![0u8; stride * height as usize];
    cpu_rt
        .decode_bgra(&mut cpu_enc_out, stride)
        .expect("cpu rt decode");

    let mut gpu_enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtSq,
        color_space: Default::default(),
    })
    .expect("gpu enc");
    gpu_enc
        .encode_from_texture(device, queue, src_tex)
        .expect("gpu encode");
    let mut gpu_bs = vec![0u8; 16 << 20];
    let gpu_len = gpu_enc.save_to(&mut gpu_bs).expect("save gpu");
    let mut gpu_rt = Codec::new(Config::new(width, height)).expect("gpu rt");
    gpu_rt.load_from(&gpu_bs[..gpu_len]).expect("load");
    let mut gpu_enc_out = vec![0u8; stride * height as usize];
    gpu_rt
        .decode_bgra(&mut gpu_enc_out, stride)
        .expect("gpu rt decode");
    print_quality(
        "GPU encode vs CPU encode (both CPU-decoded)",
        &cpu_enc_out,
        &gpu_enc_out,
    );
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

    let ps = cpu_dec.preview_size();
    let pstride = ps.width as usize * 4;
    let mut cpu_preview = vec![0u8; pstride * ps.height as usize];
    cpu_dec
        .decode_preview_bgra(&mut cpu_preview, pstride)
        .expect("seed preview");

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

    quality_report(device, queue, width, height, &encoded, &bgra, &src_tex);

    let warmup = (iters / 5).clamp(2, 8);

    println!("-- decode (host pixels in, texture out) --");
    report(
        "CPU decode_bgra (pixels only)",
        timed_iters(warmup, iters, || {
            cpu_dec
                .decode_bgra(&mut cpu_out, stride)
                .expect("cpu decode");
        }),
    );
    report(
        "CPU equivalent of GPU decode (decode_bgra + write_texture)",
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

    println!("-- preview (1/8, texture out) --");
    report(
        "CPU decode_preview_bgra (pixels only)",
        timed_iters(warmup, iters, || {
            cpu_dec
                .decode_preview_bgra(&mut cpu_preview, pstride)
                .expect("cpu preview");
        }),
    );
    report(
        "CPU equivalent of GPU preview (preview_bgra + write_texture)",
        timed_iters(warmup, iters, || {
            cpu_dec
                .decode_preview_bgra(&mut cpu_preview, pstride)
                .expect("cpu preview");
            let _tex = gpu::upload_bgra_texture(
                device,
                queue,
                ps.width as u32,
                ps.height as u32,
                &cpu_preview,
            );
            wait_idle(device);
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

    println!("-- encode (texture or host pixels in, bitstream out) --");
    report(
        "CPU encode_bgra (host pixels)",
        timed_iters(warmup, iters, || {
            enc.encode_bgra(&bgra, stride).expect("cpu encode");
        }),
    );
    report(
        "CPU equivalent of GPU encode (read_texture + encode_bgra)",
        timed_iters(warmup, iters, || {
            let pixels =
                gpu::read_texture_bgra(device, queue, &src_tex, width as u32, height as u32)
                    .expect("readback");
            enc.encode_bgra(&pixels, stride).expect("cpu encode");
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
        "adapter={} backend={:?} device_type={:?} bgra8_storage={}",
        info.name,
        info.backend,
        info.device_type,
        adapter
            .features()
            .contains(wgpu::Features::BGRA8UNORM_STORAGE)
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
