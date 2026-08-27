#![cfg(feature = "wgpu")]

use vmx::{Codec, Config, Profile, gpu};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    gpu::request_headless_device().map(|(_, _, d, q)| (d, q))
}

fn sample_bgra(width: i32, height: i32) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut bgra = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let o = y * stride + x * 4;
            bgra[o] = (x.wrapping_mul(13) % 220 + 16) as u8;
            bgra[o + 1] = (y.wrapping_mul(7) % 200 + 20) as u8;
            bgra[o + 2] = ((x + y) % 180 + 30) as u8;
            bgra[o + 3] = 255;
        }
    }
    bgra
}

#[test]
fn gpu_decode_matches_cpu_bgra() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 64i32;
    let height = 64i32;
    let stride = width as usize * 4;
    let src = sample_bgra(width, height);
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtHq,
        color_space: Default::default(),
    })
    .unwrap();
    enc.encode_bgra(&src, stride).unwrap();
    let mut bitstream = vec![0u8; 2 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut cpu = Codec::new(Config::new(width, height)).unwrap();
    cpu.load_from(&bitstream[..len]).unwrap();
    let mut cpu_out = vec![0u8; stride * height as usize];
    cpu.decode_bgra(&mut cpu_out, stride).unwrap();

    let mut gpu_dec = Codec::new(Config::new(width, height)).unwrap();
    gpu_dec.load_from(&bitstream[..len]).unwrap();
    let frame = gpu_dec.decode_to_texture(&device, &queue).unwrap();
    let gpu_out =
        gpu::read_texture_bgra(&device, &queue, &frame.texture, frame.width, frame.height).unwrap();
    assert_eq!(cpu_out, gpu_out, "GPU decode must match CPU decode_bgra");
}

#[test]
fn gpu_preview_matches_cpu() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 128i32;
    let height = 128i32;
    let stride = width as usize * 4;
    let src = sample_bgra(width, height);
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtSq,
        color_space: Default::default(),
    })
    .unwrap();
    enc.encode_bgra(&src, stride).unwrap();
    let mut bitstream = vec![0u8; 2 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut cpu = Codec::new(Config::new(width, height)).unwrap();
    cpu.load_from(&bitstream[..len]).unwrap();
    let ps = cpu.preview_size();
    let pstride = ps.width as usize * 4;
    let mut cpu_out = vec![0u8; pstride * ps.height as usize];
    cpu.decode_preview_bgra(&mut cpu_out, pstride).unwrap();

    let mut gpu_dec = Codec::new(Config::new(width, height)).unwrap();
    gpu_dec.load_from(&bitstream[..len]).unwrap();
    let frame = gpu_dec.decode_preview_to_texture(&device, &queue).unwrap();
    let gpu_out =
        gpu::read_texture_bgra(&device, &queue, &frame.texture, frame.width, frame.height).unwrap();
    assert_eq!(cpu_out, gpu_out);
}

#[test]
fn gpu_encode_bitstream_matches_cpu() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 32i32;
    let height = 32i32;
    let stride = width as usize * 4;
    let src = sample_bgra(width, height);

    let mut cpu = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtHq,
        color_space: Default::default(),
    })
    .unwrap();
    cpu.encode_bgra(&src, stride).unwrap();
    let mut cpu_bs = vec![0u8; 1 << 20];
    let cpu_len = cpu.save_to(&mut cpu_bs).unwrap();

    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);
    let mut gpu_enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtHq,
        color_space: Default::default(),
    })
    .unwrap();
    gpu_enc.encode_from_texture(&device, &queue, &tex).unwrap();
    let mut gpu_bs = vec![0u8; 1 << 20];
    let gpu_len = gpu_enc.save_to(&mut gpu_bs).unwrap();
    assert_eq!(&cpu_bs[..cpu_len], &gpu_bs[..gpu_len]);
}

#[test]
fn gpu_idct_single_block_via_frame() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 16i32;
    let height = 16i32;
    let stride = width as usize * 4;
    let src = sample_bgra(width, height);
    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtHq,
        color_space: Default::default(),
    })
    .unwrap();
    enc.encode_bgra(&src, stride).unwrap();
    let mut bitstream = vec![0u8; 1 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut cpu = Codec::new(Config::new(width, height)).unwrap();
    cpu.load_from(&bitstream[..len]).unwrap();
    let mut cpu_out = vec![0u8; stride * height as usize];
    cpu.decode_bgra(&mut cpu_out, stride).unwrap();

    let mut gpu_dec = Codec::new(Config::new(width, height)).unwrap();
    gpu_dec.load_from(&bitstream[..len]).unwrap();
    let frame = gpu_dec.decode_to_texture(&device, &queue).unwrap();
    let gpu_out =
        gpu::read_texture_bgra(&device, &queue, &frame.texture, frame.width, frame.height).unwrap();
    assert_eq!(cpu_out, gpu_out);
}
