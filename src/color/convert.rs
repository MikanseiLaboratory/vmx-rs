//! Color conversion between packed formats and internal planar 4:2:2(:4).

use crate::tables::{RGB_YUV_601, RGB_YUV_709, ShortRgb, YUV_RGB_601, YUV_RGB_709};
use crate::types::{ColorSpace, Size};

pub fn select_rgb_yuv(cs: ColorSpace, height: i32) -> &'static [ShortRgb; 3] {
    match cs {
        ColorSpace::Bt601 => &RGB_YUV_601,
        ColorSpace::Bt709 => &RGB_YUV_709,
        ColorSpace::Undefined => {
            if height >= 720 {
                &RGB_YUV_709
            } else {
                &RGB_YUV_601
            }
        }
    }
}

pub fn select_yuv_rgb(cs: ColorSpace, height: i32) -> &'static [i16; 5] {
    match cs {
        ColorSpace::Bt601 => &YUV_RGB_601,
        ColorSpace::Bt709 => &YUV_RGB_709,
        ColorSpace::Undefined => {
            if height >= 720 {
                &YUV_RGB_709
            } else {
                &YUV_RGB_601
            }
        }
    }
}

/// Runtime-dispatched UYVY → planar. `pub(crate)` for `Codec` encode paths.
pub(crate) fn uyvy_to_planar(
    src: &[u8],
    stride: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    size: Size,
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("ssse3") {
            // SAFETY: SSSE3 detected; function only reads/writes within sized rows.
            return unsafe {
                uyvy_to_planar_ssse3(src, stride, y, y_stride, u, u_stride, v, v_stride, size)
            };
        }
    }
    uyvy_to_planar_scalar(src, stride, y, y_stride, u, u_stride, v, v_stride, size);
}

/// Scalar UYVY → planar.
///
/// `pub` so Criterion can call it via `vmx::kernels` (benches are a separate crate).
pub fn uyvy_to_planar_scalar(
    src: &[u8],
    stride: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    size: Size,
) {
    for row in 0..size.height as usize {
        let s = &src[row * stride..];
        let yd = &mut y[row * y_stride..];
        let ud = &mut u[row * u_stride..];
        let vd = &mut v[row * v_stride..];
        let mut x = 0;
        let mut px = 0;
        while px + 1 < size.width as usize {
            // U Y0 V Y1
            ud[x] = s[px * 2];
            yd[px] = s[px * 2 + 1];
            vd[x] = s[px * 2 + 2];
            yd[px + 1] = s[px * 2 + 3];
            x += 1;
            px += 2;
        }
    }
}

/// SSSE3 UYVY → planar. `pub` for Criterion via `vmx::kernels` (not a stable API).
///
/// # Safety
/// Caller must have detected SSSE3. Buffers must cover `size` with the given strides.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
pub unsafe fn uyvy_to_planar_ssse3(
    src: &[u8],
    stride: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    size: Size,
) {
    use std::arch::x86_64::*;
    // SAFETY: caller gated on SSSE3; row loops clamp to buffer lengths below.
    unsafe {
        // Matches libvmx: shuffle UYVY → YYYYYYYY UUUUVVVV within each 16-byte lane.
        let y_shuffle = _mm_set_epi8(14, 10, 6, 2, 12, 8, 4, 0, 15, 13, 11, 9, 7, 5, 3, 1);
        let width = size.width as usize;
        let simd_w = width & !31; // 32 luma samples = 64 UYVY bytes per iteration

        for row in 0..size.height as usize {
            let s = src.get_unchecked(row * stride..);
            let yd = y.get_unchecked_mut(row * y_stride..);
            let ud = u.get_unchecked_mut(row * u_stride..);
            let vd = v.get_unchecked_mut(row * v_stride..);

            let mut px = 0usize;
            while px < simd_w {
                let src_off = px * 2;
                let uyvy1 = _mm_loadu_si128(s.as_ptr().add(src_off).cast());
                let uyvy2 = _mm_loadu_si128(s.as_ptr().add(src_off + 16).cast());
                let uyvy3 = _mm_loadu_si128(s.as_ptr().add(src_off + 32).cast());
                let uyvy4 = _mm_loadu_si128(s.as_ptr().add(src_off + 48).cast());

                let s1 = _mm_shuffle_epi8(uyvy1, y_shuffle);
                let s2 = _mm_shuffle_epi8(uyvy2, y_shuffle);
                let s3 = _mm_shuffle_epi8(uyvy3, y_shuffle);
                let s4 = _mm_shuffle_epi8(uyvy4, y_shuffle);

                let y1 = _mm_unpacklo_epi64(s1, s2);
                let y2 = _mm_unpacklo_epi64(s3, s4);
                _mm_storeu_si128(yd.as_mut_ptr().add(px).cast(), y1);
                _mm_storeu_si128(yd.as_mut_ptr().add(px + 16).cast(), y2);

                let uv1 = _mm_unpackhi_epi32(s1, s2);
                let uv2 = _mm_unpackhi_epi32(s3, s4);
                let uu = _mm_unpacklo_epi64(uv1, uv2);
                let vv = _mm_unpackhi_epi64(uv1, uv2);
                let uv_x = px / 2;
                _mm_storeu_si128(ud.as_mut_ptr().add(uv_x).cast(), uu);
                _mm_storeu_si128(vd.as_mut_ptr().add(uv_x).cast(), vv);
                px += 32;
            }

            // Scalar tail for widths not divisible by 32.
            let mut x = simd_w / 2;
            let mut p = simd_w;
            while p + 1 < width {
                ud[x] = s[p * 2];
                yd[p] = s[p * 2 + 1];
                vd[x] = s[p * 2 + 2];
                yd[p + 1] = s[p * 2 + 3];
                x += 1;
                p += 2;
            }
        }
    }
}

/// Runtime-dispatched planar → UYVY. `pub(crate)` for `Codec` decode paths.
pub(crate) fn planar_to_uyvy(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    dst: &mut [u8],
    stride: usize,
    size: Size,
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            return unsafe {
                planar_to_uyvy_sse2(y, y_stride, u, u_stride, v, v_stride, dst, stride, size)
            };
        }
    }
    planar_to_uyvy_scalar(y, y_stride, u, u_stride, v, v_stride, dst, stride, size);
}

/// Scalar planar → UYVY. `pub` for Criterion via `vmx::kernels` (not a stable API).
pub fn planar_to_uyvy_scalar(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    dst: &mut [u8],
    stride: usize,
    size: Size,
) {
    for row in 0..size.height as usize {
        let yd = &y[row * y_stride..];
        let ud = &u[row * u_stride..];
        let vd = &v[row * v_stride..];
        let d = &mut dst[row * stride..];
        let mut x = 0;
        let mut px = 0;
        while px + 1 < size.width as usize {
            d[px * 2] = ud[x];
            d[px * 2 + 1] = yd[px];
            d[px * 2 + 2] = vd[x];
            d[px * 2 + 3] = yd[px + 1];
            x += 1;
            px += 2;
        }
    }
}

/// SSE2 planar → UYVY. `pub` for Criterion via `vmx::kernels` (not a stable API).
///
/// # Safety
/// Caller must have detected SSE2. Buffers must cover `size` with the given strides.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn planar_to_uyvy_sse2(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    dst: &mut [u8],
    stride: usize,
    size: Size,
) {
    use std::arch::x86_64::*;
    unsafe {
        let width = size.width as usize;
        let simd_w = width & !15; // 16 luma → 8 UV pairs → 32 destination bytes

        for row in 0..size.height as usize {
            let yd = y.get_unchecked(row * y_stride..);
            let ud = u.get_unchecked(row * u_stride..);
            let vd = v.get_unchecked(row * v_stride..);
            let d = dst.get_unchecked_mut(row * stride..);

            let mut px = 0usize;
            while px < simd_w {
                let yv = _mm_loadu_si128(yd.as_ptr().add(px).cast());
                let uv_x = px / 2;
                let uu = _mm_loadl_epi64(ud.as_ptr().add(uv_x).cast());
                let vv = _mm_loadl_epi64(vd.as_ptr().add(uv_x).cast());
                // Interleave U/V: u0 v0 u1 v1 ...
                let uv = _mm_unpacklo_epi8(uu, vv);
                // Interleave UV with Y: u y0 v y1 ...
                let lo = _mm_unpacklo_epi8(uv, yv);
                let hi = _mm_unpackhi_epi8(uv, yv);
                _mm_storeu_si128(d.as_mut_ptr().add(px * 2).cast(), lo);
                _mm_storeu_si128(d.as_mut_ptr().add(px * 2 + 16).cast(), hi);
                px += 16;
            }

            let mut x = simd_w / 2;
            let mut p = simd_w;
            while p + 1 < width {
                d[p * 2] = ud[x];
                d[p * 2 + 1] = yd[p];
                d[p * 2 + 2] = vd[x];
                d[p * 2 + 3] = yd[p + 1];
                x += 1;
                p += 2;
            }
        }
    }
}

pub fn yuy2_to_planar(
    src: &[u8],
    stride: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    size: Size,
) {
    for row in 0..size.height as usize {
        let s = &src[row * stride..];
        let yd = &mut y[row * y_stride..];
        let ud = &mut u[row * u_stride..];
        let vd = &mut v[row * v_stride..];
        let mut x = 0;
        let mut px = 0;
        while px + 1 < size.width as usize {
            // Y0 U Y1 V
            yd[px] = s[px * 2];
            ud[x] = s[px * 2 + 1];
            yd[px + 1] = s[px * 2 + 2];
            vd[x] = s[px * 2 + 3];
            x += 1;
            px += 2;
        }
    }
}

pub fn planar_to_yuy2(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    dst: &mut [u8],
    stride: usize,
    size: Size,
) {
    for row in 0..size.height as usize {
        let yd = &y[row * y_stride..];
        let ud = &u[row * u_stride..];
        let vd = &v[row * v_stride..];
        let d = &mut dst[row * stride..];
        let mut x = 0;
        let mut px = 0;
        while px + 1 < size.width as usize {
            d[px * 2] = yd[px];
            d[px * 2 + 1] = ud[x];
            d[px * 2 + 2] = yd[px + 1];
            d[px * 2 + 3] = vd[x];
            x += 1;
            px += 2;
        }
    }
}

pub fn bgra_to_yuv4224(
    src: &[u8],
    src_stride: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    a: &mut [u8],
    a_stride: usize,
    size: Size,
    table: &[ShortRgb; 3],
) {
    for row in 0..size.height as usize {
        let s = &src[row * src_stride..];
        let yd = &mut y[row * y_stride..];
        let ud = &mut u[row * u_stride..];
        let vd = &mut v[row * v_stride..];
        let ad = &mut a[row * a_stride..];
        let mut x = 0;
        let mut px = 0;
        while px + 1 < size.width as usize {
            let mut uy = 0i32;
            let mut vv = 0i32;
            for i in 0..2 {
                let o = (px + i) * 4;
                let b = s[o] as i32;
                let g = s[o + 1] as i32;
                let r = s[o + 2] as i32;
                ad[px + i] = s[o + 3];
                let yi =
                    (table[0].r as i32 * r + table[0].g as i32 * g + table[0].b as i32 * b + 128)
                        >> 8;
                yd[px + i] = yi.clamp(0, 255) as u8;
                uy += (table[1].r as i32 * r + table[1].g as i32 * g + table[1].b as i32 * b + 128)
                    >> 8;
                vv += (table[2].r as i32 * r + table[2].g as i32 * g + table[2].b as i32 * b + 128)
                    >> 8;
            }
            ud[x] = ((uy / 2) + 128).clamp(0, 255) as u8;
            vd[x] = ((vv / 2) + 128).clamp(0, 255) as u8;
            x += 1;
            px += 2;
        }
    }
}

pub fn yuv4224_to_bgra(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    a: &[u8],
    a_stride: usize,
    dst: &mut [u8],
    dst_stride: usize,
    size: Size,
    table: &[i16; 5],
) {
    yuv422_to_bgra_impl(
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        Some((a, a_stride)),
        dst,
        dst_stride,
        size,
        table,
    );
}

/// Progressive 4:2:2 → BGRA with opaque alpha (no alpha plane read).
#[allow(dead_code)]
pub fn yuv422_to_bgra(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    dst: &mut [u8],
    dst_stride: usize,
    size: Size,
    table: &[i16; 5],
) {
    yuv422_to_bgra_impl(
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        None,
        dst,
        dst_stride,
        size,
        table,
    );
}

fn yuv422_to_bgra_impl(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    alpha: Option<(&[u8], usize)>,
    dst: &mut [u8],
    dst_stride: usize,
    size: Size,
    table: &[i16; 5],
) {
    yuv422_to_bgra_scalar(
        y, y_stride, u, u_stride, v, v_stride, alpha, dst, dst_stride, size, table,
    );
}

/// Pack a horizontal band of planar 4:2:2 into opaque BGRA (used by fused decode).
pub fn yuv422_band_to_bgra(
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
        let mut x = 0;
        let mut px = 0;
        while px + 1 < width {
            let cb = ud[x] as i32 - 128;
            let cr = vd[x] as i32 - 128;
            for i in 0..2 {
                let yy = yd[px + i] as i32;
                let y_term = (table[0] as i32 * (yy - 16)) >> 14;
                let r = y_term + ((table[1] as i32 * cr) >> 14);
                let g = y_term - ((table[2] as i32 * cb) >> 14) - ((table[3] as i32 * cr) >> 14);
                let b = y_term + ((table[4] as i32 * cb) >> 13);
                let o = (px + i) * 4;
                d[o] = b.clamp(0, 255) as u8;
                d[o + 1] = g.clamp(0, 255) as u8;
                d[o + 2] = r.clamp(0, 255) as u8;
                d[o + 3] = 255;
            }
            x += 1;
            px += 2;
        }
    }
}

fn yuv422_to_bgra_scalar(
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
    alpha: Option<(&[u8], usize)>,
    dst: &mut [u8],
    dst_stride: usize,
    size: Size,
    table: &[i16; 5],
) {
    for row in 0..size.height as usize {
        let yd = &y[row * y_stride..];
        let ud = &u[row * u_stride..];
        let vd = &v[row * v_stride..];
        let ad = alpha.map(|(a, asz)| &a[row * asz..]);
        let d = &mut dst[row * dst_stride..];
        let mut x = 0;
        let mut px = 0;
        while px + 1 < size.width as usize {
            let cb = ud[x] as i32 - 128;
            let cr = vd[x] as i32 - 128;
            for i in 0..2 {
                let yy = yd[px + i] as i32;
                let y_term = (table[0] as i32 * (yy - 16)) >> 14;
                let r = y_term + ((table[1] as i32 * cr) >> 14);
                let g = y_term - ((table[2] as i32 * cb) >> 14) - ((table[3] as i32 * cr) >> 14);
                let b = y_term + ((table[4] as i32 * cb) >> 13); // B was halved
                let o = (px + i) * 4;
                d[o] = b.clamp(0, 255) as u8;
                d[o + 1] = g.clamp(0, 255) as u8;
                d[o + 2] = r.clamp(0, 255) as u8;
                d[o + 3] = ad.map(|a| a[px + i]).unwrap_or(255);
            }
            x += 1;
            px += 2;
        }
    }
}

pub fn nv12_to_planar(
    src_y: &[u8],
    stride_y: usize,
    src_uv: &[u8],
    stride_uv: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    size: Size,
) {
    for row in 0..size.height as usize {
        y[row * y_stride..row * y_stride + size.width as usize]
            .copy_from_slice(&src_y[row * stride_y..row * stride_y + size.width as usize]);
        if row % 2 == 0 {
            let uv_row = row / 2;
            let s = &src_uv[uv_row * stride_uv..];
            let w2 = size.width as usize / 2;
            let mut tmp_u = vec![0u8; w2];
            let mut tmp_v = vec![0u8; w2];
            for x in 0..w2 {
                tmp_u[x] = s[x * 2];
                tmp_v[x] = s[x * 2 + 1];
            }
            u[row * u_stride..row * u_stride + w2].copy_from_slice(&tmp_u);
            v[row * v_stride..row * v_stride + w2].copy_from_slice(&tmp_v);
            if row + 1 < size.height as usize {
                u[(row + 1) * u_stride..(row + 1) * u_stride + w2].copy_from_slice(&tmp_u);
                v[(row + 1) * v_stride..(row + 1) * v_stride + w2].copy_from_slice(&tmp_v);
            }
        }
    }
}

pub fn yv12_to_planar(
    src_y: &[u8],
    stride_y: usize,
    src_u: &[u8],
    stride_u: usize,
    src_v: &[u8],
    stride_v: usize,
    y: &mut [u8],
    y_stride: usize,
    u: &mut [u8],
    u_stride: usize,
    v: &mut [u8],
    v_stride: usize,
    size: Size,
) {
    for row in 0..size.height as usize {
        y[row * y_stride..row * y_stride + size.width as usize]
            .copy_from_slice(&src_y[row * stride_y..row * stride_y + size.width as usize]);
        if row % 2 == 0 {
            let uv_row = row / 2;
            let w2 = size.width as usize / 2;
            let tmp_u = src_u[uv_row * stride_u..uv_row * stride_u + w2].to_vec();
            let tmp_v = src_v[uv_row * stride_v..uv_row * stride_v + w2].to_vec();
            u[row * u_stride..row * u_stride + w2].copy_from_slice(&tmp_u);
            v[row * v_stride..row * v_stride + w2].copy_from_slice(&tmp_v);
            if row + 1 < size.height as usize {
                u[(row + 1) * u_stride..(row + 1) * u_stride + w2].copy_from_slice(&tmp_u);
                v[(row + 1) * v_stride..(row + 1) * v_stride + w2].copy_from_slice(&tmp_v);
            }
        }
    }
}

pub fn calculate_psnr(
    a: &[u8],
    b: &[u8],
    stride: usize,
    bytes_per_pixel: usize,
    size: Size,
) -> f32 {
    let mut mse = 0f64;
    let mut count = 0f64;
    for y in 0..size.height as usize {
        for x in 0..(size.width as usize * bytes_per_pixel) {
            let d = a[y * stride + x] as f64 - b[y * stride + x] as f64;
            mse += d * d;
            count += 1.0;
        }
    }
    if count == 0.0 || mse == 0.0 {
        return 99.0;
    }
    mse /= count;
    (10.0 * (255.0f64 * 255.0 / mse).log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Size;

    fn fill_uyvy(width: i32, height: i32) -> (Vec<u8>, usize) {
        let stride = (width as usize) * 2;
        let mut src = vec![0u8; stride * height as usize];
        for (i, b) in src.iter_mut().enumerate() {
            *b = ((i * 37 + 11) % 256) as u8;
        }
        (src, stride)
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn uyvy_to_planar_ssse3_matches_scalar() {
        if !is_x86_feature_detected!("ssse3") {
            return;
        }
        // Width not divisible by 32 exercises SIMD body + scalar tail.
        let size = Size::new(80, 16);
        let (src, stride) = fill_uyvy(size.width, size.height);
        let y_stride = size.width as usize;
        let u_stride = (size.width / 2) as usize;
        let plane_len = y_stride * size.height as usize;
        let uv_len = u_stride * size.height as usize;

        let mut y_s = vec![0u8; plane_len];
        let mut u_s = vec![0u8; uv_len];
        let mut v_s = vec![0u8; uv_len];
        let mut y_v = vec![0u8; plane_len];
        let mut u_v = vec![0u8; uv_len];
        let mut v_v = vec![0u8; uv_len];

        uyvy_to_planar_scalar(
            &src, stride, &mut y_s, y_stride, &mut u_s, u_stride, &mut v_s, u_stride, size,
        );
        // SAFETY: SSSE3 detected; buffers sized for `size`.
        unsafe {
            uyvy_to_planar_ssse3(
                &src, stride, &mut y_v, y_stride, &mut u_v, u_stride, &mut v_v, u_stride, size,
            );
        }
        assert_eq!(y_s, y_v);
        assert_eq!(u_s, u_v);
        assert_eq!(v_s, v_v);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn planar_to_uyvy_sse2_matches_scalar() {
        if !is_x86_feature_detected!("sse2") {
            return;
        }
        let size = Size::new(80, 16);
        let y_stride = size.width as usize;
        let u_stride = (size.width / 2) as usize;
        let plane_len = y_stride * size.height as usize;
        let uv_len = u_stride * size.height as usize;
        let mut y = vec![0u8; plane_len];
        let mut u = vec![0u8; uv_len];
        let mut v = vec![0u8; uv_len];
        for (i, b) in y.iter_mut().enumerate() {
            *b = ((i * 13) % 256) as u8;
        }
        for (i, b) in u.iter_mut().enumerate() {
            *b = ((i * 17 + 3) % 256) as u8;
        }
        for (i, b) in v.iter_mut().enumerate() {
            *b = ((i * 19 + 5) % 256) as u8;
        }

        let stride = (size.width as usize) * 2;
        let mut dst_s = vec![0u8; stride * size.height as usize];
        let mut dst_v = vec![0u8; stride * size.height as usize];
        planar_to_uyvy_scalar(
            &y, y_stride, &u, u_stride, &v, u_stride, &mut dst_s, stride, size,
        );
        // SAFETY: SSE2 detected; buffers sized for `size`.
        unsafe {
            planar_to_uyvy_sse2(
                &y, y_stride, &u, u_stride, &v, u_stride, &mut dst_v, stride, size,
            );
        }
        assert_eq!(dst_s, dst_v);
    }

    #[test]
    fn uyvy_roundtrip_via_public_api() {
        let size = Size::new(64, 16);
        let (src, stride) = fill_uyvy(size.width, size.height);
        let y_stride = size.width as usize;
        let u_stride = (size.width / 2) as usize;
        let plane_len = y_stride * size.height as usize;
        let uv_len = u_stride * size.height as usize;
        let mut y = vec![0u8; plane_len];
        let mut u = vec![0u8; uv_len];
        let mut v = vec![0u8; uv_len];
        uyvy_to_planar(
            &src, stride, &mut y, y_stride, &mut u, u_stride, &mut v, u_stride, size,
        );
        let mut out = vec![0u8; stride * size.height as usize];
        planar_to_uyvy(
            &y, y_stride, &u, u_stride, &v, u_stride, &mut out, stride, size,
        );
        assert_eq!(src, out);
    }
}
