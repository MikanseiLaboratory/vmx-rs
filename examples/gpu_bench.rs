//! Compare GPU texture decode/encode with the CPU work that replaces each GPU path.
//!
//! The timed loops are unchanged. A UYVY frame is converted to BGRA on the CPU
//! before those loops; that conversion is not included in the samples. Each
//! GPU path is also run untimed (`--prime`, default 20) before its timed
//! samples, so the first profile at a resolution is not the cold transfer.
//!
//! Usage:
//!   cargo run --release --example gpu_bench --features wgpu
//!   cargo run --release --example gpu_bench --features wgpu -- 1920 1080 20
//!   cargo run --release --example gpu_bench --features wgpu -- \
//!     --image bench/images/1080p-natural.uyvy.gz --profile OMT_HQ --iters 20
//!   cargo run --release --example gpu_bench --features wgpu -- \
//!     --image natural --res 1080 --profile all
//!
//! `--image` is a file path or a name under `bench/images/` (`natural`, `bars`,
//! `1080p-natural`, `2160p-bars`, with or without `.uyvy.gz`). `natural` and
//! `bars` need `--res 1080` or `--res 2160`. Omit `--image` for the synthetic
//! gradient. `--profile` is `OMT_LQ`, `OMT_SQ`, `OMT_HQ`, or `all` (default
//! `OMT_SQ`). `--prime N` (default 20) is the untimed GPU-path iterations
//! before the timed samples; `--prime 0` skips them.
//!
//! CSV on stdout:
//!   res,image,profile,case,ms_median,fps_median,ms_samples
//! `image` is the file name, or `synthetic`. Quality lines and the adapter
//! name go to stderr. `--require-gpu` exits if the adapter is llvmpipe,
//! lavapipe, SwiftShader, or a CPU device. `--probe` prints the adapter and
//! exits before any timed sample.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use vmx::{Codec, Config, Profile, gpu};

fn median_ms(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) * 0.5
    }
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
    eprintln!(
        "quality {label}: PSNR={:.2} dB  MAE={mae:.2}  max_abs={max_abs}",
        psnr_bgra(cpu, gpu)
    );
}

fn quality_report(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &Frame,
    profile: Profile,
    encoded: &[u8],
    src_tex: &wgpu::Texture,
) {
    let width = frame.width;
    let height = frame.height;
    let src_bgra = &frame.bgra;
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
        profile,
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
        profile,
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

struct Frame {
    image: String,
    width: i32,
    height: i32,
    bgra: Vec<u8>,
}

fn res_label(width: i32, height: i32) -> String {
    match (width, height) {
        (1920, 1080) => "1080p".to_string(),
        (3840, 2160) => "2160p".to_string(),
        _ => format!("{width}x{height}"),
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn emit_case(out: &mut dyn Write, frame: &Frame, profile: &str, label: &str, samples: &[f64]) {
    let med = median_ms(samples);
    let avg = samples.iter().sum::<f64>() / samples.len() as f64;
    let fps = 1000.0 / med.max(1e-9);
    eprintln!(
        "{label}: median={med:.3} ms/frame ({fps:.1} fps) avg={avg:.3} ms over {} iters",
        samples.len()
    );
    let samples_field = samples
        .iter()
        .map(|v| format!("{v:.3}"))
        .collect::<Vec<_>>()
        .join(";");
    writeln!(
        out,
        "{},{},{},{},{med:.3},{fps:.1},{samples_field}",
        res_label(frame.width, frame.height),
        csv_field(&frame.image),
        profile,
        csv_field(label),
    )
    .expect("write csv");
    let _ = out.flush();
}

fn discard_iters<F: FnMut()>(iters: usize, mut body: F) {
    for _ in 0..iters {
        body();
    }
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

struct Pass {
    iters: usize,
    prime: usize,
}

fn bench_resolution(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &Frame,
    profile: Profile,
    profile_name: &str,
    pass: Pass,
    out: &mut dyn Write,
) {
    let iters = pass.iters;
    let prime = pass.prime;
    let width = frame.width;
    let height = frame.height;
    let bgra = &frame.bgra;
    let stride = (width as usize) * 4;

    let mut enc = Codec::new(Config {
        width,
        height,
        profile,
        color_space: Default::default(),
    })
    .expect("codec");
    enc.encode_bgra(bgra, stride).expect("encode");
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

    let src_tex = gpu::upload_bgra_texture(device, queue, width as u32, height as u32, bgra);
    wait_idle(device);

    let caps = enc.simd_capabilities();
    eprintln!(
        "=== {width}x{height} image={} profile={profile_name} path={} color={} caps={{ssse3:{},sse42:{},avx2:{},bmi2:{},neon:{}}} ===",
        frame.image,
        enc.simd_path(),
        enc.color_simd_path(),
        caps.ssse3,
        caps.sse42,
        caps.avx2,
        caps.bmi2,
        caps.neon
    );

    quality_report(device, queue, frame, profile, &encoded, &src_tex);

    let mut gpu_enc = Codec::new(Config {
        width,
        height,
        profile,
        color_space: Default::default(),
    })
    .expect("gpu enc");

    if prime > 0 {
        eprintln!("priming {prime} untimed iters of each GPU path (not in the median)");
        discard_iters(prime, || {
            cpu_dec
                .decode_bgra(&mut cpu_out, stride)
                .expect("prime decode");
            let _tex =
                gpu::upload_bgra_texture(device, queue, width as u32, height as u32, &cpu_out);
            wait_idle(device);
        });
        discard_iters(prime, || {
            let _frame = gpu_dec
                .decode_to_texture(device, queue)
                .expect("prime gpu decode");
            wait_idle(device);
        });
        discard_iters(prime, || {
            cpu_dec
                .decode_preview_bgra(&mut cpu_preview, pstride)
                .expect("prime preview");
            let _tex = gpu::upload_bgra_texture(
                device,
                queue,
                ps.width as u32,
                ps.height as u32,
                &cpu_preview,
            );
            wait_idle(device);
        });
        discard_iters(prime, || {
            let _frame = gpu_dec
                .decode_preview_to_texture(device, queue)
                .expect("prime gpu preview");
            wait_idle(device);
        });
        discard_iters(prime, || {
            let pixels =
                gpu::read_texture_bgra(device, queue, &src_tex, width as u32, height as u32)
                    .expect("prime readback");
            enc.encode_bgra(&pixels, stride).expect("prime cpu encode");
        });
        discard_iters(prime, || {
            gpu_enc
                .encode_from_texture(device, queue, &src_tex)
                .expect("prime gpu encode");
        });
        discard_iters(prime, || {
            gpu_enc
                .encode_from_texture_opaque(device, queue, &src_tex)
                .expect("prime gpu opaque");
        });
    }

    let warmup = (iters / 5).clamp(2, 8);

    eprintln!("-- decode (host pixels in, texture out) --");
    emit_case(
        out,
        frame,
        profile_name,
        "CPU decode_bgra (pixels only)",
        &timed_iters(warmup, iters, || {
            cpu_dec
                .decode_bgra(&mut cpu_out, stride)
                .expect("cpu decode");
        }),
    );
    emit_case(
        out,
        frame,
        profile_name,
        "CPU equivalent of GPU decode (decode_bgra + write_texture)",
        &timed_iters(warmup, iters, || {
            cpu_dec
                .decode_bgra(&mut cpu_out, stride)
                .expect("cpu decode");
            let _tex =
                gpu::upload_bgra_texture(device, queue, width as u32, height as u32, &cpu_out);
            wait_idle(device);
        }),
    );
    emit_case(
        out,
        frame,
        profile_name,
        "GPU decode_to_texture",
        &timed_iters(warmup, iters, || {
            let _frame = gpu_dec
                .decode_to_texture(device, queue)
                .expect("gpu decode");
            wait_idle(device);
        }),
    );

    eprintln!("-- preview (1/8, texture out) --");
    emit_case(
        out,
        frame,
        profile_name,
        "CPU decode_preview_bgra (pixels only)",
        &timed_iters(warmup, iters, || {
            cpu_dec
                .decode_preview_bgra(&mut cpu_preview, pstride)
                .expect("cpu preview");
        }),
    );
    emit_case(
        out,
        frame,
        profile_name,
        "CPU equivalent of GPU preview (preview_bgra + write_texture)",
        &timed_iters(warmup, iters, || {
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
    emit_case(
        out,
        frame,
        profile_name,
        "GPU decode_preview_to_texture",
        &timed_iters(warmup, iters, || {
            let _frame = gpu_dec
                .decode_preview_to_texture(device, queue)
                .expect("gpu preview");
            wait_idle(device);
        }),
    );

    eprintln!("-- encode (texture or host pixels in, bitstream out) --");
    emit_case(
        out,
        frame,
        profile_name,
        "CPU encode_bgra (host pixels)",
        &timed_iters(warmup, iters, || {
            enc.encode_bgra(bgra, stride).expect("cpu encode");
        }),
    );
    emit_case(
        out,
        frame,
        profile_name,
        "CPU equivalent of GPU encode (read_texture + encode_bgra)",
        &timed_iters(warmup, iters, || {
            let pixels =
                gpu::read_texture_bgra(device, queue, &src_tex, width as u32, height as u32)
                    .expect("readback");
            enc.encode_bgra(&pixels, stride).expect("cpu encode");
        }),
    );

    if prime == 0 {
        gpu_enc
            .encode_from_texture(device, queue, &src_tex)
            .expect("gpu encode warmup");
    }

    emit_case(
        out,
        frame,
        profile_name,
        "GPU encode_from_texture (4-plane / alpha)",
        &timed_iters(warmup, iters, || {
            gpu_enc
                .encode_from_texture(device, queue, &src_tex)
                .expect("gpu encode");
        }),
    );
    emit_case(
        out,
        frame,
        profile_name,
        "GPU encode_from_texture_opaque (3-plane)",
        &timed_iters(warmup, iters, || {
            gpu_enc
                .encode_from_texture_opaque(device, queue, &src_tex)
                .expect("gpu opaque");
        }),
    );

    let t_submit = std::time::Instant::now();
    gpu_enc
        .encode_from_texture_submit(device, queue, &src_tex, false)
        .expect("submit");
    let submit_ms = t_submit.elapsed().as_secs_f64() * 1e3;
    let t_finish = std::time::Instant::now();
    gpu_enc.encode_submitted_finish(device).expect("finish");
    let finish_ms = t_finish.elapsed().as_secs_f64() * 1e3;
    eprintln!(
        "GPU pipeline split (opaque, 1 sample): submit={submit_ms:.3} ms finish={finish_ms:.3} ms"
    );

    emit_case(
        out,
        frame,
        profile_name,
        "CPU encode_bgrx (3-plane)",
        &timed_iters(warmup, iters, || {
            enc.encode_bgrx(bgra, stride).expect("cpu bgrx");
        }),
    );
}

fn synthetic_bgra(width: i32, height: i32) -> Frame {
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
    Frame {
        image: "synthetic".to_string(),
        width,
        height,
        bgra,
    }
}

fn read_bytes(path: &Path) -> Vec<u8> {
    let file = File::open(path).unwrap_or_else(|err| panic!("open {}: {err}", path.display()));
    let mut buf = Vec::new();
    if path.extension().and_then(|ext| ext.to_str()) == Some("gz") {
        let mut dec = flate2::read::GzDecoder::new(file);
        dec.read_to_end(&mut buf)
            .unwrap_or_else(|err| panic!("gunzip {}: {err}", path.display()));
    } else {
        let mut file = file;
        file.read_to_end(&mut buf)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    }
    buf
}

fn res_tag(res: &str) -> &'static str {
    match res.to_ascii_lowercase().as_str() {
        "1080" | "1080p" | "1920x1080" => "1080p",
        "2160" | "2160p" | "3840x2160" => "2160p",
        _ => panic!("--res must be 1080, 1080p, 2160, or 2160p (got {res})"),
    }
}

fn dims_from_tag(tag: &str) -> (i32, i32) {
    match tag {
        "1080p" => (1920, 1080),
        "2160p" => (3840, 2160),
        _ => panic!("unknown resolution tag {tag}"),
    }
}

fn images_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench/images")
}

fn resolve_image(spec: &str, res: Option<&str>) -> PathBuf {
    let direct = PathBuf::from(spec);
    if direct.is_file() {
        return direct;
    }
    let root = images_dir();
    let mut candidates = vec![
        root.join(spec),
        root.join(format!("{spec}.uyvy.gz")),
        root.join(format!("{spec}.uyvy")),
    ];
    if let Some(res) = res {
        let tag = res_tag(res);
        let name = spec.trim_end_matches(".uyvy.gz").trim_end_matches(".uyvy");
        candidates.push(root.join(format!("{tag}-{name}.uyvy.gz")));
        candidates.push(root.join(format!("{tag}-{name}.uyvy")));
    }
    for candidate in &candidates {
        if candidate.is_file() {
            return candidate.clone();
        }
    }
    panic!(
        "image not found: {spec} (looked under {}). For natural or bars, pass --res 1080 or --res 2160.",
        root.display()
    );
}

fn frame_dims(
    path: &Path,
    nbytes: usize,
    res: Option<&str>,
    width: Option<i32>,
    height: Option<i32>,
) -> (i32, i32) {
    let (width, height) = if let (Some(width), Some(height)) = (width, height) {
        (width, height)
    } else if let Some(res) = res {
        dims_from_tag(res_tag(res))
    } else {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.contains("1080p") {
            (1920, 1080)
        } else if name.contains("2160p") {
            (3840, 2160)
        } else {
            match nbytes {
                4_147_200 => (1920, 1080),
                16_588_800 => (3840, 2160),
                _ => panic!(
                    "cannot infer resolution of {} ({nbytes} bytes); pass --res or --width and --height",
                    path.display()
                ),
            }
        }
    };
    let need = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(2))
        .unwrap_or(0);
    assert!(
        width > 0 && height > 0 && width % 2 == 0 && nbytes == need,
        "{} has {nbytes} bytes, expected {need} for {width}x{height} UYVY",
        path.display()
    );
    (width, height)
}

fn load_uyvy_frame(
    spec: &str,
    res: Option<&str>,
    width: Option<i32>,
    height: Option<i32>,
) -> Frame {
    let path = resolve_image(spec, res);
    let uyvy = read_bytes(&path);
    let (width, height) = frame_dims(&path, uyvy.len(), res, width, height);
    let bgra = vmx::uyvy_to_bgra(&uyvy, width, height)
        .unwrap_or_else(|err| panic!("UYVY to BGRA {}: {err}", path.display()));
    let image = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(spec)
        .to_string();
    eprintln!(
        "loaded {image}: {width}x{height} UYVY {} bytes, converted to BGRA before timing",
        uyvy.len()
    );
    Frame {
        image,
        width,
        height,
        bgra,
    }
}

struct Args {
    image: Option<String>,
    res: Option<String>,
    synthetic: bool,
    width: Option<i32>,
    height: Option<i32>,
    iters: usize,
    prime: usize,
    profiles: Vec<(Profile, &'static str)>,
    require_gpu: bool,
    probe: bool,
    header: bool,
}

fn usage() -> &'static str {
    "\
gpu_bench — GPU decode/encode versus the CPU path that replaces it

  cargo run --release --example gpu_bench --features wgpu -- [OPTIONS]
  cargo run --release --example gpu_bench --features wgpu -- [WIDTH HEIGHT ITERS]

Options:
  --image PATH|NAME   gzip or raw UYVY. NAME is resolved under bench/images/
                      (natural, bars, 1080p-natural, 2160p-bars, ...).
  --res 1080|2160     resolution when NAME is natural or bars
  --synthetic         in-memory gradient (default when --image is omitted)
  --width PX --height PX
  --iters N           timed samples per case (default 20). Warm-up is excluded.
  --prime N           untimed GPU-path iterations before timing (default 20)
  --profile NAME      OMT_LQ, OMT_SQ, OMT_HQ, or all (default OMT_SQ)
  --require-gpu       exit 2 if the adapter is llvmpipe or a CPU device
  --probe             print the adapter and exit before measuring
  --no-header         omit the CSV header (rows still go to stdout)
"
}

fn parse_i32(flag: &str, value: &str) -> i32 {
    value
        .parse()
        .unwrap_or_else(|_| panic!("{flag} expects an integer, got {value}"))
}

fn parse_profile(value: &str) -> Vec<(Profile, &'static str)> {
    match value.to_ascii_uppercase().as_str() {
        "ALL" => vec![
            (Profile::OmtLq, "OMT_LQ"),
            (Profile::OmtSq, "OMT_SQ"),
            (Profile::OmtHq, "OMT_HQ"),
        ],
        "OMT_LQ" => vec![(Profile::OmtLq, "OMT_LQ")],
        "OMT_SQ" => vec![(Profile::OmtSq, "OMT_SQ")],
        "OMT_HQ" => vec![(Profile::OmtHq, "OMT_HQ")],
        _ => panic!("--profile must be OMT_LQ, OMT_SQ, OMT_HQ, or all (got {value})"),
    }
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> String {
    *index += 1;
    args.get(*index)
        .cloned()
        .unwrap_or_else(|| panic!("{flag} needs a value"))
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{usage}", usage = usage());
        std::process::exit(0);
    }
    if !raw.is_empty() && raw.iter().all(|arg| !arg.starts_with('-')) {
        let width = raw.first().and_then(|s| s.parse().ok()).unwrap_or(1920);
        let height = raw.get(1).and_then(|s| s.parse().ok()).unwrap_or(1080);
        let iters = raw.get(2).and_then(|s| s.parse().ok()).unwrap_or(20).max(1);
        return Args {
            image: None,
            res: None,
            synthetic: true,
            width: Some(width),
            height: Some(height),
            iters,
            prime: 20,
            profiles: vec![(Profile::OmtSq, "OMT_SQ")],
            require_gpu: false,
            probe: false,
            header: true,
        };
    }

    let mut args = Args {
        image: None,
        res: None,
        synthetic: false,
        width: None,
        height: None,
        iters: 20,
        prime: 20,
        profiles: vec![(Profile::OmtSq, "OMT_SQ")],
        require_gpu: false,
        probe: false,
        header: true,
    };
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--image" => args.image = Some(take_value(&raw, &mut i, "--image")),
            "--res" => args.res = Some(take_value(&raw, &mut i, "--res")),
            "--synthetic" => args.synthetic = true,
            "--width" => {
                args.width = Some(parse_i32("--width", &take_value(&raw, &mut i, "--width")));
            }
            "--height" => {
                args.height = Some(parse_i32("--height", &take_value(&raw, &mut i, "--height")));
            }
            "--iters" => {
                args.iters = take_value(&raw, &mut i, "--iters")
                    .parse()
                    .unwrap_or_else(|_| panic!("--iters expects an integer"));
            }
            "--prime" => {
                args.prime = take_value(&raw, &mut i, "--prime")
                    .parse()
                    .unwrap_or_else(|_| panic!("--prime expects an integer"));
            }
            "--profile" => {
                args.profiles = parse_profile(&take_value(&raw, &mut i, "--profile"));
            }
            "--require-gpu" => args.require_gpu = true,
            "--probe" => args.probe = true,
            "--no-header" => args.header = false,
            other => panic!("unknown argument {other}\n{}", usage()),
        }
        i += 1;
    }
    if args.iters < 1 {
        panic!("--iters must be at least 1");
    }
    if args.image.is_some() && args.synthetic {
        panic!("pass either --image or --synthetic");
    }
    if args.width.is_some() != args.height.is_some() {
        panic!("pass both --width and --height");
    }
    args
}

fn cpu_adapter(info: &wgpu::AdapterInfo) -> bool {
    let name = info.name.to_ascii_lowercase();
    info.device_type == wgpu::DeviceType::Cpu
        || name.contains("llvmpipe")
        || name.contains("lavapipe")
        || name.contains("swiftshader")
}

fn frames_for(args: &Args) -> Vec<(Frame, usize)> {
    if let Some(image) = &args.image {
        let frame = load_uyvy_frame(image, args.res.as_deref(), args.width, args.height);
        return vec![(frame, args.iters)];
    }
    let width = args.width.unwrap_or(1920);
    let height = args.height.unwrap_or(1080);
    let mut frames = vec![(synthetic_bgra(width, height), args.iters)];
    if width == 1920 && height == 1080 {
        frames.push((synthetic_bgra(3840, 2160), (args.iters / 2).max(4)));
    }
    frames
}

fn main() {
    let args = parse_args();
    let Some((_inst, adapter, device, queue)) = gpu::request_headless_device() else {
        eprintln!("no wgpu adapter");
        std::process::exit(1);
    };

    let info = adapter.get_info();
    eprintln!(
        "adapter={} backend={:?} device_type={:?} bgra8_storage={}",
        info.name,
        info.backend,
        info.device_type,
        adapter
            .features()
            .contains(wgpu::Features::BGRA8UNORM_STORAGE)
    );
    if args.require_gpu && cpu_adapter(&info) {
        eprintln!(
            "refusing to benchmark: adapter {:?} ({}) is llvmpipe or a CPU device. On the L4 host install the NVIDIA driver, libnvidia-gl-580-server, and libegl1.",
            info.device_type, info.name
        );
        std::process::exit(2);
    }
    if args.probe {
        return;
    }

    let mut out = std::io::stdout().lock();
    if args.header {
        writeln!(
            out,
            "res,image,profile,case,ms_median,fps_median,ms_samples"
        )
        .expect("write csv header");
    }
    for (frame, iters) in frames_for(&args) {
        for (profile, profile_name) in &args.profiles {
            eprintln!(
                "gpu_bench image={} profile={profile_name} iters={iters} (median over timed samples, warm-up excluded)",
                frame.image
            );
            bench_resolution(
                &device,
                &queue,
                &frame,
                *profile,
                profile_name,
                Pass {
                    iters,
                    prime: args.prime,
                },
                &mut out,
            );
            eprintln!();
        }
    }
}
