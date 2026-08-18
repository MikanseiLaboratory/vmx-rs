//! Portable SIMD (`std::simd`) YUV422 → BGRA conversion.
//!
//! Processes 8 luma samples (4 chroma pairs) per iteration with `Simd<i16, 8>`.

#![cfg(feature = "portable-simd")]

use std::simd::Simd;
use std::simd::prelude::*;

type I16x8 = Simd<i16, 8>;

#[inline(always)]
fn mulhi_i16(a: I16x8, b: I16x8) -> I16x8 {
    let prod = a.cast::<i32>() * b.cast::<i32>();
    (prod >> Simd::<i32, 8>::splat(16)).cast::<i16>()
}

/// Convert 8 Y + 4 U + 4 V samples to 8 opaque BGRA pixels.
#[inline]
pub fn yuv422_macropixels_to_bgra8(y: &[u8], u: &[u8], v: &[u8], dst: &mut [u8], table: &[i16; 5]) {
    debug_assert!(y.len() >= 8 && u.len() >= 4 && v.len() >= 4 && dst.len() >= 32);

    let mut y_sat = [0i16; 8];
    for i in 0..8 {
        y_sat[i] = y[i].saturating_sub(16) as i16;
    }
    let mut y0 = I16x8::from_array(y_sat);
    y0 <<= I16x8::splat(6);
    y0 = mulhi_i16(y0, I16x8::splat(table[0]));

    // Expand 4 chroma samples to 8 lanes (each shared by a pair).
    let mut u0 = I16x8::from_array([
        u[0] as i16 - 128,
        u[0] as i16 - 128,
        u[1] as i16 - 128,
        u[1] as i16 - 128,
        u[2] as i16 - 128,
        u[2] as i16 - 128,
        u[3] as i16 - 128,
        u[3] as i16 - 128,
    ]);
    let mut v0 = I16x8::from_array([
        v[0] as i16 - 128,
        v[0] as i16 - 128,
        v[1] as i16 - 128,
        v[1] as i16 - 128,
        v[2] as i16 - 128,
        v[2] as i16 - 128,
        v[3] as i16 - 128,
        v[3] as i16 - 128,
    ]);

    v0 <<= I16x8::splat(6);
    let mut r = mulhi_i16(v0, I16x8::splat(table[1])).saturating_add(y0);

    let mut b = u0 << I16x8::splat(7);
    b = mulhi_i16(b, I16x8::splat(table[4])).saturating_add(y0);

    u0 <<= I16x8::splat(6);
    let mut g = y0
        .saturating_sub(mulhi_i16(u0, I16x8::splat(table[2])))
        .saturating_sub(mulhi_i16(v0, I16x8::splat(table[3])));

    let rounding = I16x8::splat(8);
    r = (r.saturating_add(rounding)) >> I16x8::splat(4);
    g = (g.saturating_add(rounding)) >> I16x8::splat(4);
    b = (b.saturating_add(rounding)) >> I16x8::splat(4);

    let r = r.simd_clamp(I16x8::splat(0), I16x8::splat(255)).to_array();
    let g = g.simd_clamp(I16x8::splat(0), I16x8::splat(255)).to_array();
    let b = b.simd_clamp(I16x8::splat(0), I16x8::splat(255)).to_array();
    for i in 0..8 {
        let o = i * 4;
        dst[o] = b[i] as u8;
        dst[o + 1] = g[i] as u8;
        dst[o + 2] = r[i] as u8;
        dst[o + 3] = 255;
    }
}

/// YUV422 band → BGRA using portable SIMD (8-pixel steps, scalar tail).
pub fn yuv422_band_to_bgra_portable(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    y_row0: usize,
    rows: usize,
    width: usize,
    dst: &mut [u8],
    dst_stride: usize,
    table: &[i16; 5],
) {
    for row in 0..rows {
        let yr = y_row0 + row;
        let yd = &y[yr * y_stride..];
        let ud = &u[yr * u_stride..];
        let vd = &v[yr * v_stride..];
        let d = &mut dst[yr * dst_stride..];
        let mut x = 0usize;
        let mut px = 0usize;
        while px + 8 <= width {
            yuv422_macropixels_to_bgra8(
                &yd[px..px + 8],
                &ud[x..x + 4],
                &vd[x..x + 4],
                &mut d[px * 4..px * 4 + 32],
                table,
            );
            x += 4;
            px += 8;
        }
        while px + 1 < width {
            let cb = ud[x] as i16 - 128;
            let cr = vd[x] as i16 - 128;
            for i in 0..2 {
                let (b, g, r) = crate::color::convert::yuv_to_bgra_pixel(yd[px + i], cb, cr, table);
                let o = (px + i) * 4;
                d[o] = b;
                d[o + 1] = g;
                d[o + 2] = r;
                d[o + 3] = 255;
            }
            x += 1;
            px += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::convert::yuv422_band_to_bgra_scalar;
    use crate::tables::YUV_RGB_709;

    #[test]
    fn portable_bgra_matches_scalar() {
        let width = 64usize;
        let height = 16usize;
        let y_stride = width;
        let u_stride = width / 2;
        let mut y = vec![0u8; y_stride * height];
        let mut u = vec![0u8; u_stride * height];
        let mut v = vec![0u8; u_stride * height];
        for row in 0..height {
            for x in 0..width {
                y[row * y_stride + x] = (16 + ((x * 3 + row * 5) % 220)) as u8;
            }
            for x in 0..u_stride {
                u[row * u_stride + x] = (80 + ((x + row) % 40)) as u8;
                v[row * u_stride + x] = (100 + ((x * 2 + row) % 40)) as u8;
            }
        }
        let dst_stride = width * 4;
        let mut expected = vec![0u8; dst_stride * height];
        let mut actual = vec![0u8; dst_stride * height];
        let table = &YUV_RGB_709;
        yuv422_band_to_bgra_scalar(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            0,
            height,
            width,
            &mut expected,
            dst_stride,
            table,
        );
        yuv422_band_to_bgra_portable(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            0,
            height,
            width,
            &mut actual,
            dst_stride,
            table,
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn portable_bgra_faster_than_scalar_in_release() {
        if cfg!(debug_assertions) {
            return;
        }
        use std::time::Instant;

        let width = 1920usize;
        let height = 1080usize;
        let y_stride = width;
        let u_stride = width / 2;
        let mut y = vec![0u8; y_stride * height];
        let mut u = vec![0u8; u_stride * height];
        let mut v = vec![0u8; u_stride * height];
        for (i, b) in y.iter_mut().enumerate() {
            *b = (16 + (i % 220)) as u8;
        }
        for (i, (ub, vb)) in u.iter_mut().zip(v.iter_mut()).enumerate() {
            *ub = (90 + (i % 50)) as u8;
            *vb = (110 + (i % 50)) as u8;
        }
        let dst_stride = width * 4;
        let mut dst = vec![0u8; dst_stride * height];
        let table = &YUV_RGB_709;
        let warmup = 2;
        let iters = 10;

        for _ in 0..warmup {
            yuv422_band_to_bgra_scalar(
                &y, y_stride, &u, u_stride, &v, u_stride, 0, height, width, &mut dst, dst_stride,
                table,
            );
            yuv422_band_to_bgra_portable(
                &y, y_stride, &u, u_stride, &v, u_stride, 0, height, width, &mut dst, dst_stride,
                table,
            );
        }

        let t0 = Instant::now();
        for _ in 0..iters {
            yuv422_band_to_bgra_scalar(
                &y, y_stride, &u, u_stride, &v, u_stride, 0, height, width, &mut dst, dst_stride,
                table,
            );
        }
        let scalar = t0.elapsed();

        let t0 = Instant::now();
        for _ in 0..iters {
            yuv422_band_to_bgra_portable(
                &y, y_stride, &u, u_stride, &v, u_stride, 0, height, width, &mut dst, dst_stride,
                table,
            );
        }
        let portable = t0.elapsed();

        eprintln!(
            "yuv422→bgra 1080p scalar={:.3}ms portable={:.3}ms ({:.2}x)",
            scalar.as_secs_f64() * 1000.0 / iters as f64,
            portable.as_secs_f64() * 1000.0 / iters as f64,
            scalar.as_secs_f64() / portable.as_secs_f64().max(1e-12)
        );
        assert!(
            portable < scalar,
            "portable BGRA should beat scalar (scalar={scalar:?} portable={portable:?})"
        );
    }
}
