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

pub fn uyvy_to_planar(
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

pub fn planar_to_uyvy(
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
    for row in 0..size.height as usize {
        let yd = &y[row * y_stride..];
        let ud = &u[row * u_stride..];
        let vd = &v[row * v_stride..];
        let ad = &a[row * a_stride..];
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
                d[o + 3] = ad[px + i];
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
