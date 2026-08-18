//! SIMD kernels for BGRA ↔ YUV 4:2:2 color conversion.

#![allow(dead_code)]

use crate::color::convert::yuv_to_bgra_pixel;
use crate::tables::ShortRgb;
use crate::types::Size;

/// Color conversion SIMD path.
///
/// For BGRA→YUV encode, [`ColorSimdPath::Avx2`] falls through to the SSSE3 128-bit
/// kernel (matches libvmx; there is no AVX2 BGRA encode). YUV→BGRA pack can use
/// real AVX2 when reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSimdPath {
    /// Scalar.
    Scalar,
    /// SSE2 (YUV→BGRA) / SSSE3 (BGRA→YUV when available).
    Sse2,
    /// AVX2 for YUV→BGRA pack; BGRA→YUV encode still uses SSSE3 128-bit.
    Avx2,
    /// AVX-512 for YUV→BGRA pack (32 px); BGRA→YUV encode still uses SSSE3.
    Avx512,
    /// NEON.
    Neon,
    /// AArch64 SVE/SVE2 YUV→BGRA (nightly `sve` feature). BGRA→YUV uses NEON.
    #[cfg(feature = "sve")]
    Sve,
    /// Nightly `std::simd` portable path (`portable-simd` feature).
    #[cfg(feature = "portable-simd")]
    Portable,
}

impl ColorSimdPath {
    /// Detect the best available path.
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw") {
                return Self::Avx512;
            }
            if is_x86_feature_detected!("avx2") {
                return Self::Avx2;
            }
            if is_x86_feature_detected!("sse2") {
                return Self::Sse2;
            }
            #[cfg(feature = "portable-simd")]
            {
                Self::Portable
            }
            #[cfg(not(feature = "portable-simd"))]
            {
                Self::Scalar
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            #[cfg(feature = "sve")]
            {
                #[cfg(any(target_os = "linux", target_os = "android"))]
                if std::arch::is_aarch64_feature_detected!("sve") {
                    return Self::Sve;
                }
            }
            Self::Neon
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            #[cfg(feature = "portable-simd")]
            {
                Self::Portable
            }
            #[cfg(not(feature = "portable-simd"))]
            {
                Self::Scalar
            }
        }
    }
}

impl std::fmt::Display for ColorSimdPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Scalar => "scalar",
            Self::Sse2 => "sse2",
            Self::Avx2 => "avx2",
            Self::Avx512 => "avx512",
            Self::Neon => "neon",
            #[cfg(feature = "sve")]
            Self::Sve => "sve",
            #[cfg(feature = "portable-simd")]
            Self::Portable => "portable",
        })
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::yuv_to_bgra_pixel;
    use std::arch::x86_64::*;

    /// Convert 8 Y / 4 U / 4 V samples to 8 opaque BGRA pixels.
    ///
    /// # Safety
    /// SSE2 required. `y` has 8 bytes, `u`/`v` have 4 bytes, `dst` has 32 bytes.
    #[target_feature(enable = "sse2")]
    pub unsafe fn yuv422_macropixels_to_bgra8(
        y: *const u8,
        u: *const u8,
        v: *const u8,
        dst: *mut u8,
        table: &[i16; 5],
    ) {
        unsafe {
            let rounding = _mm_set1_epi16(8);
            let mut y_line = _mm_loadl_epi64(y.cast::<__m128i>());
            y_line = _mm_subs_epu8(y_line, _mm_set1_epi8(16));
            let mut y0 = _mm_unpacklo_epi8(y_line, _mm_setzero_si128());

            let u4 =
                _mm_cvtsi32_si128(u32::from_le_bytes([*u, *u.add(1), *u.add(2), *u.add(3)]) as i32);
            let v4 =
                _mm_cvtsi32_si128(u32::from_le_bytes([*v, *v.add(1), *v.add(2), *v.add(3)]) as i32);
            let mut u0 = _mm_sub_epi16(
                _mm_unpacklo_epi8(u4, _mm_setzero_si128()),
                _mm_set1_epi16(128),
            );
            let mut v0 = _mm_sub_epi16(
                _mm_unpacklo_epi8(v4, _mm_setzero_si128()),
                _mm_set1_epi16(128),
            );
            u0 = _mm_unpacklo_epi16(u0, u0);
            v0 = _mm_unpacklo_epi16(v0, v0);

            y0 = _mm_slli_epi16(y0, 6);
            y0 = _mm_mulhi_epi16(y0, _mm_set1_epi16(table[0]));

            v0 = _mm_slli_epi16(v0, 6);
            let mut r = _mm_mulhi_epi16(v0, _mm_set1_epi16(table[1]));
            r = _mm_adds_epi16(r, y0);

            let mut b = _mm_slli_epi16(u0, 7);
            b = _mm_mulhi_epi16(b, _mm_set1_epi16(table[4]));
            b = _mm_adds_epi16(b, y0);

            u0 = _mm_slli_epi16(u0, 6);
            let mut g = _mm_mulhi_epi16(u0, _mm_set1_epi16(table[2]));
            let tmp = _mm_mulhi_epi16(v0, _mm_set1_epi16(table[3]));
            g = _mm_subs_epi16(y0, g);
            g = _mm_subs_epi16(g, tmp);

            r = _mm_adds_epi16(r, rounding);
            g = _mm_adds_epi16(g, rounding);
            b = _mm_adds_epi16(b, rounding);
            r = _mm_srai_epi16(r, 4);
            g = _mm_srai_epi16(g, 4);
            b = _mm_srai_epi16(b, 4);

            let a0 = _mm_set1_epi16(255);
            let mut bg0 = _mm_unpacklo_epi16(b, g);
            let bg1 = _mm_unpackhi_epi16(b, g);
            let ra0 = _mm_unpacklo_epi16(r, a0);
            let ra1 = _mm_unpackhi_epi16(r, a0);
            bg0 = _mm_packus_epi16(bg0, bg1);
            let ra = _mm_packus_epi16(ra0, ra1);
            let bgra0 = _mm_unpacklo_epi16(bg0, ra);
            let bgra1 = _mm_unpackhi_epi16(bg0, ra);
            _mm_storeu_si128(dst.cast::<__m128i>(), bgra0);
            _mm_storeu_si128(dst.add(16).cast::<__m128i>(), bgra1);
        }
    }

    /// Convert 16 Y / 8 U / 8 V samples to 16 opaque BGRA pixels (true 256-bit).
    ///
    /// # Safety
    /// AVX2 required. `y` has 16 bytes, `u`/`v` have 8 bytes, `dst` has 64 bytes.
    #[target_feature(enable = "avx2")]
    pub unsafe fn yuv422_macropixels_to_bgra16(
        y: *const u8,
        u: *const u8,
        v: *const u8,
        dst: *mut u8,
        table: &[i16; 5],
    ) {
        unsafe {
            let rounding = _mm256_set1_epi16(8);
            let y_bytes = _mm_loadu_si128(y.cast::<__m128i>());
            let y_sat = _mm_subs_epu8(y_bytes, _mm_set1_epi8(16));
            let mut y0 = _mm256_cvtepu8_epi16(y_sat);

            let u8v = _mm_loadl_epi64(u.cast::<__m128i>());
            let v8v = _mm_loadl_epi64(v.cast::<__m128i>());
            let u16_8 = _mm_sub_epi16(_mm_cvtepu8_epi16(u8v), _mm_set1_epi16(128));
            let v16_8 = _mm_sub_epi16(_mm_cvtepu8_epi16(v8v), _mm_set1_epi16(128));
            let mut u0 = _mm256_inserti128_si256(
                _mm256_castsi128_si256(_mm_unpacklo_epi16(u16_8, u16_8)),
                _mm_unpackhi_epi16(u16_8, u16_8),
                1,
            );
            let mut v0 = _mm256_inserti128_si256(
                _mm256_castsi128_si256(_mm_unpacklo_epi16(v16_8, v16_8)),
                _mm_unpackhi_epi16(v16_8, v16_8),
                1,
            );

            y0 = _mm256_slli_epi16(y0, 6);
            y0 = _mm256_mulhi_epi16(y0, _mm256_set1_epi16(table[0]));

            v0 = _mm256_slli_epi16(v0, 6);
            let mut r = _mm256_mulhi_epi16(v0, _mm256_set1_epi16(table[1]));
            r = _mm256_adds_epi16(r, y0);

            let mut b = _mm256_slli_epi16(u0, 7);
            b = _mm256_mulhi_epi16(b, _mm256_set1_epi16(table[4]));
            b = _mm256_adds_epi16(b, y0);

            u0 = _mm256_slli_epi16(u0, 6);
            let mut g = _mm256_mulhi_epi16(u0, _mm256_set1_epi16(table[2]));
            let tmp = _mm256_mulhi_epi16(v0, _mm256_set1_epi16(table[3]));
            g = _mm256_subs_epi16(y0, g);
            g = _mm256_subs_epi16(g, tmp);

            r = _mm256_adds_epi16(r, rounding);
            g = _mm256_adds_epi16(g, rounding);
            b = _mm256_adds_epi16(b, rounding);
            r = _mm256_srai_epi16(r, 4);
            g = _mm256_srai_epi16(g, 4);
            b = _mm256_srai_epi16(b, 4);

            let a0 = _mm256_set1_epi16(255);
            let bg_lo = _mm256_unpacklo_epi16(b, g);
            let bg_hi = _mm256_unpackhi_epi16(b, g);
            let ra_lo = _mm256_unpacklo_epi16(r, a0);
            let ra_hi = _mm256_unpackhi_epi16(r, a0);
            let bg = _mm256_packus_epi16(bg_lo, bg_hi);
            let ra = _mm256_packus_epi16(ra_lo, ra_hi);
            let bgra_lo = _mm256_unpacklo_epi16(bg, ra);
            let bgra_hi = _mm256_unpackhi_epi16(bg, ra);
            _mm_storeu_si128(dst.cast::<__m128i>(), _mm256_castsi256_si128(bgra_lo));
            _mm_storeu_si128(
                dst.add(16).cast::<__m128i>(),
                _mm256_castsi256_si128(bgra_hi),
            );
            _mm_storeu_si128(
                dst.add(32).cast::<__m128i>(),
                _mm256_extracti128_si256(bgra_lo, 1),
            );
            _mm_storeu_si128(
                dst.add(48).cast::<__m128i>(),
                _mm256_extracti128_si256(bgra_hi, 1),
            );
        }
    }

    /// Convert 32 Y / 16 U / 16 V samples to 32 opaque BGRA pixels (512-bit).
    ///
    /// # Safety
    /// AVX-512F+BW required. `y` has 32 bytes, `u`/`v` have 16 bytes, `dst` has 128 bytes.
    #[target_feature(enable = "avx512f,avx512bw,avx2")]
    pub unsafe fn yuv422_macropixels_to_bgra32(
        y: *const u8,
        u: *const u8,
        v: *const u8,
        dst: *mut u8,
        table: &[i16; 5],
    ) {
        unsafe {
            let rounding = _mm512_set1_epi16(8);
            let y_bytes = _mm256_loadu_si256(y.cast::<__m256i>());
            let y_sat = _mm256_subs_epu8(y_bytes, _mm256_set1_epi8(16));
            let mut y0 = _mm512_cvtepu8_epi16(y_sat);

            let u16_16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(u.cast::<__m128i>()));
            let v16_16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(v.cast::<__m128i>()));
            let u16_16 = _mm256_sub_epi16(u16_16, _mm256_set1_epi16(128));
            let v16_16 = _mm256_sub_epi16(v16_16, _mm256_set1_epi16(128));
            // Expand each of 16 chroma samples to a duplicated luma pair → 32 lanes.
            let mut u_tmp = [0i16; 16];
            let mut v_tmp = [0i16; 16];
            _mm256_storeu_si256(u_tmp.as_mut_ptr().cast(), u16_16);
            _mm256_storeu_si256(v_tmp.as_mut_ptr().cast(), v16_16);
            let mut u_exp = [0i16; 32];
            let mut v_exp = [0i16; 32];
            for i in 0..16 {
                u_exp[i * 2] = u_tmp[i];
                u_exp[i * 2 + 1] = u_tmp[i];
                v_exp[i * 2] = v_tmp[i];
                v_exp[i * 2 + 1] = v_tmp[i];
            }
            let mut u0 = _mm512_loadu_si512(u_exp.as_ptr().cast());
            let mut v0 = _mm512_loadu_si512(v_exp.as_ptr().cast());

            y0 = _mm512_slli_epi16(y0, 6);
            y0 = _mm512_mulhi_epi16(y0, _mm512_set1_epi16(table[0]));

            v0 = _mm512_slli_epi16(v0, 6);
            let mut r = _mm512_mulhi_epi16(v0, _mm512_set1_epi16(table[1]));
            r = _mm512_adds_epi16(r, y0);

            let mut b = _mm512_slli_epi16(u0, 7);
            b = _mm512_mulhi_epi16(b, _mm512_set1_epi16(table[4]));
            b = _mm512_adds_epi16(b, y0);

            u0 = _mm512_slli_epi16(u0, 6);
            let mut g = _mm512_mulhi_epi16(u0, _mm512_set1_epi16(table[2]));
            let tmp = _mm512_mulhi_epi16(v0, _mm512_set1_epi16(table[3]));
            g = _mm512_subs_epi16(y0, g);
            g = _mm512_subs_epi16(g, tmp);

            r = _mm512_adds_epi16(r, rounding);
            g = _mm512_adds_epi16(g, rounding);
            b = _mm512_adds_epi16(b, rounding);
            r = _mm512_srai_epi16(r, 4);
            g = _mm512_srai_epi16(g, 4);
            b = _mm512_srai_epi16(b, 4);

            let mut ra = [0i16; 32];
            let mut ga = [0i16; 32];
            let mut ba = [0i16; 32];
            _mm512_storeu_si512(ra.as_mut_ptr().cast(), r);
            _mm512_storeu_si512(ga.as_mut_ptr().cast(), g);
            _mm512_storeu_si512(ba.as_mut_ptr().cast(), b);
            for i in 0..32 {
                let o = i * 4;
                *dst.add(o) = ba[i].clamp(0, 255) as u8;
                *dst.add(o + 1) = ga[i].clamp(0, 255) as u8;
                *dst.add(o + 2) = ra[i].clamp(0, 255) as u8;
                *dst.add(o + 3) = 255;
            }
        }
    }

    /// # Safety
    /// AVX-512F+BW required. Buffers must cover the described band.
    #[target_feature(enable = "avx512f,avx512bw,avx2")]
    pub unsafe fn yuv422_band_to_bgra_avx512(
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
        unsafe {
            for row in 0..rows {
                let yr = y_row0 + row;
                let yd = y.as_ptr().add(yr * y_stride);
                let ud = u.as_ptr().add(yr * u_stride);
                let vd = v.as_ptr().add(yr * v_stride);
                let d = dst.as_mut_ptr().add(yr * dst_stride);
                let mut x = 0usize;
                let mut px = 0usize;
                while px + 32 <= width {
                    yuv422_macropixels_to_bgra32(
                        yd.add(px),
                        ud.add(x),
                        vd.add(x),
                        d.add(px * 4),
                        table,
                    );
                    x += 16;
                    px += 32;
                }
                while px + 16 <= width {
                    yuv422_macropixels_to_bgra16(
                        yd.add(px),
                        ud.add(x),
                        vd.add(x),
                        d.add(px * 4),
                        table,
                    );
                    x += 8;
                    px += 16;
                }
                if px + 8 <= width {
                    while px + 8 <= width {
                        yuv422_macropixels_to_bgra8(
                            yd.add(px),
                            ud.add(x),
                            vd.add(x),
                            d.add(px * 4),
                            table,
                        );
                        x += 4;
                        px += 8;
                    }
                }
                while px + 1 < width {
                    scalar_tail_pair(yd, ud, vd, d, x, px, table);
                    x += 1;
                    px += 2;
                }
            }
        }
    }

    #[inline]
    unsafe fn scalar_tail_pair(
        yd: *const u8,
        ud: *const u8,
        vd: *const u8,
        d: *mut u8,
        x: usize,
        px: usize,
        table: &[i16; 5],
    ) {
        unsafe {
            let cb = *ud.add(x) as i16 - 128;
            let cr = *vd.add(x) as i16 - 128;
            for i in 0..2 {
                let (b, g, r) = yuv_to_bgra_pixel(*yd.add(px + i), cb, cr, table);
                let o = (px + i) * 4;
                *d.add(o) = b;
                *d.add(o + 1) = g;
                *d.add(o + 2) = r;
                *d.add(o + 3) = 255;
            }
        }
    }

    /// # Safety
    /// SSE2 required. Buffers must cover the described band.
    #[target_feature(enable = "sse2")]
    pub unsafe fn yuv422_band_to_bgra_sse2(
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
        unsafe {
            for row in 0..rows {
                let yr = y_row0 + row;
                let yd = y.as_ptr().add(yr * y_stride);
                let ud = u.as_ptr().add(yr * u_stride);
                let vd = v.as_ptr().add(yr * v_stride);
                let d = dst.as_mut_ptr().add(yr * dst_stride);
                let mut x = 0usize;
                let mut px = 0usize;
                while px + 8 <= width {
                    yuv422_macropixels_to_bgra8(
                        yd.add(px),
                        ud.add(x),
                        vd.add(x),
                        d.add(px * 4),
                        table,
                    );
                    x += 4;
                    px += 8;
                }
                while px + 1 < width {
                    scalar_tail_pair(yd, ud, vd, d, x, px, table);
                    x += 1;
                    px += 2;
                }
            }
        }
    }

    /// # Safety
    /// AVX2 required. Buffers must cover the described band.
    #[target_feature(enable = "avx2")]
    pub unsafe fn yuv422_band_to_bgra_avx2(
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
        // 16px, then 8px, then scalar tail.
        unsafe {
            for row in 0..rows {
                let yr = y_row0 + row;
                let yd = y.as_ptr().add(yr * y_stride);
                let ud = u.as_ptr().add(yr * u_stride);
                let vd = v.as_ptr().add(yr * v_stride);
                let d = dst.as_mut_ptr().add(yr * dst_stride);
                let mut x = 0usize;
                let mut px = 0usize;
                while px + 16 <= width {
                    yuv422_macropixels_to_bgra16(
                        yd.add(px),
                        ud.add(x),
                        vd.add(x),
                        d.add(px * 4),
                        table,
                    );
                    x += 8;
                    px += 16;
                }
                if px + 8 <= width {
                    _mm256_zeroupper();
                    while px + 8 <= width {
                        yuv422_macropixels_to_bgra8(
                            yd.add(px),
                            ud.add(x),
                            vd.add(x),
                            d.add(px * 4),
                            table,
                        );
                        x += 4;
                        px += 8;
                    }
                }
                while px + 1 < width {
                    scalar_tail_pair(yd, ud, vd, d, x, px, table);
                    x += 1;
                    px += 2;
                }
            }
            if width >= 16 {
                _mm256_zeroupper();
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arm {
    use super::yuv_to_bgra_pixel;
    use std::arch::aarch64::*;

    #[inline]
    unsafe fn mulhi_s16(a: int16x8_t, coeff: i16) -> int16x8_t {
        unsafe {
            let c = vdup_n_s16(coeff);
            let lo = vmull_s16(vget_low_s16(a), c);
            let hi = vmull_s16(vget_high_s16(a), c);
            vcombine_s16(vshrn_n_s32(lo, 16), vshrn_n_s32(hi, 16))
        }
    }

    /// # Safety
    /// NEON required. Buffers must cover the described band.
    #[target_feature(enable = "neon")]
    pub unsafe fn yuv422_band_to_bgra_neon(
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
        unsafe {
            let rounding = vdupq_n_s16(8);
            for row in 0..rows {
                let yr = y_row0 + row;
                let yd = y.as_ptr().add(yr * y_stride);
                let ud = u.as_ptr().add(yr * u_stride);
                let vd = v.as_ptr().add(yr * v_stride);
                let d = dst.as_mut_ptr().add(yr * dst_stride);
                let mut x = 0usize;
                let mut px = 0usize;
                while px + 8 <= width {
                    let y_bytes = vld1_u8(yd.add(px));
                    let y_sat = vqsub_u8(y_bytes, vdup_n_u8(16));
                    let mut y0 = vreinterpretq_s16_u16(vmovl_u8(y_sat));

                    let mut u_bytes = [0u8; 8];
                    let mut v_bytes = [0u8; 8];
                    core::ptr::copy_nonoverlapping(ud.add(x), u_bytes.as_mut_ptr(), 4);
                    core::ptr::copy_nonoverlapping(vd.add(x), v_bytes.as_mut_ptr(), 4);
                    let mut u0 = vsubq_s16(
                        vreinterpretq_s16_u16(vmovl_u8(vld1_u8(u_bytes.as_ptr()))),
                        vdupq_n_s16(128),
                    );
                    let mut v0 = vsubq_s16(
                        vreinterpretq_s16_u16(vmovl_u8(vld1_u8(v_bytes.as_ptr()))),
                        vdupq_n_s16(128),
                    );
                    u0 = vzip1q_s16(u0, u0);
                    v0 = vzip1q_s16(v0, v0);

                    y0 = vshlq_n_s16(y0, 6);
                    y0 = mulhi_s16(y0, table[0]);

                    v0 = vshlq_n_s16(v0, 6);
                    let mut r = mulhi_s16(v0, table[1]);
                    r = vqaddq_s16(r, y0);

                    let mut b = vshlq_n_s16(u0, 7);
                    b = mulhi_s16(b, table[4]);
                    b = vqaddq_s16(b, y0);

                    u0 = vshlq_n_s16(u0, 6);
                    let mut g = mulhi_s16(u0, table[2]);
                    let tmp = mulhi_s16(v0, table[3]);
                    g = vqsubq_s16(y0, g);
                    g = vqsubq_s16(g, tmp);

                    r = vqaddq_s16(r, rounding);
                    g = vqaddq_s16(g, rounding);
                    b = vqaddq_s16(b, rounding);
                    r = vshrq_n_s16(r, 4);
                    g = vshrq_n_s16(g, 4);
                    b = vshrq_n_s16(b, 4);

                    let bu = vqmovun_s16(b);
                    let gu = vqmovun_s16(g);
                    let ru = vqmovun_s16(r);
                    let au = vdup_n_u8(255);
                    let bg = vzip1_u8(bu, gu);
                    let ra = vzip1_u8(ru, au);
                    let bg2 = vzip2_u8(bu, gu);
                    let ra2 = vzip2_u8(ru, au);
                    let lo = vzip1_u16(vreinterpret_u16_u8(bg), vreinterpret_u16_u8(ra));
                    let mid = vzip2_u16(vreinterpret_u16_u8(bg), vreinterpret_u16_u8(ra));
                    let hi0 = vzip1_u16(vreinterpret_u16_u8(bg2), vreinterpret_u16_u8(ra2));
                    let hi1 = vzip2_u16(vreinterpret_u16_u8(bg2), vreinterpret_u16_u8(ra2));
                    vst1_u8(d.add(px * 4), vreinterpret_u8_u16(lo));
                    vst1_u8(d.add(px * 4 + 8), vreinterpret_u8_u16(mid));
                    vst1_u8(d.add(px * 4 + 16), vreinterpret_u8_u16(hi0));
                    vst1_u8(d.add(px * 4 + 24), vreinterpret_u8_u16(hi1));
                    x += 4;
                    px += 8;
                }
                while px + 1 < width {
                    let cb = *ud.add(x) as i16 - 128;
                    let cr = *vd.add(x) as i16 - 128;
                    for i in 0..2 {
                        let (b, g, r) = yuv_to_bgra_pixel(*yd.add(px + i), cb, cr, table);
                        let o = (px + i) * 4;
                        *d.add(o) = b;
                        *d.add(o + 1) = g;
                        *d.add(o + 2) = r;
                        *d.add(o + 3) = 255;
                    }
                    x += 1;
                    px += 2;
                }
            }
        }
    }
}

#[cfg(all(target_arch = "aarch64", feature = "sve"))]
mod arm_sve {
    use super::yuv_to_bgra_pixel;
    use std::arch::aarch64::*;

    /// SVE YUV422 → BGRA using scalable i16 lanes.
    ///
    /// # Safety
    /// Caller must ensure FEAT_SVE. Buffers must cover the described band.
    #[target_feature(enable = "sve")]
    pub unsafe fn yuv422_band_to_bgra_sve(
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
        // Max architectural SVE length is 2048 bits → 128 i16 lanes.
        const MAX_I16: usize = 128;
        unsafe {
            let vl = svcnth() as usize;
            debug_assert!(vl > 0 && vl <= MAX_I16);
            let rounding = svdup_n_s16(8);
            let mut y_tmp = [0i16; MAX_I16];
            let mut u_tmp = [0i16; MAX_I16];
            let mut v_tmp = [0i16; MAX_I16];
            let mut r_tmp = [0i16; MAX_I16];
            let mut g_tmp = [0i16; MAX_I16];
            let mut b_tmp = [0i16; MAX_I16];

            for row in 0..rows {
                let yr = y_row0 + row;
                let yd = y.as_ptr().add(yr * y_stride);
                let ud = u.as_ptr().add(yr * u_stride);
                let vd = v.as_ptr().add(yr * v_stride);
                let d = dst.as_mut_ptr().add(yr * dst_stride);
                let mut px = 0usize;
                while px + 2 <= width {
                    let remaining = (width - px) & !1; // keep even for 4:2:2
                    if remaining == 0 {
                        break;
                    }
                    let n = remaining.min(vl);
                    let pg = svwhilelt_b16_u64(0, n as u64);

                    for i in 0..n {
                        // Match scalar/NEON: saturating u8 subtract before widening.
                        y_tmp[i] = (*yd.add(px + i)).saturating_sub(16) as i16;
                        let c = (px + i) / 2;
                        u_tmp[i] = *ud.add(c) as i16 - 128;
                        v_tmp[i] = *vd.add(c) as i16 - 128;
                    }

                    let mut y0 = svld1_s16(pg, y_tmp.as_ptr());
                    let mut u0 = svld1_s16(pg, u_tmp.as_ptr());
                    let mut v0 = svld1_s16(pg, v_tmp.as_ptr());

                    y0 = svlsl_n_s16_x(pg, y0, 6);
                    y0 = svmulh_n_s16_x(pg, y0, table[0]);

                    v0 = svlsl_n_s16_x(pg, v0, 6);
                    let mut r = svmulh_n_s16_x(pg, v0, table[1]);
                    r = svqadd_s16(r, y0);

                    let mut b = svlsl_n_s16_x(pg, u0, 7);
                    b = svmulh_n_s16_x(pg, b, table[4]);
                    b = svqadd_s16(b, y0);

                    u0 = svlsl_n_s16_x(pg, u0, 6);
                    let mut g = svmulh_n_s16_x(pg, u0, table[2]);
                    let tmp = svmulh_n_s16_x(pg, v0, table[3]);
                    g = svqsub_s16(y0, g);
                    g = svqsub_s16(g, tmp);

                    r = svqadd_s16(r, rounding);
                    g = svqadd_s16(g, rounding);
                    b = svqadd_s16(b, rounding);
                    r = svasr_n_s16_x(pg, r, 4);
                    g = svasr_n_s16_x(pg, g, 4);
                    b = svasr_n_s16_x(pg, b, 4);

                    svst1_s16(pg, r_tmp.as_mut_ptr(), r);
                    svst1_s16(pg, g_tmp.as_mut_ptr(), g);
                    svst1_s16(pg, b_tmp.as_mut_ptr(), b);

                    for i in 0..n {
                        let o = (px + i) * 4;
                        *d.add(o) = b_tmp[i].clamp(0, 255) as u8;
                        *d.add(o + 1) = g_tmp[i].clamp(0, 255) as u8;
                        *d.add(o + 2) = r_tmp[i].clamp(0, 255) as u8;
                        *d.add(o + 3) = 255;
                    }
                    px += n;
                }
                // Odd-width tail (rare for 4:2:2).
                if px + 1 < width {
                    let x = px / 2;
                    let cb = *ud.add(x) as i16 - 128;
                    let cr = *vd.add(x) as i16 - 128;
                    let (b, g, r) = yuv_to_bgra_pixel(*yd.add(px), cb, cr, table);
                    let o = px * 4;
                    *d.add(o) = b;
                    *d.add(o + 1) = g;
                    *d.add(o + 2) = r;
                    *d.add(o + 3) = 255;
                }
            }
        }
    }
}

/// YUV422 band → BGRA via [`ColorSimdPath`].
pub fn yuv422_band_to_bgra_dispatch(
    path: ColorSimdPath,
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
    match path {
        #[cfg(target_arch = "x86_64")]
        ColorSimdPath::Avx512 => unsafe {
            x86::yuv422_band_to_bgra_avx512(
                y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
            )
        },
        #[cfg(target_arch = "x86_64")]
        ColorSimdPath::Avx2 => unsafe {
            x86::yuv422_band_to_bgra_avx2(
                y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
            )
        },
        #[cfg(target_arch = "x86_64")]
        ColorSimdPath::Sse2 => unsafe {
            x86::yuv422_band_to_bgra_sse2(
                y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
            )
        },
        #[cfg(all(target_arch = "aarch64", feature = "sve"))]
        ColorSimdPath::Sve => unsafe {
            arm_sve::yuv422_band_to_bgra_sve(
                y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
            )
        },
        #[cfg(target_arch = "aarch64")]
        ColorSimdPath::Neon => unsafe {
            arm::yuv422_band_to_bgra_neon(
                y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
            )
        },
        #[cfg(feature = "portable-simd")]
        ColorSimdPath::Portable => crate::color::portable::yuv422_band_to_bgra_portable(
            y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
        ),
        _ => crate::color::convert::yuv422_band_to_bgra_scalar(
            y, y_stride, u, u_stride, v, v_stride, y_row0, rows, width, dst, dst_stride, table,
        ),
    }
}

#[cfg(target_arch = "x86_64")]
mod x86_bgra {
    use crate::color::convert::bgra_to_yuv4224_scalar;
    use crate::tables::ShortRgb;
    use crate::types::Size;
    use std::arch::x86_64::*;

    /// Extract channel `pos` from 8 BGRA pixels.
    #[inline]
    #[target_feature(enable = "ssse3")]
    unsafe fn create_rgb_vec(m1: __m128i, m2: __m128i, pos: i8) -> __m128i {
        let r = _mm_shuffle_epi8(
            m1,
            _mm_set_epi8(
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                pos + 12,
                -1,
                pos + 8,
                -1,
                pos + 4,
                -1,
                pos,
            ),
        );
        let tmp = _mm_shuffle_epi8(
            m2,
            _mm_set_epi8(
                -1,
                pos + 12,
                -1,
                pos + 8,
                -1,
                pos + 4,
                -1,
                pos,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
            ),
        );
        _mm_or_si128(r, tmp)
    }

    /// Signed 16-bit U/V path (+128 bias).
    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn convert_rgb_vec(
        r: __m128i,
        g: __m128i,
        b: __m128i,
        mul_r: i16,
        mul_g: i16,
        mul_b: i16,
        add: i16,
    ) -> __m128i {
        let mut y = _mm_mullo_epi16(r, _mm_set1_epi16(mul_r));
        let mut tmp = _mm_mullo_epi16(g, _mm_set1_epi16(mul_g));
        y = _mm_adds_epi16(y, tmp);
        tmp = _mm_mullo_epi16(b, _mm_set1_epi16(mul_b));
        y = _mm_adds_epi16(y, tmp);
        y = _mm_adds_epi16(y, _mm_set1_epi16(128));
        y = _mm_srai_epi16(y, 8);
        y = _mm_adds_epi16(y, _mm_set1_epi16(add));
        y
    }

    /// Unsigned G via 32-bit mul.
    #[inline]
    #[target_feature(enable = "sse4.1")]
    unsafe fn convert_rgb_vec_u(
        r: __m128i,
        g: __m128i,
        b: __m128i,
        mul_r: i16,
        mul_g: i16,
        mul_b: i16,
        add: i16,
    ) -> __m128i {
        let mut y = _mm_mullo_epi16(r, _mm_set1_epi16(mul_r));

        let mut tmp = _mm_cvtepu16_epi32(g);
        tmp = _mm_mullo_epi32(tmp, _mm_set1_epi32(mul_g as i32));
        let mut tmp2 = _mm_srli_si128::<8>(g);
        tmp2 = _mm_cvtepi16_epi32(tmp2);
        tmp2 = _mm_mullo_epi32(tmp2, _mm_set1_epi32(mul_g as i32));
        tmp = _mm_packus_epi32(tmp, tmp2);
        y = _mm_adds_epu16(y, tmp);

        tmp = _mm_mullo_epi16(b, _mm_set1_epi16(mul_b));
        y = _mm_adds_epu16(y, tmp);

        y = _mm_adds_epu16(y, _mm_set1_epi16(128));
        y = _mm_srli_epi16(y, 8);
        y = _mm_adds_epu16(y, _mm_set1_epi16(add));
        y
    }

    /// 16 BGRA pixels → Y/U/V/A planar.
    #[inline]
    #[target_feature(enable = "ssse3,sse4.1")]
    unsafe fn convert_bgra_block(
        m_input: *const __m128i,
        p_y: *mut u8,
        p_u: *mut u8,
        p_v: *mut u8,
        p_a: *mut u8,
        c_y: ShortRgb,
        c_u: ShortRgb,
        c_v: ShortRgb,
    ) {
        unsafe {
            let m1 = _mm_loadu_si128(m_input);
            let m2 = _mm_loadu_si128(m_input.add(1));
            let m3 = _mm_loadu_si128(m_input.add(2));
            let m4 = _mm_loadu_si128(m_input.add(3));

            let a1 = create_rgb_vec(m1, m2, 3);
            let r1 = create_rgb_vec(m1, m2, 2);
            let g1 = create_rgb_vec(m1, m2, 1);
            let b1 = create_rgb_vec(m1, m2, 0);

            let a2 = create_rgb_vec(m3, m4, 3);
            let r2 = create_rgb_vec(m3, m4, 2);
            let g2 = create_rgb_vec(m3, m4, 1);
            let b2 = create_rgb_vec(m3, m4, 0);

            let mut y1 = convert_rgb_vec_u(r1, g1, b1, c_y.r, c_y.g, c_y.b, 16);
            let mut u1 = convert_rgb_vec(r1, g1, b1, c_u.r, c_u.g, c_u.b, 128);
            let mut v1 = convert_rgb_vec(r1, g1, b1, c_v.r, c_v.g, c_v.b, 128);

            let y2 = convert_rgb_vec_u(r2, g2, b2, c_y.r, c_y.g, c_y.b, 16);
            let u2 = convert_rgb_vec(r2, g2, b2, c_u.r, c_u.g, c_u.b, 128);
            let v2 = convert_rgb_vec(r2, g2, b2, c_v.r, c_v.g, c_v.b, 128);

            u1 = _mm_hadd_epi16(u1, u2);
            u1 = _mm_srai_epi16(u1, 1);

            v1 = _mm_hadd_epi16(v1, v2);
            v1 = _mm_srai_epi16(v1, 1);

            y1 = _mm_packus_epi16(y1, y2);
            u1 = _mm_packus_epi16(u1, u1);
            v1 = _mm_packus_epi16(v1, v1);
            let a = _mm_packus_epi16(a1, a2);

            _mm_storeu_si128(p_y.cast(), y1);
            _mm_storel_epi64(p_u.cast(), u1);
            _mm_storel_epi64(p_v.cast(), v1);
            _mm_storeu_si128(p_a.cast(), a);
        }
    }

    /// # Safety
    /// SSSE3 + SSE4.1 required. Buffers must cover `size` with the given strides.
    #[target_feature(enable = "ssse3,sse4.1")]
    pub unsafe fn bgra_to_yuv4224_ssse3(
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
        unsafe {
            let width = size.width as usize;
            let simd_w = width & !15;
            let c_y = table[0];
            let c_u = table[1];
            let c_v = table[2];

            for row in 0..size.height as usize {
                let s = src.as_ptr().add(row * src_stride);
                let yd = y.as_mut_ptr().add(row * y_stride);
                let ud = u.as_mut_ptr().add(row * u_stride);
                let vd = v.as_mut_ptr().add(row * v_stride);
                let ad = a.as_mut_ptr().add(row * a_stride);

                let mut px = 0usize;
                while px < simd_w {
                    convert_bgra_block(
                        s.add(px * 4).cast(),
                        yd.add(px),
                        ud.add(px / 2),
                        vd.add(px / 2),
                        ad.add(px),
                        c_y,
                        c_u,
                        c_v,
                    );
                    px += 16;
                }
            }

            let rem = width - simd_w;
            if rem >= 2 {
                for row in 0..size.height as usize {
                    let s_off = row * src_stride + simd_w * 4;
                    let y_off = row * y_stride + simd_w;
                    let u_off = row * u_stride + simd_w / 2;
                    let v_off = row * v_stride + simd_w / 2;
                    let a_off = row * a_stride + simd_w;
                    bgra_to_yuv4224_scalar(
                        &src[s_off..],
                        rem * 4,
                        &mut y[y_off..],
                        rem,
                        &mut u[u_off..],
                        rem / 2,
                        &mut v[v_off..],
                        rem / 2,
                        &mut a[a_off..],
                        rem,
                        Size::new(rem as i32, 1),
                        table,
                    );
                }
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arm_bgra {
    use crate::color::convert::bgra_to_yuv4224_scalar;
    use crate::tables::ShortRgb;
    use crate::types::Size;
    use std::arch::aarch64::*;

    /// Signed U/V path (8 pixels).
    #[inline]
    unsafe fn convert_rgb_vec(
        r: int16x8_t,
        g: int16x8_t,
        b: int16x8_t,
        mul_r: i16,
        mul_g: i16,
        mul_b: i16,
        add: i16,
    ) -> int16x8_t {
        unsafe {
            let mut y = vmulq_n_s16(r, mul_r);
            y = vqaddq_s16(y, vmulq_n_s16(g, mul_g));
            y = vqaddq_s16(y, vmulq_n_s16(b, mul_b));
            y = vqaddq_s16(y, vdupq_n_s16(128));
            y = vshrq_n_s16(y, 8);
            vqaddq_s16(y, vdupq_n_s16(add))
        }
    }

    /// Unsigned G via 32-bit mul (8 pixels).
    #[inline]
    unsafe fn convert_rgb_vec_u(
        r: uint16x8_t,
        g: uint16x8_t,
        b: uint16x8_t,
        mul_r: i16,
        mul_g: i16,
        mul_b: i16,
        add: i16,
    ) -> uint16x8_t {
        unsafe {
            let mut y = vmulq_n_u16(r, mul_r as u16);
            let g_lo = vmull_n_u16(vget_low_u16(g), mul_g as u16);
            let g_hi = vmull_n_u16(vget_high_u16(g), mul_g as u16);
            let g_packed = vcombine_u16(vqmovn_u32(g_lo), vqmovn_u32(g_hi));
            y = vqaddq_u16(y, g_packed);
            y = vqaddq_u16(y, vmulq_n_u16(b, mul_b as u16));
            y = vqaddq_u16(y, vdupq_n_u16(128));
            y = vshrq_n_u16(y, 8);
            vqaddq_u16(y, vdupq_n_u16(add as u16))
        }
    }

    #[inline]
    unsafe fn hadd_avg_chroma(u1: int16x8_t, u2: int16x8_t) -> int16x8_t {
        unsafe {
            let sum1 = vpadd_s16(vget_low_s16(u1), vget_high_s16(u1));
            let sum2 = vpadd_s16(vget_low_s16(u2), vget_high_s16(u2));
            vshrq_n_s16(vcombine_s16(sum1, sum2), 1)
        }
    }

    /// # Safety
    /// NEON required. Buffers must cover `size` with the given strides.
    #[target_feature(enable = "neon")]
    pub unsafe fn bgra_to_yuv4224_neon(
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
        unsafe {
            let width = size.width as usize;
            let simd_w = width & !15;
            let c_y = table[0];
            let c_u = table[1];
            let c_v = table[2];

            for row in 0..size.height as usize {
                let s = src.as_ptr().add(row * src_stride);
                let yd = y.as_mut_ptr().add(row * y_stride);
                let ud = u.as_mut_ptr().add(row * u_stride);
                let vd = v.as_mut_ptr().add(row * v_stride);
                let ad = a.as_mut_ptr().add(row * a_stride);

                let mut px = 0usize;
                while px < simd_w {
                    let px4 = vld4q_u8(s.add(px * 4));
                    let b = px4.0;
                    let g = px4.1;
                    let r = px4.2;
                    let alpha = px4.3;

                    let r1 = vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(r)));
                    let g1 = vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(g)));
                    let b1 = vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(b)));
                    let r2 = vreinterpretq_s16_u16(vmovl_u8(vget_high_u8(r)));
                    let g2 = vreinterpretq_s16_u16(vmovl_u8(vget_high_u8(g)));
                    let b2 = vreinterpretq_s16_u16(vmovl_u8(vget_high_u8(b)));

                    let y1 = convert_rgb_vec_u(
                        vreinterpretq_u16_s16(r1),
                        vreinterpretq_u16_s16(g1),
                        vreinterpretq_u16_s16(b1),
                        c_y.r,
                        c_y.g,
                        c_y.b,
                        16,
                    );
                    let y2 = convert_rgb_vec_u(
                        vreinterpretq_u16_s16(r2),
                        vreinterpretq_u16_s16(g2),
                        vreinterpretq_u16_s16(b2),
                        c_y.r,
                        c_y.g,
                        c_y.b,
                        16,
                    );
                    let u1 = convert_rgb_vec(r1, g1, b1, c_u.r, c_u.g, c_u.b, 128);
                    let u2 = convert_rgb_vec(r2, g2, b2, c_u.r, c_u.g, c_u.b, 128);
                    let v1 = convert_rgb_vec(r1, g1, b1, c_v.r, c_v.g, c_v.b, 128);
                    let v2 = convert_rgb_vec(r2, g2, b2, c_v.r, c_v.g, c_v.b, 128);

                    let y_bytes = vcombine_u8(vqmovn_u16(y1), vqmovn_u16(y2));
                    let u_avg = hadd_avg_chroma(u1, u2);
                    let v_avg = hadd_avg_chroma(v1, v2);
                    let u_bytes = vqmovun_s16(u_avg);
                    let v_bytes = vqmovun_s16(v_avg);

                    vst1q_u8(yd.add(px), y_bytes);
                    vst1_u8(ud.add(px / 2), u_bytes);
                    vst1_u8(vd.add(px / 2), v_bytes);
                    vst1q_u8(ad.add(px), alpha);
                    px += 16;
                }
            }

            let rem = width - simd_w;
            if rem >= 2 {
                for row in 0..size.height as usize {
                    let s_off = row * src_stride + simd_w * 4;
                    let y_off = row * y_stride + simd_w;
                    let u_off = row * u_stride + simd_w / 2;
                    let v_off = row * v_stride + simd_w / 2;
                    let a_off = row * a_stride + simd_w;
                    bgra_to_yuv4224_scalar(
                        &src[s_off..],
                        rem * 4,
                        &mut y[y_off..],
                        rem,
                        &mut u[u_off..],
                        rem / 2,
                        &mut v[v_off..],
                        rem / 2,
                        &mut a[a_off..],
                        rem,
                        Size::new(rem as i32, 1),
                        table,
                    );
                }
            }
        }
    }
}

/// BGRA → planar 4:2:2:4 via [`ColorSimdPath`].
///
/// [`ColorSimdPath::Avx2`] falls through to SSSE3 128-bit (no AVX2 BGRA encode;
/// matches libvmx). The path name affects metrics/reporting only for encode.
pub fn bgra_to_yuv4224_dispatch(
    path: ColorSimdPath,
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
    match path {
        #[cfg(target_arch = "x86_64")]
        ColorSimdPath::Avx512 | ColorSimdPath::Avx2 | ColorSimdPath::Sse2 => {
            // Avx2 encode falls through here: SSSE3+SSE4.1 128-bit (libvmx has no AVX2 BGRA encode).
            if is_x86_feature_detected!("ssse3") && is_x86_feature_detected!("sse4.1") {
                unsafe {
                    x86_bgra::bgra_to_yuv4224_ssse3(
                        src, src_stride, y, y_stride, u, u_stride, v, v_stride, a, a_stride, size,
                        table,
                    )
                }
            } else {
                crate::color::convert::bgra_to_yuv4224_scalar(
                    src, src_stride, y, y_stride, u, u_stride, v, v_stride, a, a_stride, size,
                    table,
                )
            }
        }
        #[cfg(target_arch = "aarch64")]
        ColorSimdPath::Neon => unsafe {
            arm_bgra::bgra_to_yuv4224_neon(
                src, src_stride, y, y_stride, u, u_stride, v, v_stride, a, a_stride, size, table,
            )
        },
        #[cfg(all(target_arch = "aarch64", feature = "sve"))]
        ColorSimdPath::Sve => unsafe {
            // BGRA→YUV stays on NEON; SVE accelerates YUV→BGRA pack.
            arm_bgra::bgra_to_yuv4224_neon(
                src, src_stride, y, y_stride, u, u_stride, v, v_stride, a, a_stride, size, table,
            )
        },
        _ => crate::color::convert::bgra_to_yuv4224_scalar(
            src, src_stride, y, y_stride, u, u_stride, v, v_stride, a, a_stride, size, table,
        ),
    }
}
