//! Compare CPU decode+upload vs GPU decode-to-texture (1080p).

use std::time::Instant;

use vmx::{Codec, Config, Profile, gpu};

fn main() {
    let Some((_inst, _adapt, device, queue)) = gpu::request_headless_device() else {
        eprintln!("no wgpu adapter; skip");
        return;
    };

    let width = 1920i32;
    let height = 1080i32;
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
    let mut bitstream = vec![0u8; 8 << 20];
    let len = enc.save_to(&mut bitstream).expect("save");

    let mut cpu = Codec::new(Config::new(width, height)).expect("cpu");
    cpu.load_from(&bitstream[..len]).expect("load");
    let mut cpu_out = vec![0u8; stride * height as usize];
    let t0 = Instant::now();
    cpu.decode_bgra(&mut cpu_out, stride).expect("cpu decode");
    let cpu_decode = t0.elapsed();
    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &cpu_out);
    let cpu_upload = t0.elapsed();
    let _ = tex;

    let mut gpu_dec = Codec::new(Config::new(width, height)).expect("gpu codec");
    gpu_dec.load_from(&bitstream[..len]).expect("load");
    let t1 = Instant::now();
    let frame = gpu_dec
        .decode_to_texture(&device, &queue)
        .expect("gpu decode");
    let gpu_elapsed = t1.elapsed();

    eprintln!(
        "1080p CPU decode: {:.3} ms, CPU decode+write_texture: {:.3} ms, GPU decode_to_texture: {:.3} ms ({}x{})",
        cpu_decode.as_secs_f64() * 1e3,
        cpu_upload.as_secs_f64() * 1e3,
        gpu_elapsed.as_secs_f64() * 1e3,
        frame.width,
        frame.height
    );
}
