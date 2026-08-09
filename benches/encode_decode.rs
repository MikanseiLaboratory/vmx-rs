use criterion::{Criterion, criterion_group, criterion_main};
use vmx::{Codec, Config, Profile};

fn make_uyvy(width: i32, height: i32) -> (Vec<u8>, usize) {
    let stride = (width as usize) * 2;
    let mut frame = vec![128u8; stride * height as usize];
    for y in 0..height as usize {
        for x in (0..width as usize).step_by(2) {
            let o = y * stride + x * 2;
            frame[o] = 128;
            frame[o + 1] = 16;
            frame[o + 2] = 128;
            frame[o + 3] = 16;
        }
    }
    (frame, stride)
}

fn encode_uyvy_profile(c: &mut Criterion, name: &str, width: i32, height: i32, profile: Profile) {
    let (frame, stride) = make_uyvy(width, height);
    let mut enc = Codec::new(Config {
        width,
        height,
        profile,
        color_space: Default::default(),
    })
    .unwrap();
    let mut buf = vec![0u8; 8 << 20];

    c.bench_function(name, |b| {
        b.iter(|| {
            enc.encode_uyvy(&frame, stride).unwrap();
            let _ = enc.save_to(&mut buf).unwrap();
        })
    });
}

fn benches(c: &mut Criterion) {
    encode_uyvy_profile(c, "vmx_encode_uyvy_720p_omt_lq", 1280, 720, Profile::OmtLq);
    encode_uyvy_profile(
        c,
        "vmx_encode_uyvy_1080p_omt_hq",
        1920,
        1080,
        Profile::OmtHq,
    );
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
