//! Per-instruction-family microbenchmarks (scalar vs live SIMD).
//!
//! ```bash
//! cargo bench --bench simd_paths -- --warm-up-time 2 --measurement-time 5
//! ```

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use vmx::kernels::{Size, fdct_quant_zig, planar_to_uyvy_scalar, uyvy_to_planar_scalar};

#[cfg(target_arch = "x86_64")]
use vmx::kernels::{fdct_quant_zig_sse, planar_to_uyvy_sse2, uyvy_to_planar_ssse3};

const W: i32 = 1280;
const H: i32 = 720;

fn make_uyvy(width: i32, height: i32) -> (Vec<u8>, usize) {
    let stride = (width as usize) * 2;
    let mut frame = vec![0u8; stride * height as usize];
    for (i, b) in frame.iter_mut().enumerate() {
        *b = ((i * 37 + 11) % 256) as u8;
    }
    (frame, stride)
}

fn plane_bufs(width: i32, height: i32) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize) {
    let y_stride = width as usize;
    let u_stride = (width / 2) as usize;
    let y = vec![0u8; y_stride * height as usize];
    let u = vec![128u8; u_stride * height as usize];
    let v = vec![128u8; u_stride * height as usize];
    (y, u, v, y_stride, u_stride)
}

fn bench_uyvy_to_planar(c: &mut Criterion) {
    let size = Size::new(W, H);
    let (src, stride) = make_uyvy(W, H);
    let (mut y, mut u, mut v, y_stride, u_stride) = plane_bufs(W, H);

    c.bench_function("uyvy_to_planar/1280x720/scalar", |b| {
        b.iter(|| {
            uyvy_to_planar_scalar(
                black_box(&src),
                stride,
                black_box(&mut y),
                y_stride,
                black_box(&mut u),
                u_stride,
                black_box(&mut v),
                u_stride,
                size,
            );
        })
    });

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("ssse3") {
        c.bench_function("uyvy_to_planar/1280x720/ssse3", |b| {
            b.iter(|| unsafe {
                uyvy_to_planar_ssse3(
                    black_box(&src),
                    stride,
                    black_box(&mut y),
                    y_stride,
                    black_box(&mut u),
                    u_stride,
                    black_box(&mut v),
                    u_stride,
                    size,
                );
            })
        });
    }
}

fn bench_planar_to_uyvy(c: &mut Criterion) {
    let size = Size::new(W, H);
    let (src, stride) = make_uyvy(W, H);
    let (mut y, mut u, mut v, y_stride, u_stride) = plane_bufs(W, H);
    uyvy_to_planar_scalar(
        &src, stride, &mut y, y_stride, &mut u, u_stride, &mut v, u_stride, size,
    );
    let mut dst = vec![0u8; stride * H as usize];

    c.bench_function("planar_to_uyvy/1280x720/scalar", |b| {
        b.iter(|| {
            planar_to_uyvy_scalar(
                black_box(&y),
                y_stride,
                black_box(&u),
                u_stride,
                black_box(&v),
                u_stride,
                black_box(&mut dst),
                stride,
                size,
            );
        })
    });

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("sse2") {
        c.bench_function("planar_to_uyvy/1280x720/sse2", |b| {
            b.iter(|| unsafe {
                planar_to_uyvy_sse2(
                    black_box(&y),
                    y_stride,
                    black_box(&u),
                    u_stride,
                    black_box(&v),
                    u_stride,
                    black_box(&mut dst),
                    stride,
                    size,
                );
            })
        });
    }
}

fn bench_fdct_quant(c: &mut Criterion) {
    // Full 720p luma plane as 8×8 blocks (matches encode Y workload order of magnitude).
    let width = 1280usize;
    let height = 720usize;
    let stride = width;
    let mut src = vec![0u8; stride * height];
    for (i, b) in src.iter_mut().enumerate() {
        *b = ((i * 41 + 7) % 256) as u8;
    }
    let mut matrix = [0u16; 192];
    for i in 0..64 {
        matrix[i] = 16;
        matrix[64 + i] = u16::MAX;
        matrix[128 + i] = u16::MAX;
    }
    let mut out = [0i16; 64];
    let add_val = -128i16;

    let run_scalar = |src: &[u8], out: &mut [i16; 64]| {
        for y in (0..height).step_by(8) {
            for x in (0..width).step_by(8) {
                let off = y * stride + x;
                fdct_quant_zig(&src[off..], stride, &matrix, add_val, out);
            }
        }
    };

    c.bench_function("fdct_quant_zig/720p_y/scalar", |b| {
        b.iter(|| run_scalar(black_box(&src), black_box(&mut out)))
    });

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("sse4.2") {
        c.bench_function("fdct_quant_zig/720p_y/sse4.2", |b| {
            b.iter(|| unsafe {
                for y in (0..height).step_by(8) {
                    for x in (0..width).step_by(8) {
                        let off = y * stride + x;
                        fdct_quant_zig_sse(
                            black_box(src.as_ptr().add(off)),
                            stride,
                            matrix.as_ptr(),
                            add_val,
                            black_box(&mut out),
                        );
                    }
                }
            })
        });
    }
}

criterion_group!(
    simd_benches,
    bench_uyvy_to_planar,
    bench_planar_to_uyvy,
    bench_fdct_quant
);
criterion_main!(simd_benches);
