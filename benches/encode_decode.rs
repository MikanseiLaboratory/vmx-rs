use criterion::{Criterion, criterion_group, criterion_main};
use vmx::{Codec, Config, Profile};

fn encode_decode_1080p(c: &mut Criterion) {
    let width = 1920i32;
    let height = 1088i32; // aligned
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

    c.bench_function("vmx_encode_uyvy_1080p", |b| {
        b.iter(|| {
            let mut enc = Codec::new(Config {
                width,
                height: 1080,
                profile: Profile::OmtHq,
                color_space: Default::default(),
            })
            .unwrap();
            enc.encode_uyvy(&frame, stride).unwrap();
            let mut buf = vec![0u8; 8 << 20];
            let _ = enc.save_to(&mut buf).unwrap();
        })
    });
}

criterion_group!(benches, encode_decode_1080p);
criterion_main!(benches);
