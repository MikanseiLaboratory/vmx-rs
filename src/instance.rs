//! Codec instance — create, encode, decode, container I/O.

use crate::bitrate::{adjust_bitrate, lookup_bitrate};
use crate::codec::slice::{PlaneBuffers, SliceSet, decode_slices, encode_slices};
use crate::color::convert::{
    bgra_to_yuv4224, calculate_psnr, nv12_to_planar, planar_to_uyvy, planar_to_yuy2,
    select_rgb_yuv, select_yuv_rgb, uyvy_to_planar, yuy2_to_planar, yv12_to_planar,
};
use crate::container::{encoded_preview_length, parse_and_load, preview_bitstream_length, save_to};
use crate::error::{Result, VmxError};
use crate::simd::dispatch::CpuFeatures;
use crate::tables::{QUALITY, QUANT_MATRIX};
use crate::thread_pool::ThreadPool;
use crate::types::{
    ALIGNMENT, ColorSpace, DECODE_MATRIX_COUNT, ENCODE_MATRIX_COUNT, Format, ImageFormat,
    MAX_HEIGHT, MAX_Q, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH, Profile, QUALITY_COUNT, SLICE_HEIGHT,
    Size, align_up,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub width: i32,
    pub height: i32,
    pub profile: Profile,
    pub color_space: ColorSpace,
}

impl Config {
    /// Create a config with HQ profile and undefined color space.
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            profile: Profile::Hq,
            color_space: ColorSpace::Undefined,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

pub struct Codec {
    size: Size,
    format: Format,
    #[allow(dead_code)]
    profile: Profile,
    color_space: ColorSpace,
    quality: i32,
    min_quality: i32,
    dc_shift: i32,
    features: CpuFeatures,
    decode_presets: Vec<Vec<u16>>,
    encode_presets: Vec<Vec<u16>>,
    decode_matrix_idx: usize,
    planes: PlaneBuffers,
    slices: Vec<SliceSet>,
    slice_count: usize,
    #[allow(dead_code)]
    aligned_height: i32,
    target_bytes_min: i32,
    target_bytes_max: i32,
    pool: ThreadPool,
    preview_size: Size,
    image_format: ImageFormat,
}

impl Codec {
    pub fn new(config: Config) -> Result<Self> {
        let Config {
            width,
            height,
            mut profile,
            mut color_space,
        } = config;

        if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) || !(MIN_HEIGHT..=MAX_HEIGHT).contains(&height)
        {
            return Err(VmxError::UnsupportedDimensions { width, height });
        }
        if width % 2 != 0 {
            return Err(VmxError::InvalidParameters);
        }

        if profile == Profile::Default {
            profile = Profile::Hq;
        }
        if color_space == ColorSpace::Undefined {
            color_space = if height >= 720 {
                ColorSpace::Bt709
            } else {
                ColorSpace::Bt601
            };
        }

        let mut features = CpuFeatures::detect();
        let br = lookup_bitrate(profile, height);
        // Decode/encode share the pool; prefer enough workers for 1080p59.94
        // rather than the historical bitrate-table default of 2 @ 1080p.
        let nthreads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let mut threads = br.threads as usize;
        if height >= 4320 {
            threads = threads.max(16).min(nthreads);
        } else if height >= 2160 {
            threads = threads.max(8).min(nthreads);
        } else if height >= 1080 {
            threads = threads.max(6).min(nthreads);
        } else {
            threads = threads.max(2).min(nthreads);
        }

        let mut y_stride = width as usize;
        let uv_w = (width / 2) as usize;
        let mut uv_stride = uv_w;
        y_stride = align_up(y_stride as i32, 8) as usize;
        uv_stride = align_up(uv_stride as i32, 8) as usize;
        if !uv_w.is_multiple_of(16) {
            features.avx2 = false;
        }

        let aligned_height = align_up(height, 16);
        let plane_len = y_stride * aligned_height as usize * 2;
        let y = vec![0u8; plane_len];
        let u = vec![128u8; plane_len];
        let v = vec![128u8; plane_len];
        let a = vec![255u8; plane_len];

        let slice_count = (aligned_height >> 4) as usize;
        let dc_len = y_stride * SLICE_HEIGHT as usize * 2;
        let ac_len = y_stride * SLICE_HEIGHT as usize * 4;

        let mut offsets = [0usize; 4];
        let mut offsets16 = [0usize; 4];
        let mut slices = Vec::with_capacity(slice_count);
        for i in 0..slice_count {
            let mut s = SliceSet::new(dc_len, ac_len);
            s.pixel_height = SLICE_HEIGHT;
            if i == slice_count - 1 {
                s.pixel_height = SLICE_HEIGHT - (aligned_height - height);
            }
            s.pixel_height_interlaced = SLICE_HEIGHT;
            let mid = slice_count / 2;
            if (mid > 0 && i == mid - 1) || i == slice_count - 1 {
                s.pixel_height_interlaced = SLICE_HEIGHT - ((aligned_height - height) >> 1);
            }
            s.lower_field = i >= slice_count / 2;
            s.offset = offsets;
            s.offset16 = offsets16;
            offsets[0] += y_stride * SLICE_HEIGHT as usize;
            offsets[1] += uv_stride * SLICE_HEIGHT as usize;
            offsets[2] += uv_stride * SLICE_HEIGHT as usize;
            offsets[3] += y_stride * SLICE_HEIGHT as usize;
            offsets16[0] += y_stride * SLICE_HEIGHT as usize * 2;
            offsets16[1] += uv_stride * SLICE_HEIGHT as usize * 2;
            offsets16[2] += uv_stride * SLICE_HEIGHT as usize * 2;
            offsets16[3] += y_stride * SLICE_HEIGHT as usize * 2;
            slices.push(s);
        }

        let mut decode_presets = Vec::with_capacity(QUALITY_COUNT);
        let mut encode_presets = Vec::with_capacity(QUALITY_COUNT);
        for &quality_scale in QUALITY.iter().take(QUALITY_COUNT) {
            let mut dec = vec![0u16; DECODE_MATRIX_COUNT];
            let mut enc = vec![0u16; ENCODE_MATRIX_COUNT];
            for y in 0..DECODE_MATRIX_COUNT {
                dec[y] = if y == 0 {
                    QUANT_MATRIX[0]
                } else {
                    QUANT_MATRIX[y].wrapping_mul(quality_scale as u16)
                };
                let rc = create_reciprocal(dec[y]);
                enc[y] = rc[0];
                enc[y + 64] = rc[1];
                enc[y + 128] = rc[2];
            }
            decode_presets.push(dec);
            encode_presets.push(enc);
        }

        let mut codec = Self {
            size: Size::new(width, height),
            format: Format::Progressive,
            profile,
            color_space,
            quality: 80,
            min_quality: br.min_quality,
            dc_shift: br.dc_shift,
            features,
            decode_presets,
            encode_presets,
            decode_matrix_idx: 0,
            planes: PlaneBuffers {
                data: [y, u, v, a],
                stride: [y_stride, uv_stride, uv_stride, y_stride],
                width: [width, width / 2, width / 2, width],
                height: [height, height, height, height],
            },
            slices,
            slice_count,
            aligned_height,
            target_bytes_min: br.target_bytes_min,
            target_bytes_max: br.target_bytes_max,
            pool: ThreadPool::new(threads),
            preview_size: Size::new(align_up(width >> 3, 2), height >> 3),
            image_format: ImageFormat::Uyvy,
        };
        codec.set_quality(80);
        let _ = uv_w;
        let _ = ALIGNMENT;
        Ok(codec)
    }

    /// Alias for [`Codec::new`] (public API name).
    pub fn create(config: Config) -> Result<Self> {
        Self::new(config)
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    pub fn set_color_space(&mut self, cs: ColorSpace) {
        self.color_space = if cs == ColorSpace::Undefined {
            if self.size.height >= 720 {
                ColorSpace::Bt709
            } else {
                ColorSpace::Bt601
            }
        } else {
            cs
        };
    }

    pub fn preview_size(&self) -> Size {
        self.preview_size
    }

    pub fn features(&self) -> CpuFeatures {
        self.features
    }

    pub fn threads(&self) -> usize {
        self.pool.thread_count()
    }

    pub fn set_threads(&mut self, threads: usize) {
        self.pool = ThreadPool::new(threads.max(1));
    }

    pub fn quality(&self) -> i32 {
        self.quality
    }

    pub fn set_quality(&mut self, mut q: i32) {
        if q > MAX_Q {
            q = MAX_Q;
        }
        if q < self.min_quality {
            q = self.min_quality;
        }
        self.set_quality_internal(q);
    }

    fn set_quality_internal(&mut self, mut q: i32) {
        let mut index = 0;
        for (i, &quality_scale) in QUALITY.iter().enumerate().take(QUALITY_COUNT) {
            if quality_scale >= (100 - q) {
                q = 100 - quality_scale;
                index = i;
                break;
            }
        }
        self.quality = q;
        self.decode_matrix_idx = index;
    }

    pub fn encoding_parameters(&self) -> (i32, i32, i32, i32) {
        (
            self.target_bytes_min,
            self.target_bytes_max,
            self.min_quality,
            self.dc_shift,
        )
    }

    pub fn set_encoding_parameters(
        &mut self,
        frame_min: i32,
        frame_max: i32,
        min_quality: i32,
        dc_shift: i32,
    ) {
        self.target_bytes_min = frame_min;
        self.target_bytes_max = frame_max;
        self.min_quality = min_quality;
        self.dc_shift = dc_shift;
    }

    fn encode_planes(&mut self) {
        let plane_count = match self.image_format {
            ImageFormat::Bgra | ImageFormat::Bgrx | ImageFormat::Uyva | ImageFormat::Pa16 => 4,
            _ => 3,
        };
        let dc_shift = self.dc_shift;
        let idx = self.decode_matrix_idx;
        encode_slices(
            &self.planes,
            &mut self.slices,
            &self.encode_presets[idx],
            dc_shift,
            plane_count,
            Some(&self.pool),
        );
    }

    fn decode_planes(&mut self) {
        let dc_shift = self.dc_shift;
        let idx = self.decode_matrix_idx;
        decode_slices(
            &mut self.planes,
            &mut self.slices,
            &self.decode_presets[idx],
            dc_shift,
            Some(&self.pool),
        );
    }

    pub fn save_to(&mut self, dst: &mut [u8]) -> Result<usize> {
        let len = save_to(dst, &self.slices, self.format, self.quality, self.dc_shift)?;
        adjust_bitrate(
            &mut self.quality,
            self.min_quality,
            len as i32,
            self.target_bytes_min,
            self.target_bytes_max,
        );
        // Re-sync matrix after quality tweak for next frame
        let q = self.quality;
        self.set_quality_internal(q);
        Ok(len)
    }

    pub fn load_from(&mut self, data: &[u8]) -> Result<()> {
        let header = parse_and_load(data, self.slice_count, &mut self.slices)?;
        self.set_quality_internal(header.quality);
        self.configure_interlaced(header.format);
        self.dc_shift = header.dc_shift;
        Ok(())
    }

    fn configure_interlaced(&mut self, format: Format) {
        // Interlaced decode is not supported — always treat as progressive pack.
        let _ = format;
        self.format = Format::Progressive;
    }

    pub fn get_encoded_preview_length(&self) -> usize {
        encoded_preview_length(&self.slices, self.format, self.quality, self.dc_shift)
    }

    pub fn encode_uyvy(&mut self, src: &[u8], stride: usize) -> Result<()> {
        self.image_format = ImageFormat::Uyvy;
        let (y, uv) = self.planes.data.split_at_mut(1);
        let (u, rest) = uv.split_at_mut(1);
        let (v, _) = rest.split_at_mut(1);
        uyvy_to_planar(
            src,
            stride,
            &mut y[0],
            self.planes.stride[0],
            &mut u[0],
            self.planes.stride[1],
            &mut v[0],
            self.planes.stride[2],
            self.size,
        );
        self.encode_planes();
        Ok(())
    }

    pub fn decode_uyvy(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.validate_output(dst, stride, 2)?;
        self.decode_planes();
        planar_to_uyvy(
            &self.planes.data[0],
            self.planes.stride[0],
            &self.planes.data[1],
            self.planes.stride[1],
            &self.planes.data[2],
            self.planes.stride[2],
            dst,
            stride,
            self.size,
        );
        Ok(())
    }

    fn validate_output(&self, dst: &[u8], stride: usize, bpp: usize) -> Result<()> {
        Self::validate_dims(dst, stride, bpp, self.size)
    }

    fn validate_preview_output(&self, dst: &[u8], stride: usize, bpp: usize) -> Result<()> {
        Self::validate_dims(dst, stride, bpp, self.preview_size)
    }

    fn validate_dims(dst: &[u8], stride: usize, bpp: usize, size: Size) -> Result<()> {
        let min_stride = size.width as usize * bpp;
        if stride < min_stride {
            return Err(VmxError::InvalidParameters);
        }
        let need = stride
            .checked_mul(size.height as usize)
            .ok_or(VmxError::InvalidParameters)?;
        if dst.len() < need {
            return Err(VmxError::OutputTooSmall {
                need,
                have: dst.len(),
            });
        }
        Ok(())
    }

    /// DC-only decode into internal planes (one 8×8 DC broadcast per block).
    fn decode_planes_preview(&mut self) {
        for slice in self.slices.iter_mut() {
            slice.dc.pos = 0;
            slice.dc.bits_left = crate::types::BITS_SIZE;
            slice.dc.temp_read = {
                let mut buf = [0u8; 8];
                let n = 8.min(slice.dc.stream.len());
                buf[..n].copy_from_slice(&slice.dc.stream[..n]);
                u64::from_be_bytes(buf)
            };
            for pi in 0..3 {
                let mut view = crate::codec::plane::PlaneView {
                    index: pi,
                    data: self.planes.data[pi].as_mut_slice(),
                    stride: self.planes.stride[pi],
                    offset: slice.offset[pi],
                };
                crate::codec::preview::decode_plane_preview(
                    &mut view,
                    &mut slice.dc,
                    self.dc_shift,
                );
            }
        }
    }

    pub fn encode_yuy2(&mut self, src: &[u8], stride: usize) -> Result<()> {
        self.image_format = ImageFormat::Yuy2;
        let (y, uv) = self.planes.data.split_at_mut(1);
        let (u, rest) = uv.split_at_mut(1);
        let (v, _) = rest.split_at_mut(1);
        yuy2_to_planar(
            src,
            stride,
            &mut y[0],
            self.planes.stride[0],
            &mut u[0],
            self.planes.stride[1],
            &mut v[0],
            self.planes.stride[2],
            self.size,
        );
        self.encode_planes();
        Ok(())
    }

    pub fn decode_yuy2(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.validate_output(dst, stride, 2)?;
        self.decode_planes();
        planar_to_yuy2(
            &self.planes.data[0],
            self.planes.stride[0],
            &self.planes.data[1],
            self.planes.stride[1],
            &self.planes.data[2],
            self.planes.stride[2],
            dst,
            stride,
            self.size,
        );
        Ok(())
    }

    pub fn encode_bgra(&mut self, src: &[u8], stride: usize) -> Result<()> {
        self.image_format = ImageFormat::Bgra;
        let table = select_rgb_yuv(self.color_space, self.size.height);
        let stride_y = self.planes.stride[0];
        let stride_u = self.planes.stride[1];
        let stride_v = self.planes.stride[2];
        let stride_a = self.planes.stride[3];
        let size = self.size;
        let (y, rest) = self.planes.data.split_at_mut(1);
        let (u, rest) = rest.split_at_mut(1);
        let (v, a) = rest.split_at_mut(1);
        bgra_to_yuv4224(
            src, stride, &mut y[0], stride_y, &mut u[0], stride_u, &mut v[0], stride_v, &mut a[0],
            stride_a, size, table,
        );
        self.encode_planes();
        // Also encode alpha plane — currently encode_slices encodes all 4; OK
        Ok(())
    }

    pub fn decode_bgra(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.validate_output(dst, stride, 4)?;
        let table = select_yuv_rgb(self.color_space, self.size.height);
        let dc_shift = self.dc_shift;
        let idx = self.decode_matrix_idx;
        crate::codec::slice::decode_slices_fused_bgra(
            &mut self.planes,
            &mut self.slices,
            &self.decode_presets[idx],
            dc_shift,
            Some(&self.pool),
            dst,
            stride,
            self.size.width,
            table,
        );
        Ok(())
    }

    pub fn encode_bgrx(&mut self, src: &[u8], stride: usize) -> Result<()> {
        self.encode_bgra(src, stride)
    }

    pub fn decode_bgrx(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.decode_bgra(dst, stride)
    }

    pub fn encode_nv12(
        &mut self,
        src_y: &[u8],
        stride_y: usize,
        src_uv: &[u8],
        stride_uv: usize,
    ) -> Result<()> {
        self.image_format = ImageFormat::Nv12;
        let (y, uv) = self.planes.data.split_at_mut(1);
        let (u, rest) = uv.split_at_mut(1);
        let (v, _) = rest.split_at_mut(1);
        nv12_to_planar(
            src_y,
            stride_y,
            src_uv,
            stride_uv,
            &mut y[0],
            self.planes.stride[0],
            &mut u[0],
            self.planes.stride[1],
            &mut v[0],
            self.planes.stride[2],
            self.size,
        );
        self.encode_planes();
        Ok(())
    }

    pub fn encode_yv12(
        &mut self,
        src_y: &[u8],
        stride_y: usize,
        src_u: &[u8],
        stride_u: usize,
        src_v: &[u8],
        stride_v: usize,
    ) -> Result<()> {
        self.image_format = ImageFormat::Yv12;
        let (y, uv) = self.planes.data.split_at_mut(1);
        let (u, rest) = uv.split_at_mut(1);
        let (v, _) = rest.split_at_mut(1);
        yv12_to_planar(
            src_y,
            stride_y,
            src_u,
            stride_u,
            src_v,
            stride_v,
            &mut y[0],
            self.planes.stride[0],
            &mut u[0],
            self.planes.stride[1],
            &mut v[0],
            self.planes.stride[2],
            self.size,
        );
        self.encode_planes();
        Ok(())
    }

    /// Alpha / UYVA is not supported in this build.
    pub fn encode_uyva(&mut self, _src: &[u8], _stride: usize) -> Result<()> {
        Err(VmxError::InvalidParameters)
    }

    /// Alpha / UYVA is not supported in this build.
    pub fn decode_uyva(&mut self, _dst: &mut [u8], _stride: usize) -> Result<()> {
        Err(VmxError::InvalidParameters)
    }

    /// 10-bit P216 is not supported in this build.
    pub fn encode_p216(
        &mut self,
        _src_y: &[u8],
        _stride_y: usize,
        _src_uv: &[u8],
        _stride_uv: usize,
    ) -> Result<()> {
        Err(VmxError::InvalidParameters)
    }

    /// 10-bit P216 is not supported in this build.
    pub fn decode_p216(
        &mut self,
        _dst_y: &mut [u8],
        _stride_y: usize,
        _dst_uv: &mut [u8],
        _stride_uv: usize,
    ) -> Result<()> {
        Err(VmxError::InvalidParameters)
    }

    /// 10-bit PA16 is not supported in this build.
    pub fn encode_pa16(
        &mut self,
        _src_y: &[u8],
        _stride_y: usize,
        _src_uv: &[u8],
        _stride_uv: usize,
        _src_a: &[u8],
        _stride_a: usize,
    ) -> Result<()> {
        Err(VmxError::InvalidParameters)
    }

    /// 10-bit PA16 is not supported in this build.
    pub fn decode_pa16(
        &mut self,
        _dst_y: &mut [u8],
        _stride_y: usize,
        _dst_uv: &mut [u8],
        _stride_uv: usize,
        _dst_a: &mut [u8],
        _stride_a: usize,
    ) -> Result<()> {
        Err(VmxError::InvalidParameters)
    }

    /// Decode a 1/8 progressive preview as packed UYVY.
    ///
    /// Codec dimensions must match the **full** frame; output size is
    /// [`Self::preview_size`]. Interlaced preview is not supported.
    pub fn decode_preview_uyvy(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.validate_preview_output(dst, stride, 2)?;
        self.decode_planes_preview();
        let pw = self.preview_size.width as usize;
        let ph = self.preview_size.height as usize;
        for row in 0..ph {
            for x in 0..pw / 2 {
                let sx = x * 8;
                let sy = row * 8;
                let y0 = self.planes.data[0][sy * self.planes.stride[0] + sx * 2];
                let y1 = self.planes.data[0][sy * self.planes.stride[0] + sx * 2 + 1];
                let u = self.planes.data[1][sy * self.planes.stride[1] + sx];
                let v = self.planes.data[2][sy * self.planes.stride[2] + sx];
                let o = row * stride + x * 4;
                dst[o] = u;
                dst[o + 1] = y0;
                dst[o + 2] = v;
                dst[o + 3] = y1;
            }
        }
        Ok(())
    }

    /// Decode a 1/8 progressive preview as packed BGRA8 (alpha = 255).
    ///
    /// Codec dimensions must match the **full** frame; output size is
    /// [`Self::preview_size`]. Interlaced preview is not supported.
    pub fn decode_preview_bgra(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.validate_preview_output(dst, stride, 4)?;
        self.decode_planes_preview();
        let table = select_yuv_rgb(self.color_space, self.size.height);
        let pw = self.preview_size.width as usize;
        let ph = self.preview_size.height as usize;
        let y_stride = self.planes.stride[0];
        let u_stride = self.planes.stride[1];
        let v_stride = self.planes.stride[2];
        let y_plane = &self.planes.data[0];
        let u_plane = &self.planes.data[1];
        let v_plane = &self.planes.data[2];
        for row in 0..ph {
            let sy = row * 8;
            let d = &mut dst[row * stride..];
            let mut x = 0usize;
            let mut px = 0usize;
            while px + 1 < pw {
                let sx = x * 8;
                let cb = u_plane[sy * u_stride + sx] as i32 - 128;
                let cr = v_plane[sy * v_stride + sx] as i32 - 128;
                for i in 0..2 {
                    // Full-plane Y is 4:2:2 packed as Y0 Y1 per macropixel at sx*2.
                    let yy = y_plane[sy * y_stride + sx * 2 + i] as i32;
                    let y_term = (table[0] as i32 * (yy - 16)) >> 14;
                    let r = y_term + ((table[1] as i32 * cr) >> 14);
                    let g =
                        y_term - ((table[2] as i32 * cb) >> 14) - ((table[3] as i32 * cr) >> 14);
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
        Ok(())
    }

    /// Alias for [`Self::decode_preview_bgra`] (opaque alpha).
    pub fn decode_preview_bgrx(&mut self, dst: &mut [u8], stride: usize) -> Result<()> {
        self.decode_preview_bgra(dst, stride)
    }

    /// DC-prefix length of `data` without a live codec instance.
    ///
    /// See [`preview_bitstream_length`].
    pub fn preview_payload_len(data: &[u8]) -> Result<usize> {
        preview_bitstream_length(data)
    }

    pub fn calculate_psnr(&self, a: &[u8], b: &[u8], stride: usize, bpp: usize) -> f32 {
        calculate_psnr(a, b, stride, bpp, self.size)
    }
}

fn create_reciprocal(divisor: u16) -> [u16; 3] {
    if divisor == 1 {
        return [0, 1, 1];
    }
    let b = (16 - divisor.leading_zeros() as i32) - 1;
    let mut r = 2 * 8 + b;
    let mut fq = (1u32 << r) / divisor as u32;
    let fr = (1u32 << r) % divisor as u32;
    let mut c = divisor / 2;
    if fr == 0 {
        fq >>= 1;
        r -= 1;
    } else if fr <= (divisor as u32 / 2) {
        c += 1;
    } else {
        fq += 1;
    }
    let s = 1u16 << (2 * 8 * 2 - r) as u16;
    [c, fq as u16, s]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_roundtrip_uyvy_smoke() {
        let mut enc = Codec::new(Config::new(64, 64)).unwrap();
        let stride = 64 * 2;
        let mut frame = vec![0u8; stride * 64];
        for (i, b) in frame.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }
        enc.encode_uyvy(&frame, stride).unwrap();
        let mut bitstream = vec![0u8; 1024 * 1024];
        let len = enc.save_to(&mut bitstream).unwrap();
        assert!(len > 3);

        let mut dec = Codec::new(Config::new(64, 64)).unwrap();
        dec.load_from(&bitstream[..len]).unwrap();
        let mut out = vec![0u8; stride * 64];
        dec.decode_uyvy(&mut out, stride).unwrap();
        // Smoke: output not all zeros for patterned input
        assert!(out.iter().any(|&b| b != 0) || frame.iter().all(|&b| b == 0));
    }
}
