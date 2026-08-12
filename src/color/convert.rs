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

/// Runtime-dispatched UYVY → planar.
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

/// SSSE3 UYVY → planar.
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
        let y_shuffle = _mm_set_epi8(14, 10, 6, 2, 12, 8, 4, 0, 15, 13, 11, 9, 7, 5, 3, 1);
        let width = size.width as usize;
        let simd_w = width & !31; // 32 luma samples per iteration

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

            // Scalar tail.
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

/// Runtime-dispatched planar → UYVY.
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

/// Scalar planar → UYVY.
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

/// SSE2 planar → UYVY.
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

/// Runtime-dispatched YUY2 → planar.
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
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("ssse3") {
            return unsafe {
                yuy2_to_planar_ssse3(src, stride, y, y_stride, u, u_stride, v, v_stride, size)
            };
        }
    }
    yuy2_to_planar_scalar(src, stride, y, y_stride, u, u_stride, v, v_stride, size);
}

/// Scalar YUY2 → planar.
pub fn yuy2_to_planar_scalar(
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

/// SSSE3 YUY2 → planar.
///
/// # Safety
/// Caller must have detected SSSE3. Buffers must cover `size` with the given strides.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
pub unsafe fn yuy2_to_planar_ssse3(
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
    unsafe {
        // Low 8: Y at even bytes; high 8: U then V (same layout as UYVY shuffle output).
        let y_shuffle = _mm_set_epi8(15, 11, 7, 3, 13, 9, 5, 1, 14, 12, 10, 8, 6, 4, 2, 0);
        let width = size.width as usize;
        let simd_w = width & !31;

        for row in 0..size.height as usize {
            let s = src.get_unchecked(row * stride..);
            let yd = y.get_unchecked_mut(row * y_stride..);
            let ud = u.get_unchecked_mut(row * u_stride..);
            let vd = v.get_unchecked_mut(row * v_stride..);

            let mut px = 0usize;
            while px < simd_w {
                let src_off = px * 2;
                let p1 = _mm_loadu_si128(s.as_ptr().add(src_off).cast());
                let p2 = _mm_loadu_si128(s.as_ptr().add(src_off + 16).cast());
                let p3 = _mm_loadu_si128(s.as_ptr().add(src_off + 32).cast());
                let p4 = _mm_loadu_si128(s.as_ptr().add(src_off + 48).cast());

                let s1 = _mm_shuffle_epi8(p1, y_shuffle);
                let s2 = _mm_shuffle_epi8(p2, y_shuffle);
                let s3 = _mm_shuffle_epi8(p3, y_shuffle);
                let s4 = _mm_shuffle_epi8(p4, y_shuffle);

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

            let mut x = simd_w / 2;
            let mut p = simd_w;
            while p + 1 < width {
                yd[p] = s[p * 2];
                ud[x] = s[p * 2 + 1];
                yd[p + 1] = s[p * 2 + 2];
                vd[x] = s[p * 2 + 3];
                x += 1;
                p += 2;
            }
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
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            return unsafe {
                planar_to_yuy2_sse2(y, y_stride, u, u_stride, v, v_stride, dst, stride, size)
            };
        }
    }
    planar_to_yuy2_scalar(y, y_stride, u, u_stride, v, v_stride, dst, stride, size);
}

/// Scalar planar → YUY2.
pub fn planar_to_yuy2_scalar(
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

/// SSE2 planar → YUY2.
///
/// # Safety
/// Caller must have detected SSE2. Buffers must cover `size` with the given strides.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn planar_to_yuy2_sse2(
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
        let simd_w = width & !15;

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
                let uv = _mm_unpacklo_epi8(uu, vv);
                let lo = _mm_unpacklo_epi8(yv, uv);
                let hi = _mm_unpackhi_epi8(yv, uv);
                _mm_storeu_si128(d.as_mut_ptr().add(px * 2).cast(), lo);
                _mm_storeu_si128(d.as_mut_ptr().add(px * 2 + 16).cast(), hi);
                px += 16;
            }

            let mut x = simd_w / 2;
            let mut p = simd_w;
            while p + 1 < width {
                d[p * 2] = yd[p];
                d[p * 2 + 1] = ud[x];
                d[p * 2 + 2] = yd[p + 1];
                d[p * 2 + 3] = vd[x];
                x += 1;
                p += 2;
            }
        }
    }
}

/// BGRA → planar 4:2:2:4.
#[allow(dead_code)] // convenience wrapper; hot path uses with_path
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
    bgra_to_yuv4224_with_path(
        crate::color::simd::ColorSimdPath::detect(),
        src,
        src_stride,
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        a,
        a_stride,
        size,
        table,
    );
}

/// Same as [`bgra_to_yuv4224`] using a preselected [`crate::color::simd::ColorSimdPath`].
pub fn bgra_to_yuv4224_with_path(
    path: crate::color::simd::ColorSimdPath,
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
    crate::color::simd::bgra_to_yuv4224_dispatch(
        path, src, src_stride, y, y_stride, u, u_stride, v, v_stride, a, a_stride, size, table,
    );
}

/// BGRA → YUV4224 scalar.
pub fn bgra_to_yuv4224_scalar(
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
            let mut u_sum = 0i32;
            let mut v_sum = 0i32;
            for i in 0..2 {
                let o = (px + i) * 4;
                let b = s[o] as i32;
                let g = s[o + 1] as i32;
                let r = s[o + 2] as i32;
                ad[px + i] = s[o + 3];
                yd[px + i] = rgb_to_y_libvmx(r, g, b, &table[0]);
                u_sum += rgb_to_chroma_biased(r, g, b, &table[1]);
                v_sum += rgb_to_chroma_biased(r, g, b, &table[2]);
            }
            // hadd + srai 1.
            ud[x] = (u_sum >> 1).clamp(0, 255) as u8;
            vd[x] = (v_sum >> 1).clamp(0, 255) as u8;
            x += 1;
            px += 2;
        }
    }
}

#[inline]
fn rgb_to_y_libvmx(r: i32, g: i32, b: i32, c: &ShortRgb) -> u8 {
    // ConvertRGBVecU: +128, >>8, +16.
    let y = (c.r as i32 * r + c.g as i32 * g + c.b as i32 * b + 128) >> 8;
    (y + 16).clamp(0, 255) as u8
}

#[inline]
fn rgb_to_chroma_biased(r: i32, g: i32, b: i32, c: &ShortRgb) -> i32 {
    // ConvertRGBVec: +128, >>8, +128.
    ((c.r as i32 * r + c.g as i32 * g + c.b as i32 * b + 128) >> 8) + 128
}

/// Progressive 4:2:2:4 → BGRA.
///
/// Packs YUV via SIMD (opaque), then overwrites alpha from the A plane.
#[allow(dead_code)] // convenience wrapper; hot path uses with_path
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
    yuv4224_to_bgra_with_path(
        crate::color::simd::ColorSimdPath::detect(),
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        a,
        a_stride,
        dst,
        dst_stride,
        size,
        table,
    );
}

/// Same as [`yuv4224_to_bgra`] using a preselected [`crate::color::simd::ColorSimdPath`].
#[allow(dead_code)] // used by tests and public API parity
pub fn yuv4224_to_bgra_with_path(
    path: crate::color::simd::ColorSimdPath,
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
    let height = size.height as usize;
    let width = size.width as usize;
    yuv422_band_to_bgra_with_path(
        path, y, y_stride, u, u_stride, v, v_stride, 0, height, width, dst, dst_stride, table,
    );
    // Scalar merge of alpha channel only.
    for row in 0..height {
        let ad = &a[row * a_stride..];
        let d = &mut dst[row * dst_stride..];
        for px in 0..width {
            d[px * 4 + 3] = ad[px];
        }
    }
}

/// Progressive 4:2:2 → BGRA, opaque alpha.
#[allow(dead_code)] // convenience wrapper; hot path uses with_path
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
    yuv422_to_bgra_with_path(
        crate::color::simd::ColorSimdPath::detect(),
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        dst,
        dst_stride,
        size,
        table,
    );
}

/// Same as [`yuv422_to_bgra`] using a preselected [`crate::color::simd::ColorSimdPath`].
#[allow(dead_code)] // used by tests and public API parity
pub fn yuv422_to_bgra_with_path(
    path: crate::color::simd::ColorSimdPath,
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
    yuv422_band_to_bgra_with_path(
        path,
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        0,
        size.height as usize,
        size.width as usize,
        dst,
        dst_stride,
        table,
    );
}

/// Pack a planar 4:2:2 band into opaque BGRA.
///
/// Prefer [`yuv422_band_to_bgra_with_path`] on hot codec paths.
#[allow(dead_code)] // convenience wrapper; hot path uses with_path
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
    yuv422_band_to_bgra_with_path(
        crate::color::simd::ColorSimdPath::detect(),
        y,
        y_stride,
        u,
        u_stride,
        v,
        v_stride,
        y_row0,
        rows,
        width,
        dst,
        dst_stride,
        table,
    );
}

/// Pack a band with a preselected color SIMD path.
pub fn yuv422_band_to_bgra_with_path(
    path: crate::color::simd::ColorSimdPath,
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
    crate::color::simd::yuv422_band_to_bgra_dispatch(
        path, y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
    );
}

/// YUV422 band → BGRA scalar.
pub fn yuv422_band_to_bgra_scalar(
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
            let cb = ud[x] as i16 - 128;
            let cr = vd[x] as i16 - 128;
            for i in 0..2 {
                let (b, g, r) = yuv_to_bgra_pixel(yd[px + i], cb, cr, table);
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

#[inline]
fn mulhi_i16(a: i16, b: i16) -> i16 {
    ((a as i32 * b as i32) >> 16) as i16
}

/// One YUV→BGRA pixel (`<<6/<<7`, `mulhi`, `+8`, `>>4`).
#[inline]
pub(crate) fn yuv_to_bgra_pixel(yy: u8, cb: i16, cr: i16, table: &[i16; 5]) -> (u8, u8, u8) {
    let y_sat = yy.saturating_sub(16) as i16;
    let y0 = mulhi_i16(y_sat << 6, table[0]);
    let r = y0.saturating_add(mulhi_i16(cr << 6, table[1]));
    let b = y0.saturating_add(mulhi_i16(cb << 7, table[4]));
    let g = y0
        .saturating_sub(mulhi_i16(cb << 6, table[2]))
        .saturating_sub(mulhi_i16(cr << 6, table[3]));
    let r = r.saturating_add(8) >> 4;
    let g = g.saturating_add(8) >> 4;
    let b = b.saturating_add(8) >> 4;
    (
        b.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        r.clamp(0, 255) as u8,
    )
}

/// Scalar YUV422 → BGRA (optional alpha plane). Kept for fallback / alpha full path.
#[allow(dead_code)] // scalar fallback / tests
pub fn yuv422_to_bgra_scalar(
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
            let cb = ud[x] as i16 - 128;
            let cr = vd[x] as i16 - 128;
            for i in 0..2 {
                let (b, g, r) = yuv_to_bgra_pixel(yd[px + i], cb, cr, table);
                let o = (px + i) * 4;
                d[o] = b;
                d[o + 1] = g;
                d[o + 2] = r;
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
            let u_off = row * u_stride;
            let v_off = row * v_stride;
            nv12_split_uv_row(s, &mut u[u_off..u_off + w2], &mut v[v_off..v_off + w2]);
            if row + 1 < size.height as usize {
                u.copy_within(u_off..u_off + w2, (row + 1) * u_stride);
                v.copy_within(v_off..v_off + w2, (row + 1) * v_stride);
            }
        }
    }
}

fn nv12_split_uv_row(src: &[u8], u: &mut [u8], v: &mut [u8]) {
    let w2 = u.len().min(v.len()).min(src.len() / 2);
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            unsafe {
                nv12_split_uv_row_sse2(src, u, v, w2);
            }
            return;
        }
    }
    for x in 0..w2 {
        u[x] = src[x * 2];
        v[x] = src[x * 2 + 1];
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn nv12_split_uv_row_sse2(src: &[u8], u: &mut [u8], v: &mut [u8], w2: usize) {
    use std::arch::x86_64::*;
    unsafe {
        let mask = _mm_set1_epi16(0x00FF);
        let simd_w = w2 & !15;
        let mut x = 0usize;
        while x < simd_w {
            let uv0 = _mm_loadu_si128(src.as_ptr().add(x * 2).cast());
            let uv1 = _mm_loadu_si128(src.as_ptr().add(x * 2 + 16).cast());
            let u_pack = _mm_packus_epi16(_mm_and_si128(uv0, mask), _mm_and_si128(uv1, mask));
            let v_pack = _mm_packus_epi16(_mm_srli_epi16(uv0, 8), _mm_srli_epi16(uv1, 8));
            _mm_storeu_si128(u.as_mut_ptr().add(x).cast(), u_pack);
            _mm_storeu_si128(v.as_mut_ptr().add(x).cast(), v_pack);
            x += 16;
        }
        for i in simd_w..w2 {
            u[i] = src[i * 2];
            v[i] = src[i * 2 + 1];
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
            let u_off = row * u_stride;
            let v_off = row * v_stride;
            u[u_off..u_off + w2].copy_from_slice(&src_u[uv_row * stride_u..uv_row * stride_u + w2]);
            v[v_off..v_off + w2].copy_from_slice(&src_v[uv_row * stride_v..uv_row * stride_v + w2]);
            if row + 1 < size.height as usize {
                u.copy_within(u_off..u_off + w2, (row + 1) * u_stride);
                v.copy_within(v_off..v_off + w2, (row + 1) * v_stride);
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

    fn fill_yuv422_planes(
        width: usize,
        height: usize,
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize) {
        let y_stride = width;
        let u_stride = width / 2;
        let mut y = vec![0u8; y_stride * height];
        let mut u = vec![0u8; u_stride * height];
        let mut v = vec![0u8; u_stride * height];
        for (i, b) in y.iter_mut().enumerate() {
            *b = (16 + (i * 13) % 220) as u8;
        }
        for (i, b) in u.iter_mut().enumerate() {
            *b = (16 + (i * 17) % 220) as u8;
        }
        for (i, b) in v.iter_mut().enumerate() {
            *b = (16 + (i * 19) % 220) as u8;
        }
        (y, u, v, y_stride, u_stride)
    }

    #[test]
    fn yuv422_to_bgra_public_matches_scalar() {
        use crate::tables::YUV_RGB_601;

        let size = Size::new(64, 16);
        let width = size.width as usize;
        let height = size.height as usize;
        let (y, u, v, y_stride, u_stride) = fill_yuv422_planes(width, height);
        let dst_stride = width * 4;

        let mut expected = vec![0u8; dst_stride * height];
        yuv422_to_bgra_scalar(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            None,
            &mut expected,
            dst_stride,
            size,
            &YUV_RGB_601,
        );

        let mut actual = vec![0u8; dst_stride * height];
        yuv422_to_bgra(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            &mut actual,
            dst_stride,
            size,
            &YUV_RGB_601,
        );
        assert_eq!(actual, expected);

        let mut actual_path = vec![0u8; dst_stride * height];
        yuv422_to_bgra_with_path(
            crate::color::simd::ColorSimdPath::Scalar,
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            &mut actual_path,
            dst_stride,
            size,
            &YUV_RGB_601,
        );
        assert_eq!(actual_path, expected);

        // Convenience band wrapper.
        let mut band = vec![0u8; dst_stride * height];
        yuv422_band_to_bgra(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            0,
            height,
            width,
            &mut band,
            dst_stride,
            &YUV_RGB_601,
        );
        assert_eq!(band, expected);
    }

    #[test]
    fn yuv4224_to_bgra_merges_alpha_after_simd_pack() {
        use crate::tables::YUV_RGB_709;

        let size = Size::new(48, 8);
        let width = size.width as usize;
        let height = size.height as usize;
        let (y, u, v, y_stride, u_stride) = fill_yuv422_planes(width, height);
        let a_stride = width;
        let mut a = vec![0u8; a_stride * height];
        for (i, b) in a.iter_mut().enumerate() {
            *b = ((i * 3) % 256) as u8;
        }
        let dst_stride = width * 4;

        let mut expected = vec![0u8; dst_stride * height];
        yuv422_to_bgra_scalar(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            Some((&a, a_stride)),
            &mut expected,
            dst_stride,
            size,
            &YUV_RGB_709,
        );

        let mut actual = vec![0u8; dst_stride * height];
        yuv4224_to_bgra(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            &a,
            a_stride,
            &mut actual,
            dst_stride,
            size,
            &YUV_RGB_709,
        );
        assert_eq!(actual, expected);

        let mut actual_path = vec![0u8; dst_stride * height];
        yuv4224_to_bgra_with_path(
            crate::color::simd::ColorSimdPath::detect(),
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            &a,
            a_stride,
            &mut actual_path,
            dst_stride,
            size,
            &YUV_RGB_709,
        );
        assert_eq!(actual_path, expected);
    }

    #[test]
    fn yuv422_band_to_bgra_simd_matches_scalar_bt601_and_bt709() {
        use crate::color::simd::ColorSimdPath;
        use crate::tables::{YUV_RGB_601, YUV_RGB_709};

        let width = 80usize;
        let height = 16usize;
        let (y, u, v, y_stride, u_stride) = fill_yuv422_planes(width, height);
        let dst_stride = width * 4;

        for table in [&YUV_RGB_601, &YUV_RGB_709] {
            let mut expected = vec![0u8; dst_stride * height];
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

            for path in [
                ColorSimdPath::Scalar,
                #[cfg(target_arch = "x86_64")]
                ColorSimdPath::Sse2,
                #[cfg(target_arch = "x86_64")]
                ColorSimdPath::Avx2,
                #[cfg(target_arch = "aarch64")]
                ColorSimdPath::Neon,
            ] {
                #[cfg(target_arch = "x86_64")]
                {
                    if path == ColorSimdPath::Sse2 && !is_x86_feature_detected!("sse2") {
                        continue;
                    }
                    if path == ColorSimdPath::Avx2 && !is_x86_feature_detected!("avx2") {
                        continue;
                    }
                }
                let mut actual = vec![0u8; dst_stride * height];
                yuv422_band_to_bgra_with_path(
                    path,
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
                assert_eq!(actual, expected, "path={path} table={table:?}");
            }
        }
    }

    #[test]
    fn yuv422_band_edge_saturation_matches_scalar() {
        use crate::color::simd::ColorSimdPath;
        use crate::tables::YUV_RGB_709;

        let width = 16usize;
        let height = 8usize;
        let y = vec![0u8; width * height];
        let u = vec![0u8; (width / 2) * height];
        let v = vec![255u8; (width / 2) * height];
        let dst_stride = width * 4;
        let mut expected = vec![0u8; dst_stride * height];
        let mut actual = vec![0u8; dst_stride * height];
        yuv422_band_to_bgra_scalar(
            &y,
            width,
            &u,
            width / 2,
            &v,
            width / 2,
            0,
            height,
            width,
            &mut expected,
            dst_stride,
            &YUV_RGB_709,
        );
        yuv422_band_to_bgra_with_path(
            ColorSimdPath::detect(),
            &y,
            width,
            &u,
            width / 2,
            &v,
            width / 2,
            0,
            height,
            width,
            &mut actual,
            dst_stride,
            &YUV_RGB_709,
        );
        assert_eq!(actual, expected);
    }

    /// BGRA lattice over 0/1/128/255.
    fn fill_bgra_lattice(width: usize, height: usize) -> (Vec<u8>, usize) {
        let stride = width * 4;
        let mut src = vec![0u8; stride * height];
        let levels = [0u8, 1, 128, 255];
        for row in 0..height {
            for x in 0..width {
                let o = row * stride + x * 4;
                let bi = x % levels.len();
                let gi = (x / levels.len()) % levels.len();
                let ri = (row + x) % levels.len();
                let (b, g, r) = if x % 17 == 0 {
                    (0u8, 255u8, 0u8)
                } else {
                    (levels[bi], levels[gi], levels[ri])
                };
                src[o] = b;
                src[o + 1] = g;
                src[o + 2] = r;
                src[o + 3] = ((row * 3 + x * 5) % 256) as u8;
            }
        }
        (src, stride)
    }

    #[test]
    fn bgra_to_yuv_simd_matches_scalar() {
        use crate::color::simd::ColorSimdPath;
        use crate::tables::{RGB_YUV_601, RGB_YUV_709};

        let size = Size::new(80, 8);
        let width = size.width as usize;
        let height = size.height as usize;
        let (src, src_stride) = fill_bgra_lattice(width, height);
        let y_stride = width;
        let u_stride = width / 2;
        let a_stride = width;
        let plane_len = y_stride * height;
        let uv_len = u_stride * height;

        for table in [&RGB_YUV_601, &RGB_YUV_709] {
            let mut y_s = vec![0u8; plane_len];
            let mut u_s = vec![0u8; uv_len];
            let mut v_s = vec![0u8; uv_len];
            let mut a_s = vec![0u8; plane_len];
            bgra_to_yuv4224_scalar(
                &src, src_stride, &mut y_s, y_stride, &mut u_s, u_stride, &mut v_s, u_stride,
                &mut a_s, a_stride, size, table,
            );

            let mut y_d = vec![0u8; plane_len];
            let mut u_d = vec![0u8; uv_len];
            let mut v_d = vec![0u8; uv_len];
            let mut a_d = vec![0u8; plane_len];
            bgra_to_yuv4224(
                &src, src_stride, &mut y_d, y_stride, &mut u_d, u_stride, &mut v_d, u_stride,
                &mut a_d, a_stride, size, table,
            );
            assert_eq!(y_d, y_s, "Y detect table={table:?}");
            assert_eq!(u_d, u_s, "U detect table={table:?}");
            assert_eq!(v_d, v_s, "V detect table={table:?}");
            assert_eq!(a_d, a_s, "A detect table={table:?}");

            for path in [
                ColorSimdPath::Scalar,
                #[cfg(target_arch = "x86_64")]
                ColorSimdPath::Sse2,
                #[cfg(target_arch = "x86_64")]
                ColorSimdPath::Avx2,
                #[cfg(target_arch = "aarch64")]
                ColorSimdPath::Neon,
            ] {
                #[cfg(target_arch = "x86_64")]
                {
                    if matches!(path, ColorSimdPath::Sse2 | ColorSimdPath::Avx2)
                        && !(is_x86_feature_detected!("ssse3")
                            && is_x86_feature_detected!("sse4.1"))
                    {
                        continue;
                    }
                    if path == ColorSimdPath::Avx2 && !is_x86_feature_detected!("avx2") {
                        continue;
                    }
                }
                let mut y_v = vec![0u8; plane_len];
                let mut u_v = vec![0u8; uv_len];
                let mut v_v = vec![0u8; uv_len];
                let mut a_v = vec![0u8; plane_len];
                bgra_to_yuv4224_with_path(
                    path, &src, src_stride, &mut y_v, y_stride, &mut u_v, u_stride, &mut v_v,
                    u_stride, &mut a_v, a_stride, size, table,
                );
                assert_eq!(y_v, y_s, "Y path={path} table={table:?}");
                assert_eq!(u_v, u_s, "U path={path} table={table:?}");
                assert_eq!(v_v, v_s, "V path={path} table={table:?}");
                assert_eq!(a_v, a_s, "A path={path} table={table:?}");
            }
        }
    }

    fn fill_yuy2(width: i32, height: i32) -> (Vec<u8>, usize) {
        let stride = (width as usize) * 2;
        let mut src = vec![0u8; stride * height as usize];
        for (i, b) in src.iter_mut().enumerate() {
            *b = ((i * 41 + 7) % 256) as u8;
        }
        (src, stride)
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn yuy2_to_planar_ssse3_matches_scalar() {
        if !is_x86_feature_detected!("ssse3") {
            return;
        }
        let size = Size::new(80, 16);
        let (src, stride) = fill_yuy2(size.width, size.height);
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

        yuy2_to_planar_scalar(
            &src, stride, &mut y_s, y_stride, &mut u_s, u_stride, &mut v_s, u_stride, size,
        );
        unsafe {
            yuy2_to_planar_ssse3(
                &src, stride, &mut y_v, y_stride, &mut u_v, u_stride, &mut v_v, u_stride, size,
            );
        }
        assert_eq!(y_s, y_v);
        assert_eq!(u_s, u_v);
        assert_eq!(v_s, v_v);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn planar_to_yuy2_sse2_matches_scalar() {
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
            *b = ((i * 17) % 256) as u8;
        }
        for (i, b) in v.iter_mut().enumerate() {
            *b = ((i * 19) % 256) as u8;
        }
        let dst_stride = size.width as usize * 2;
        let mut expected = vec![0u8; dst_stride * size.height as usize];
        let mut actual = vec![0u8; dst_stride * size.height as usize];
        planar_to_yuy2_scalar(
            &y,
            y_stride,
            &u,
            u_stride,
            &v,
            u_stride,
            &mut expected,
            dst_stride,
            size,
        );
        unsafe {
            planar_to_yuy2_sse2(
                &y,
                y_stride,
                &u,
                u_stride,
                &v,
                u_stride,
                &mut actual,
                dst_stride,
                size,
            );
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn nv12_split_uv_matches_scalar_pairs() {
        let w2 = 80usize;
        let mut src = vec![0u8; w2 * 2];
        for (i, b) in src.iter_mut().enumerate() {
            *b = ((i * 23 + 5) % 256) as u8;
        }
        let mut u = vec![0u8; w2];
        let mut v = vec![0u8; w2];
        nv12_split_uv_row(&src, &mut u, &mut v);
        for x in 0..w2 {
            assert_eq!(u[x], src[x * 2], "u[{x}]");
            assert_eq!(v[x], src[x * 2 + 1], "v[{x}]");
        }
    }

    #[test]
    fn yuv422_bgra_simd_faster_than_scalar_in_release() {
        if cfg!(debug_assertions) {
            return;
        }
        use crate::color::simd::ColorSimdPath;
        use crate::tables::YUV_RGB_709;
        use std::time::Instant;

        let width = 1920usize;
        let height = 1080usize;
        let (y, u, v, y_stride, u_stride) = fill_yuv422_planes(width, height);
        let dst_stride = width * 4;
        let mut dst = vec![0u8; dst_stride * height];
        let table = &YUV_RGB_709;
        let warmup = 2;
        let iters = 8;

        let mut time_path = |path: ColorSimdPath| {
            for _ in 0..warmup {
                yuv422_band_to_bgra_with_path(
                    path, &y, y_stride, &u, u_stride, &v, u_stride, 0, height, width, &mut dst,
                    dst_stride, table,
                );
            }
            let t0 = Instant::now();
            for _ in 0..iters {
                yuv422_band_to_bgra_with_path(
                    path, &y, y_stride, &u, u_stride, &v, u_stride, 0, height, width, &mut dst,
                    dst_stride, table,
                );
            }
            t0.elapsed()
        };

        let scalar = time_path(ColorSimdPath::Scalar);
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("sse2") {
                let sse = time_path(ColorSimdPath::Sse2);
                eprintln!(
                    "yuv422→bgra 1080p scalar={:.3}ms sse2={:.3}ms ({:.2}x)",
                    scalar.as_secs_f64() * 1000.0 / iters as f64,
                    sse.as_secs_f64() * 1000.0 / iters as f64,
                    scalar.as_secs_f64() / sse.as_secs_f64().max(1e-12)
                );
                assert!(
                    sse * 3 < scalar * 2,
                    "SSE2 BGRA pack should be >1.5x faster than scalar (scalar={scalar:?} sse={sse:?})"
                );
            }
            if is_x86_feature_detected!("avx2") {
                let avx = time_path(ColorSimdPath::Avx2);
                eprintln!(
                    "yuv422→bgra 1080p scalar={:.3}ms avx2={:.3}ms ({:.2}x)",
                    scalar.as_secs_f64() * 1000.0 / iters as f64,
                    avx.as_secs_f64() * 1000.0 / iters as f64,
                    scalar.as_secs_f64() / avx.as_secs_f64().max(1e-12)
                );
                assert!(
                    avx * 3 < scalar * 2,
                    "AVX2 BGRA pack should be >1.5x faster than scalar (scalar={scalar:?} avx={avx:?})"
                );
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            let neon = time_path(ColorSimdPath::Neon);
            eprintln!(
                "yuv422→bgra 1080p scalar={:.3}ms neon={:.3}ms ({:.2}x)",
                scalar.as_secs_f64() * 1000.0 / iters as f64,
                neon.as_secs_f64() * 1000.0 / iters as f64,
                scalar.as_secs_f64() / neon.as_secs_f64().max(1e-12)
            );
            assert!(
                neon * 3 < scalar * 2,
                "NEON BGRA pack should be >1.5x faster than scalar"
            );
        }
        let _ = scalar;
    }
}
