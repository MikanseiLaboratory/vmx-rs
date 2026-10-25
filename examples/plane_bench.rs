//! Times the CPU encode/decode of one VMX1 stream, and the GPU decode's CPU part.
//!
//! ```text
//! cargo run --release --example plane_bench -- 1920 1080 60
//! cargo run --release --features wgpu --example plane_bench -- 1920 1080 60
//! ```
//!
//! The point is the per-frame cost: the mixer records one stream per angle, so the
//! number of angles a machine can record is the frame budget divided by it.

use std::time::Instant;

use vmx::{Codec, ColorSpace, Config, Profile};

fn frame(width: u32, height: u32, noise: bool) -> Vec<u8> {
    let mut pixels = vec![128u8; (width * height * 2) as usize];
    for (index, byte) in pixels.iter_mut().enumerate() {
        let value = if noise {
            // Worst case: every byte unrelated to its neighbour, chroma included.
            // This is what a synthetic "random" test frame costs; a camera is not
            // this expensive, but a broken source can be.
            (index % 251) as u8
        } else if index % 4 < 2 {
            // Luma: a gradient that moves down the frame. Chroma stays flat, which
            // is what a normal picture looks like once it is 4:2:2.
            ((index / 2) % 251) as u8
        } else {
            128
        };
        *byte = value;
    }
    pixels
}

fn ms(elapsed: std::time::Duration, frames: u32) -> f64 {
    elapsed.as_secs_f64() * 1000.0 / f64::from(frames.max(1))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let width: u32 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(1920);
    let height: u32 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(1080);
    let frames: u32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(60);
    let noise = args.get(4).map(|value| value == "noise").unwrap_or(false);
    let streams: usize = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mode = args.get(6).cloned().unwrap_or_else(|| "both".into());
    let pixels = frame(width, height, noise);
    let stride = (width * 2) as usize;
    println!(
        "{width}x{height} profile=SQ frames={frames} pattern={}",
        if noise { "noise" } else { "picture" }
    );

    // Whole-frame CPU encode, for a few thread counts. 0 asks for the default the
    // bitrate table picks.
    for threads in [0usize, 1, 2, 4, 6, 8, 12, 24] {
        let mut codec = Codec::new(Config {
            width: width as i32,
            height: height as i32,
            profile: Profile::Sq,
            color_space: ColorSpace::Bt709,
        })
        .expect("encoder");
        if threads > 0 {
            codec.set_threads(threads);
        }
        let mut packet = vec![0u8; 8 * 1024 * 1024];
        // One warm-up frame: slice state and the first allocation are not per frame.
        codec.encode_uyvy(&pixels, stride).expect("warm-up");
        let _ = codec.save_to(&mut packet);
        let started = Instant::now();
        let mut bytes = 0u64;
        for _ in 0..frames {
            codec.encode_uyvy(&pixels, stride).expect("encode");
            loop {
                match codec.save_to(&mut packet) {
                    Ok(n) => {
                        bytes += n as u64;
                        break;
                    }
                    Err(_) => packet.resize(packet.len() * 2, 0),
                }
            }
        }
        let elapsed = started.elapsed();
        println!(
            "cpu encode   threads={threads:<3} {:6.2} ms/frame  {:6.1} fps  {:5.0} Mbps",
            ms(elapsed, frames),
            1000.0 / ms(elapsed, frames),
            bytes as f64 * 8.0 / elapsed.as_secs_f64() / 1_000_000.0
        );
    }

    // CPU decode of the same bitstream, so encode and decode are comparable.
    let mut encoder = Codec::new(Config {
        width: width as i32,
        height: height as i32,
        profile: Profile::Sq,
        color_space: ColorSpace::Bt709,
    })
    .expect("encoder");
    encoder.encode_uyvy(&pixels, stride).expect("encode");
    let mut packet = vec![0u8; 8 * 1024 * 1024];
    let length = loop {
        match encoder.save_to(&mut packet) {
            Ok(n) => break n,
            Err(_) => packet.resize(packet.len() * 2, 0),
        }
    };
    for threads in [0usize, 1, 2, 4, 6, 8, 12, 24] {
        let mut codec = Codec::new(Config {
            width: width as i32,
            height: height as i32,
            profile: Profile::Sq,
            color_space: ColorSpace::Bt709,
        })
        .expect("decoder");
        if threads > 0 {
            codec.set_threads(threads);
        }
        let mut out = vec![0u8; pixels.len()];
        codec.load_from(&packet[..length]).expect("warm-up load");
        codec.decode_uyvy(&mut out, stride).expect("warm-up");
        let started = Instant::now();
        for _ in 0..frames {
            codec.load_from(&packet[..length]).expect("load");
            codec.decode_uyvy(&mut out, stride).expect("decode");
        }
        let elapsed = started.elapsed();
        println!(
            "cpu decode   threads={threads:<3} {:6.2} ms/frame  {:6.1} fps",
            ms(elapsed, frames),
            1000.0 / ms(elapsed, frames)
        );
    }

    #[cfg(feature = "wgpu")]
    {
        if let Some((_instance, _adapter, device, queue)) = vmx::gpu::request_headless_device() {
            let mut codec = Codec::new(Config {
                width: width as i32,
                height: height as i32,
                profile: Profile::Sq,
                color_space: ColorSpace::Bt709,
            })
            .expect("gpu decoder");
            codec.load_from(&packet[..length]).expect("gpu load");
            // The first decode builds the compute pipelines.
            let _ = codec
                .decode_to_texture(&device, &queue)
                .expect("gpu warm-up");
            let started = Instant::now();
            for _ in 0..frames {
                codec.load_from(&packet[..length]).expect("gpu load");
                let _ = codec
                    .decode_to_texture(&device, &queue)
                    .expect("gpu decode");
            }
            let elapsed = started.elapsed();
            println!(
                "gpu decode   (entropy on the CPU, transform on the device) {:6.2} ms/frame  {:6.1} fps",
                ms(elapsed, frames),
                1000.0 / ms(elapsed, frames)
            );
        }
    }

    // Several streams at once is what a replay recorder actually runs: one encoder
    // per angle. The aggregate number is what decides how many angles fit.
    if streams > 0 {
        for threads in [0usize, 3, 6] {
            run_streams(width, height, frames, streams, threads, noise, &mode);
        }
    }
}

/// Runs `count` independent encode+decode pairs at the same time and reports the
/// aggregate rate. Each stream has its own codec, exactly like one angle.
fn run_streams(
    width: u32,
    height: u32,
    frames: u32,
    count: usize,
    threads: usize,
    noise: bool,
    mode: &str,
) {
    let stride = (width * 2) as usize;
    let pixels = std::sync::Arc::new(frame(width, height, noise));
    let mode = mode.to_string();
    let started = Instant::now();
    let mut handles = Vec::with_capacity(count);
    for stream in 0..count {
        let pixels = std::sync::Arc::clone(&pixels);
        // "mixed" is the workflow that matters: most streams record, a few play
        // back. The first half encodes, the rest decodes.
        let mode = if mode == "mixed" {
            if stream < count / 2 {
                "encode".to_string()
            } else {
                "decode".to_string()
            }
        } else {
            mode.clone()
        };
        handles.push(std::thread::spawn(move || {
            let mut codec = Codec::new(Config {
                width: width as i32,
                height: height as i32,
                profile: Profile::Sq,
                color_space: ColorSpace::Bt709,
            })
            .expect("stream encoder");
            if threads > 0 {
                codec.set_threads(threads);
            }
            let mut packet = vec![0u8; 8 * 1024 * 1024];
            let mut decoded = vec![0u8; pixels.len()];
            // Warm-up: pool threads, slice state and the first allocation.
            codec.encode_uyvy(&pixels, stride).expect("warm-up");
            let length = loop {
                match codec.save_to(&mut packet) {
                    Ok(n) => break n,
                    Err(_) => packet.resize(packet.len() * 2, 0),
                }
            };
            codec.load_from(&packet[..length]).expect("warm-up load");
            codec
                .decode_uyvy(&mut decoded, stride)
                .expect("warm-up decode");
            // Per-frame times, so the report can say how much headroom a frame has
            // rather than only what the average is.
            let mut timings = Vec::with_capacity(frames as usize);
            for _ in 0..frames {
                let frame_started = Instant::now();
                if mode != "decode" {
                    codec.encode_uyvy(&pixels, stride).expect("encode");
                    let saved = loop {
                        match codec.save_to(&mut packet) {
                            Ok(n) => break n,
                            Err(_) => packet.resize(packet.len() * 2, 0),
                        }
                    };
                    if mode == "both" {
                        codec.load_from(&packet[..saved]).expect("load");
                    }
                }
                if mode != "encode" {
                    codec.load_from(&packet[..length]).expect("load");
                    codec.decode_uyvy(&mut decoded, stride).expect("decode");
                }
                timings.push(frame_started.elapsed().as_secs_f64() * 1000.0);
            }
            timings
        }));
    }
    let mut timings = Vec::with_capacity(frames as usize * count);
    for handle in handles {
        timings.extend(handle.join().expect("stream thread"));
    }
    let elapsed = started.elapsed();
    let total = f64::from(frames) * count as f64;
    timings.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |fraction: f64| -> f64 {
        if timings.is_empty() {
            return 0.0;
        }
        let index = ((timings.len() as f64 - 1.0) * fraction).round() as usize;
        timings[index]
    };
    println!(
        "streams={count:<2} threads={threads:<3} {:7.1} fps aggregate  per-frame p50={:5.2} p99={:5.2} max={:6.2} ms (60fps budget 16.67)",
        total / elapsed.as_secs_f64(),
        pick(0.50),
        pick(0.99),
        timings.last().copied().unwrap_or(0.0)
    );
}
