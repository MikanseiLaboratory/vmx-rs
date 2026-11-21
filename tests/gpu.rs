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

fn assert_psnr(a: &[u8], b: &[u8], min_db: f64, label: &str) {
    let psnr = psnr_bgra(a, b);
    eprintln!("{label}: PSNR {psnr:.2} dB (min {min_db:.1} dB)");
    assert!(
        psnr >= min_db,
        "{label}: PSNR {psnr:.2} dB is below {min_db:.1} dB"
    );
}

#[test]
fn gpu_decode_matches_cpu_quality() {
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
    assert_psnr(&cpu_out, &gpu_out, 28.0, "GPU decode vs CPU");
}

#[test]
fn gpu_exact_decode_matches_cpu_bytes() {
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
    let frame = gpu_dec.decode_to_texture_exact(&device, &queue).unwrap();
    let gpu_out =
        gpu::read_texture_bgra(&device, &queue, &frame.texture, frame.width, frame.height).unwrap();
    assert_eq!(cpu_out, gpu_out, "exact GPU decode must be byte-identical");
}

fn bgra_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC;
    if device
        .features()
        .contains(wgpu::Features::BGRA8UNORM_STORAGE)
    {
        usage |= wgpu::TextureUsages::STORAGE_BINDING;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test-bgra"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage,
        view_formats: &[],
    })
}

#[test]
fn gpu_decode_into_matches_allocated() {
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
    let tex = bgra_texture(&device, width as u32, height as u32);
    gpu_dec
        .decode_to_texture_exact_into(&device, &queue, &tex)
        .unwrap();
    let gpu_out =
        gpu::read_texture_bgra(&device, &queue, &tex, width as u32, height as u32).unwrap();
    assert_eq!(cpu_out, gpu_out);
}

#[test]
fn gpu_encode_from_view_matches_sync() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 32i32;
    let height = 32i32;
    let src = sample_bgra(width, height);
    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);

    let mut sync = Codec::new(Config::new(width, height)).unwrap();
    sync.encode_from_texture_opaque(&device, &queue, &tex)
        .unwrap();
    let mut sync_bs = vec![0u8; 1 << 20];
    let sync_len = sync.save_to(&mut sync_bs).unwrap();

    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut viewed = Codec::new(Config::new(width, height)).unwrap();
    viewed
        .encode_from_texture_view(&device, &queue, &tex, &view, false)
        .unwrap();
    let mut viewed_bs = vec![0u8; 1 << 20];
    let viewed_len = viewed.save_to(&mut viewed_bs).unwrap();
    assert_eq!(&sync_bs[..sync_len], &viewed_bs[..viewed_len]);
}

#[test]
fn gpu_encode_three_frames_in_flight() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 32i32;
    let height = 32i32;
    let src = sample_bgra(width, height);
    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);

    let mut sync = Codec::new(Config::new(width, height)).unwrap();
    sync.encode_from_texture_opaque(&device, &queue, &tex)
        .unwrap();
    let mut sync_bs = vec![0u8; 1 << 20];
    let sync_len = sync.save_to(&mut sync_bs).unwrap();

    let mut piped = Codec::new(Config::new(width, height)).unwrap();
    for _ in 0..3 {
        piped.set_quality(80);
        piped
            .encode_from_texture_submit(&device, &queue, &tex, false)
            .unwrap();
    }
    for _ in 0..3 {
        piped.encode_submitted_finish(&device).unwrap();
        piped.set_quality(80);
        let mut bs = vec![0u8; 1 << 20];
        let len = piped.save_to(&mut bs).unwrap();
        assert_eq!(&sync_bs[..sync_len], &bs[..len]);
    }
}

#[test]
fn gpu_stats_are_reported() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 32i32;
    let height = 32i32;
    let stride = width as usize * 4;
    let src = sample_bgra(width, height);
    let mut enc = Codec::new(Config::new(width, height)).unwrap();
    enc.encode_bgra(&src, stride).unwrap();
    let mut bitstream = vec![0u8; 1 << 20];
    let len = enc.save_to(&mut bitstream).unwrap();

    let mut dec = Codec::new(Config::new(width, height)).unwrap();
    dec.load_from(&bitstream[..len]).unwrap();
    let _ = dec.decode_to_texture(&device, &queue).unwrap();
    let stats = dec.gpu_stats().expect("stats");
    assert!(stats.decode_pack_ms >= 0.0);
    assert!(stats.decode_dispatch_ms >= 0.0);

    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);
    let mut gpu_enc = Codec::new(Config::new(width, height)).unwrap();
    gpu_enc
        .encode_from_texture_opaque(&device, &queue, &tex)
        .unwrap();
    let stats = gpu_enc.gpu_stats().expect("stats");
    assert!(stats.encode_submit_ms >= 0.0);
    assert!(stats.encode_finish_ms >= 0.0);
}

#[test]
fn gpu_preview_matches_cpu_quality() {
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
    assert_psnr(&cpu_out, &gpu_out, 40.0, "GPU preview vs CPU");
}

#[test]
fn gpu_decode_does_not_overwrite_returned_texture() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 64i32;
    let height = 64i32;
    let stride = width as usize * 4;
    let src_a = sample_bgra(width, height);
    let src_b: Vec<u8> = src_a.iter().map(|b| b.wrapping_add(90)).collect();

    let encode = |src: &[u8]| {
        let mut enc = Codec::new(Config {
            width,
            height,
            profile: Profile::OmtHq,
            color_space: Default::default(),
        })
        .unwrap();
        enc.encode_bgra(src, stride).unwrap();
        let mut bitstream = vec![0u8; 2 << 20];
        let len = enc.save_to(&mut bitstream).unwrap();
        bitstream.truncate(len);
        bitstream
    };
    let a_bs = encode(&src_a);
    let b_bs = encode(&src_b);

    let mut dec = Codec::new(Config::new(width, height)).unwrap();
    dec.load_from(&a_bs).unwrap();
    let frame_a = dec.decode_to_texture(&device, &queue).unwrap();
    dec.load_from(&b_bs).unwrap();
    let frame_b = dec.decode_to_texture(&device, &queue).unwrap();

    let mut cpu = Codec::new(Config::new(width, height)).unwrap();
    cpu.load_from(&a_bs).unwrap();
    let mut cpu_a = vec![0u8; stride * height as usize];
    cpu.decode_bgra(&mut cpu_a, stride).unwrap();
    cpu.load_from(&b_bs).unwrap();
    let mut cpu_b = vec![0u8; stride * height as usize];
    cpu.decode_bgra(&mut cpu_b, stride).unwrap();

    let gpu_a = gpu::read_texture_bgra(
        &device,
        &queue,
        &frame_a.texture,
        frame_a.width,
        frame_a.height,
    )
    .unwrap();
    let gpu_b = gpu::read_texture_bgra(
        &device,
        &queue,
        &frame_b.texture,
        frame_b.width,
        frame_b.height,
    )
    .unwrap();
    assert_psnr(&cpu_a, &gpu_a, 28.0, "held frame A after decoding B");
    assert_psnr(&cpu_b, &gpu_b, 28.0, "frame B");
}

#[test]
fn gpu_encode_roundtrip_quality() {
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
    let mut cpu_dec = Codec::new(Config::new(width, height)).unwrap();
    cpu_dec.load_from(&cpu_bs[..cpu_len]).unwrap();
    let mut cpu_out = vec![0u8; stride * height as usize];
    cpu_dec.decode_bgra(&mut cpu_out, stride).unwrap();

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
    assert_eq!(
        &cpu_bs[..cpu_len],
        &gpu_bs[..gpu_len],
        "GPU encode must be byte-identical to CPU encode"
    );
    let mut gpu_dec = Codec::new(Config::new(width, height)).unwrap();
    gpu_dec.load_from(&gpu_bs[..gpu_len]).unwrap();
    let mut gpu_out = vec![0u8; stride * height as usize];
    gpu_dec.decode_bgra(&mut gpu_out, stride).unwrap();
    assert_psnr(
        &cpu_out,
        &gpu_out,
        35.0,
        "GPU encode vs CPU encode (CPU decode)",
    );
}

#[test]
fn gpu_opaque_encode_matches_cpu_bgrx() {
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
    cpu.encode_bgrx(&src, stride).unwrap();
    let mut cpu_bs = vec![0u8; 1 << 20];
    let cpu_len = cpu.save_to(&mut cpu_bs).unwrap();
    let mut cpu_dec = Codec::new(Config::new(width, height)).unwrap();
    cpu_dec.load_from(&cpu_bs[..cpu_len]).unwrap();
    let mut cpu_out = vec![0u8; stride * height as usize];
    cpu_dec.decode_bgra(&mut cpu_out, stride).unwrap();

    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);
    let mut gpu_enc = Codec::new(Config {
        width,
        height,
        profile: Profile::OmtHq,
        color_space: Default::default(),
    })
    .unwrap();
    gpu_enc
        .encode_from_texture_opaque(&device, &queue, &tex)
        .unwrap();
    let mut gpu_bs = vec![0u8; 1 << 20];
    let gpu_len = gpu_enc.save_to(&mut gpu_bs).unwrap();
    assert_eq!(
        &cpu_bs[..cpu_len],
        &gpu_bs[..gpu_len],
        "GPU opaque encode must be byte-identical to CPU BGRX encode"
    );
    let mut gpu_dec = Codec::new(Config::new(width, height)).unwrap();
    gpu_dec.load_from(&gpu_bs[..gpu_len]).unwrap();
    let mut gpu_out = vec![0u8; stride * height as usize];
    gpu_dec.decode_bgra(&mut gpu_out, stride).unwrap();
    assert_psnr(
        &cpu_out,
        &gpu_out,
        35.0,
        "GPU opaque encode vs CPU BGRX (CPU decode)",
    );
}

#[test]
fn gpu_encode_submit_finish_matches_sync() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 32i32;
    let height = 32i32;
    let src = sample_bgra(width, height);
    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);

    let mut sync = Codec::new(Config::new(width, height)).unwrap();
    sync.encode_from_texture_opaque(&device, &queue, &tex)
        .unwrap();
    let mut sync_bs = vec![0u8; 1 << 20];
    eprintln!("sync q before save={}", sync.quality());
    let sync_len = sync.save_to(&mut sync_bs).unwrap();
    eprintln!("sync q after save={}", sync.quality());

    let mut piped = Codec::new(Config::new(width, height)).unwrap();
    eprintln!("piped q new={}", piped.quality());
    piped
        .encode_from_texture_submit(&device, &queue, &tex, false)
        .unwrap();
    piped.encode_submitted_finish(&device).unwrap();
    let mut piped_bs = vec![0u8; 1 << 20];
    let piped_len = piped.save_to(&mut piped_bs).unwrap();
    assert_eq!(&sync_bs[..sync_len], &piped_bs[..piped_len]);
}

#[test]
fn gpu_idct_single_block_quality() {
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
    assert_psnr(&cpu_out, &gpu_out, 28.0, "16x16 GPU decode vs CPU");
}

#[test]
fn gpu_encode_header_quality_follows_submit_time() {
    let Some((device, queue)) = device() else {
        eprintln!("skip: no wgpu adapter");
        return;
    };
    let width = 64i32;
    let height = 64i32;
    let stride = width as usize * 4;
    let src = sample_bgra(width, height);
    let tex = gpu::upload_bgra_texture(&device, &queue, width as u32, height as u32, &src);

    let mut enc = Codec::new(Config::new(width, height)).unwrap();
    enc.set_quality(90);
    let submitted = enc.quality();
    enc.encode_from_texture_submit(&device, &queue, &tex, false)
        .unwrap();
    // A peer quality suggestion lands between submit and finish. The header
    // must keep describing the coefficients that were already quantized.
    enc.set_quality(40);
    let later = enc.quality();
    eprintln!("submitted={submitted} later={later}");
    assert_ne!(later, submitted, "test needs a distinct quality step");
    enc.encode_submitted_finish(&device).unwrap();
    assert_eq!(
        enc.quality(),
        submitted,
        "finish must restore the submit-time quality"
    );
    let mut bs = vec![0u8; 1 << 20];
    let len = enc.save_to(&mut bs).unwrap();

    let mut dec = Codec::new(Config::new(width, height)).unwrap();
    dec.load_from(&bs[..len]).unwrap();
    assert_eq!(
        dec.quality(),
        submitted,
        "header quality must match the quantization"
    );
    let mut out = vec![0u8; stride * height as usize];
    dec.decode_bgra(&mut out, stride).unwrap();
    assert_psnr(&src, &out, 20.0, "mid-pipeline quality change");
}
