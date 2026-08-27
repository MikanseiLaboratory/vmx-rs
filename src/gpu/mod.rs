pub(crate) mod pack;

use crate::codec::slice::{decode_slices_coeffs, encode_slices_from_coeffs};
use crate::color::convert::select_yuv_rgb;
use crate::error::{Result, VmxError};
use crate::instance::Codec;
use crate::tables::{
    FTAB1_128, FTAB2_128, FTAB3_128, FTAB4_128, IDCT_ROW_TABLES, RGB_YUV_601, RGB_YUV_709,
    ZIGZAG_INV,
};
use crate::types::{ColorSpace, align_up};

const RING: usize = 3;
const COPY_ALIGN: u32 = 256;

/// Decoded (or preview) frame on the caller's device.
#[derive(Debug, Clone)]
pub struct GpuFrame {
    /// `Bgra8Unorm` texture (`TEXTURE_BINDING | COPY_DST | COPY_SRC`).
    pub texture: wgpu::Texture,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Submit index; work is waited before return from decode APIs.
    pub submission_index: wgpu::SubmissionIndex,
}

/// Try to obtain a headless wgpu device (WARP / llvmpipe / discrete). `None` if none.
pub fn request_headless_device()
-> Option<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY | wgpu::Backends::GL,
        ..Default::default()
    });
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) {
        Ok(a) => a,
        Err(_) => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: true,
        }))
        .ok()?,
    };
    let mut required = wgpu::Features::empty();
    if adapter
        .features()
        .contains(wgpu::Features::BGRA8UNORM_STORAGE)
    {
        required |= wgpu::Features::BGRA8UNORM_STORAGE;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("vmx-headless"),
        required_features: required,
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((instance, adapter, device, queue))
}

pub(crate) fn wait(device: &wgpu::Device, index: &wgpu::SubmissionIndex) {
    let _ = device.poll(wgpu::PollType::WaitForSubmissionIndex(index.clone()));
}

fn padded_bpr(width: u32) -> u32 {
    let raw = width.saturating_mul(4);
    raw.div_ceil(COPY_ALIGN) * COPY_ALIGN
}

fn y_stride(width: u32) -> u32 {
    align_up(width as i32, 8) as u32
}

fn uv_stride(width: u32) -> u32 {
    align_up((width / 2) as i32, 8) as u32
}

fn aligned_height(height: u32) -> u32 {
    align_up(height as i32, 16) as u32
}

fn buf(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(4),
        usage,
        mapped_at_creation: false,
    })
}

fn write_bytes(queue: &wgpu::Queue, buffer: &wgpu::Buffer, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut storage;
    let data = if bytes.len().is_multiple_of(4) {
        bytes
    } else {
        storage = bytes.to_vec();
        storage.resize(bytes.len().div_ceil(4) * 4, 0);
        &storage
    };
    queue.write_buffer(buffer, 0, data);
}

fn write_pod<T: bytemuck::Pod>(queue: &wgpu::Queue, buffer: &wgpu::Buffer, value: &T) {
    write_bytes(queue, buffer, bytemuck::bytes_of(value));
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FusedParams {
    y_blocks_x: u32,
    u_blocks_x: u32,
    width: u32,
    height: u32,
    dst_stride: u32,
    yuv_y: i32,
    yuv_r: i32,
    yuv_gu: i32,
    yuv_gv: i32,
    yuv_b: i32,
    _pad0: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ColorParams {
    width: u32,
    height: u32,
    y_stride: u32,
    u_stride: u32,
    v_stride: u32,
    dst_stride: u32,
    yuv_y: i32,
    yuv_r: i32,
    yuv_gu: i32,
    yuv_gv: i32,
    yuv_b: i32,
    _pad: i32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EncodeParams {
    width: u32,
    height: u32,
    src_stride: u32,
    y_stride: u32,
    u_stride: u32,
    v_stride: u32,
    y_blocks_x: u32,
    y_blocks_y: u32,
    u_blocks_x: u32,
    u_blocks_y: u32,
    add_y: i32,
    add_uv: i32,
    u_plane_off: u32,
    v_plane_off: u32,
    u_coeff_off: u32,
    v_coeff_off: u32,
    u_r: i32,
    u_g: i32,
    u_b: i32,
    v_r: i32,
    v_g: i32,
    v_b: i32,
    y_r: i32,
    y_g: i32,
    y_b: i32,
    a_plane_off: u32,
    a_coeff_off: u32,
    src_rgba: u32,
}

struct PlaneGpu {
    coeff: wgpu::Buffer,
    plane: wgpu::Buffer,
    blocks_x: u32,
    blocks_y: u32,
}

#[allow(dead_code)]
pub(crate) struct GpuSession {
    width: u32,
    height: u32,
    y: PlaneGpu,
    u: PlaneGpu,
    v: PlaneGpu,
    decode_tables: wgpu::Buffer,
    color_ubo: wgpu::Buffer,
    color_bind: wgpu::BindGroup,
    color_pipeline: wgpu::ComputePipeline,
    fused_ubo: wgpu::Buffer,
    fused_bind: wgpu::BindGroup,
    fused_pipeline: wgpu::ComputePipeline,
    bgra_buf: wgpu::Buffer,
    pack_y: Vec<u8>,
    pack_u: Vec<u8>,
    pack_v: Vec<u8>,
    textures: Vec<wgpu::Texture>,
    preview_textures: Vec<wgpu::Texture>,
    ring: usize,
    preview_ring: usize,
    color_encode_pipeline: wgpu::ComputePipeline,
    fdct_y_pipeline: wgpu::ComputePipeline,
    fdct_u_pipeline: wgpu::ComputePipeline,
    fdct_v_pipeline: wgpu::ComputePipeline,
    fdct_a_pipeline: wgpu::ComputePipeline,
    src_buf: wgpu::Buffer,
    enc_ubo: wgpu::Buffer,
    enc_tables: wgpu::Buffer,
    yuv: wgpu::Buffer,
    coeffs: wgpu::Buffer,
    coeff_read: wgpu::Buffer,
    enc_bind: wgpu::BindGroup,
    y_coeff_count: u32,
    u_coeff_count: u32,
    a_coeff_count: u32,
}

fn ubo_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bind<'a>(binding: u32, buf: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

fn compute_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
    label: &str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn make_plane(device: &wgpu::Device, stride: u32, height: u32, label: &str) -> PlaneGpu {
    let blocks_x = (stride / 8).max(1);
    let blocks_y = (height / 8).max(1);
    let n = (blocks_x * blocks_y) as u64;
    let coeff = buf(
        device,
        n * 32,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        &format!("{label}-coeff"),
    );
    let plane = buf(
        device,
        u64::from(stride) * u64::from(height) * 4,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        &format!("{label}-plane"),
    );
    PlaneGpu {
        coeff,
        plane,
        blocks_x,
        blocks_y,
    }
}

impl GpuSession {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        preview_w: u32,
        preview_h: u32,
    ) -> Self {
        let fused_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vmx-fused-bgl"),
            entries: &[
                ubo_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, true),
                storage_entry(4, true),
                storage_entry(5, false),
            ],
        });
        let color_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vmx-color-bgl"),
            entries: &[
                ubo_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, true),
                storage_entry(4, false),
            ],
        });
        let encode_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vmx-encode-bgl"),
            entries: &[
                ubo_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
                storage_entry(4, false),
            ],
        });

        let fused_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-decode"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/decode.wgsl").into()),
        });
        let color_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-color"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/color.wgsl").into()),
        });
        let encode_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-encode"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/encode.wgsl").into()),
        });

        let fused_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vmx-fused-pl"),
            bind_group_layouts: &[&fused_layout],
            push_constant_ranges: &[],
        });
        let color_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vmx-color-pl"),
            bind_group_layouts: &[&color_layout],
            push_constant_ranges: &[],
        });
        let encode_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vmx-encode-pl"),
            bind_group_layouts: &[&encode_layout],
            push_constant_ranges: &[],
        });

        let fused_pipeline =
            compute_pipeline(device, &fused_pl, &fused_mod, "main", "vmx-fused-pipe");
        let color_pipeline =
            compute_pipeline(device, &color_pl, &color_mod, "main", "vmx-color-pipe");
        let color_encode_pipeline = compute_pipeline(
            device,
            &encode_pl,
            &encode_mod,
            "color_main",
            "vmx-enc-color",
        );
        let fdct_y_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_y", "vmx-fdct-y");
        let fdct_u_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_u", "vmx-fdct-u");
        let fdct_v_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_v", "vmx-fdct-v");
        let fdct_a_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_a", "vmx-fdct-a");

        let decode_tables = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("idct-tables"),
            size: 320 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        {
            let mut data = decode_tables.slice(..).get_mapped_range_mut();
            let mut tables = Vec::with_capacity(320);
            for row in IDCT_ROW_TABLES {
                tables.extend(row.iter().map(|&v| i32::from(v)));
            }
            tables.resize(320, 0);
            data.copy_from_slice(bytemuck::cast_slice(&tables));
        }
        decode_tables.unmap();
        let ys = y_stride(width);
        let us = uv_stride(width);
        let ah = aligned_height(height);
        let y = make_plane(device, ys, ah, "y");
        let u = make_plane(device, us, ah, "u");
        let v = make_plane(device, us, ah, "v");

        let dst_stride = padded_bpr(width) / 4;
        let bgra_buf = buf(
            device,
            u64::from(dst_stride) * u64::from(height.max(preview_h)) * 4,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            "bgra-out",
        );
        let color_ubo = buf(
            device,
            48,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "color-ubo",
        );
        let color_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("color-bg"),
            layout: &color_layout,
            entries: &[
                bind(0, &color_ubo),
                bind(1, &y.plane),
                bind(2, &u.plane),
                bind(3, &v.plane),
                bind(4, &bgra_buf),
            ],
        });

        let fused_ubo = buf(
            device,
            48,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "fused-ubo",
        );
        let fused_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fused-bg"),
            layout: &fused_layout,
            entries: &[
                bind(0, &fused_ubo),
                bind(1, &decode_tables),
                bind(2, &y.coeff),
                bind(3, &u.coeff),
                bind(4, &v.coeff),
                bind(5, &bgra_buf),
            ],
        });

        let y_bx = ys / 8;
        let y_by = ah / 8;
        let u_bx = us / 8;
        let u_by = ah / 8;
        let y_coeff_count = y_bx * y_by * 64;
        let u_coeff_count = u_bx * u_by * 64;
        let a_coeff_count = y_coeff_count;
        let coeff_i16 = y_coeff_count + u_coeff_count * 2 + a_coeff_count;
        let y_len = ys * ah;
        let u_len = us * ah;
        let v_len = us * ah;
        let a_len = y_len;

        let src_buf = buf(
            device,
            u64::from(padded_bpr(width)) * u64::from(height),
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            "enc-src",
        );
        let enc_ubo = buf(
            device,
            112,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "enc-ubo",
        );
        let enc_tables = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("enc-tables"),
            size: 384 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        {
            let mut data = enc_tables.slice(..).get_mapped_range_mut();
            let mut tables = Vec::with_capacity(384);
            for t in [&FTAB1_128, &FTAB2_128, &FTAB3_128, &FTAB4_128] {
                tables.extend(t.iter().map(|&v| i32::from(v)));
            }
            tables.extend(ZIGZAG_INV.iter().map(|&v| i32::from(v)));
            tables.resize(384, 0);
            data.copy_from_slice(bytemuck::cast_slice(&tables));
        }
        enc_tables.unmap();
        let yuv = buf(
            device,
            u64::from(y_len + u_len + v_len + a_len) * 4,
            wgpu::BufferUsages::STORAGE,
            "yuv",
        );
        let coeffs = buf(
            device,
            u64::from(coeff_i16) * 2,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            "coeffs",
        );
        let coeff_read = buf(
            device,
            u64::from(coeff_i16) * 2,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            "coeff-read",
        );
        let enc_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("enc-bg"),
            layout: &encode_layout,
            entries: &[
                bind(0, &enc_ubo),
                bind(1, &src_buf),
                bind(2, &enc_tables),
                bind(3, &yuv),
                bind(4, &coeffs),
            ],
        });

        let textures = (0..RING)
            .map(|_| make_bgra_texture(device, width, height))
            .collect();
        let preview_textures = (0..RING)
            .map(|_| make_bgra_texture(device, preview_w.max(2), preview_h.max(2)))
            .collect();

        Self {
            width,
            height,
            y,
            u,
            v,
            decode_tables,
            color_ubo,
            color_bind,
            color_pipeline,
            fused_ubo,
            fused_bind,
            fused_pipeline,
            bgra_buf,
            pack_y: Vec::new(),
            pack_u: Vec::new(),
            pack_v: Vec::new(),
            textures,
            preview_textures,
            ring: 0,
            preview_ring: 0,
            color_encode_pipeline,
            fdct_y_pipeline,
            fdct_u_pipeline,
            fdct_v_pipeline,
            fdct_a_pipeline,
            src_buf,
            enc_ubo,
            enc_tables,
            yuv,
            coeffs,
            coeff_read,
            enc_bind,
            y_coeff_count,
            u_coeff_count,
            a_coeff_count,
        }
    }
}

fn make_bgra_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vmx-bgra"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn gpu_err(msg: impl Into<String>) -> VmxError {
    VmxError::Gpu(msg.into())
}

fn yuv_table(cs: ColorSpace, height: i32) -> ColorParams {
    let t = select_yuv_rgb(cs, height);
    ColorParams {
        width: 0,
        height: 0,
        y_stride: 0,
        u_stride: 0,
        v_stride: 0,
        dst_stride: 0,
        yuv_y: i32::from(t[0]),
        yuv_r: i32::from(t[1]),
        yuv_gu: i32::from(t[2]),
        yuv_gv: i32::from(t[3]),
        yuv_b: i32::from(t[4]),
        _pad: 0,
    }
}

impl Codec {
    fn ensure_gpu(&mut self, device: &wgpu::Device) {
        let w = self.size.width as u32;
        let h = self.size.height as u32;
        let pw = self.preview_size.width as u32;
        let ph = self.preview_size.height as u32;
        let need = match &self.gpu {
            Some(g) => g.width != w || g.height != h,
            None => true,
        };
        if need {
            self.gpu = Some(GpuSession::new(device, w, h, pw, ph));
        }
    }

    /// Decode the loaded bitstream into a BGRA texture on `device`.
    pub fn decode_to_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<GpuFrame> {
        self.ensure_gpu(device);
        let strides = [
            self.planes.stride[0],
            self.planes.stride[1],
            self.planes.stride[2],
        ];
        let t0 = std::time::Instant::now();
        let mut gpu = self.gpu.take().expect("gpu");
        decode_slices_coeffs(
            &mut self.slices,
            strides,
            self.dc_shift,
            &mut gpu.pack_y,
            &mut gpu.pack_u,
            &mut gpu.pack_v,
        );
        self.gpu = Some(gpu);
        let t1 = std::time::Instant::now();
        let idx = self.decode_matrix_idx;
        let matrix: Vec<i32> = self.decode_presets[idx]
            .iter()
            .map(|&m| i32::from(m as i16))
            .collect();
        let frame = self.dispatch_decode(device, queue, &matrix)?;
        if std::env::var_os("VMX_GPU_TRACE").is_some() {
            eprintln!(
                "gpu_trace golomb_pack={:.3}ms dispatch={:.3}ms",
                t1.duration_since(t0).as_secs_f64() * 1e3,
                t1.elapsed().as_secs_f64() * 1e3
            );
        }
        Ok(frame)
    }

    /// Decode a 1/8 preview into a BGRA texture on `device`.
    pub fn decode_preview_to_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<GpuFrame> {
        self.decode_planes_preview();
        self.ensure_gpu(device);
        let pw = self.preview_size.width as u32;
        let ph = self.preview_size.height as u32;
        let y_stride = self.planes.stride[0] as u32;
        let u_stride = self.planes.stride[1] as u32;
        let v_stride = self.planes.stride[2] as u32;
        let y = plane_bytes_to_u32(&self.planes.data[0], y_stride, ph);
        let u = plane_bytes_to_u32(&self.planes.data[1], u_stride, ph);
        let v = plane_bytes_to_u32(&self.planes.data[2], v_stride, ph);
        self.dispatch_preview(
            device, queue, &y, &u, &v, y_stride, u_stride, v_stride, pw, ph,
        )
    }

    /// Encode a BGRA (or RGBA) texture already on `device`. Follow with [`Codec::save_to`].
    pub fn encode_from_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Result<()> {
        let desc = texture.size();
        if desc.width != self.size.width as u32 || desc.height != self.size.height as u32 {
            return Err(gpu_err(format!(
                "texture size {}x{} != codec {}x{}",
                desc.width, desc.height, self.size.width, self.size.height
            )));
        }
        if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            return Err(gpu_err("texture must have COPY_SRC"));
        }
        match texture.format() {
            wgpu::TextureFormat::Bgra8Unorm
            | wgpu::TextureFormat::Bgra8UnormSrgb
            | wgpu::TextureFormat::Rgba8Unorm
            | wgpu::TextureFormat::Rgba8UnormSrgb => {}
            other => {
                return Err(gpu_err(format!(
                    "unsupported texture format {other:?} (need Bgra8Unorm or Rgba8Unorm)"
                )));
            }
        }
        self.ensure_gpu(device);
        self.image_format = crate::types::ImageFormat::Bgra;
        self.dispatch_encode(device, queue, texture)
    }

    fn upload_idct_tables(&self, queue: &wgpu::Queue, matrix: &[i32]) {
        let gpu = self.gpu.as_ref().expect("gpu");
        let mut m = [0i32; 64];
        for (dst, src) in m.iter_mut().zip(matrix.iter()) {
            *dst = *src;
        }
        queue.write_buffer(&gpu.decode_tables, 256 * 4, bytemuck::bytes_of(&m));
    }

    fn dispatch_decode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        matrix: &[i32],
    ) -> Result<GpuFrame> {
        self.upload_idct_tables(queue, matrix);
        let gpu = self.gpu.as_mut().ok_or_else(|| gpu_err("gpu session"))?;
        let t_up = std::time::Instant::now();
        write_bytes(queue, &gpu.y.coeff, &gpu.pack_y);
        write_bytes(queue, &gpu.u.coeff, &gpu.pack_u);
        write_bytes(queue, &gpu.v.coeff, &gpu.pack_v);
        let t_enc = std::time::Instant::now();

        let w = self.size.width as u32;
        let h = self.size.height as u32;
        let yuv = yuv_table(self.color_space, self.size.height);
        let fused = FusedParams {
            y_blocks_x: gpu.y.blocks_x.max(1),
            u_blocks_x: gpu.u.blocks_x.max(1),
            width: w,
            height: h,
            dst_stride: padded_bpr(w) / 4,
            yuv_y: yuv.yuv_y,
            yuv_r: yuv.yuv_r,
            yuv_gu: yuv.yuv_gu,
            yuv_gv: yuv.yuv_gv,
            yuv_b: yuv.yuv_b,
            _pad0: 0,
            _pad1: 0,
        };
        write_pod(queue, &gpu.fused_ubo, &fused);

        gpu.ring = (gpu.ring + 1) % RING;
        let tex = gpu.textures[gpu.ring].clone();
        let tiles = gpu.u.blocks_x.saturating_mul(gpu.u.blocks_y).max(1);

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vmx-decode"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("decode"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.fused_pipeline);
            pass.set_bind_group(0, &gpu.fused_bind, &[]);
            pass.dispatch_workgroups(tiles, 1, 1);
        }
        enc.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &gpu.bgra_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr(w)),
                    rows_per_image: Some(h),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let index = queue.submit(Some(enc.finish()));
        let t_wait = std::time::Instant::now();
        wait(device, &index);
        if std::env::var_os("VMX_GPU_TRACE").is_some() {
            eprintln!(
                "gpu_trace upload={:.3}ms encode={:.3}ms wait={:.3}ms",
                t_enc.duration_since(t_up).as_secs_f64() * 1e3,
                t_wait.duration_since(t_enc).as_secs_f64() * 1e3,
                t_wait.elapsed().as_secs_f64() * 1e3
            );
        }
        Ok(GpuFrame {
            texture: tex,
            width: w,
            height: h,
            submission_index: index,
        })
    }

    fn dispatch_preview(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        y: &[u32],
        u: &[u32],
        v: &[u32],
        y_stride: u32,
        u_stride: u32,
        v_stride: u32,
        w: u32,
        h: u32,
    ) -> Result<GpuFrame> {
        let gpu = self.gpu.as_mut().ok_or_else(|| gpu_err("gpu session"))?;
        write_bytes(queue, &gpu.y.plane, bytemuck::cast_slice(y));
        write_bytes(queue, &gpu.u.plane, bytemuck::cast_slice(u));
        write_bytes(queue, &gpu.v.plane, bytemuck::cast_slice(v));
        let mut cp = yuv_table(self.color_space, self.size.height);
        cp.width = w;
        cp.height = h;
        cp.y_stride = y_stride;
        cp.u_stride = u_stride;
        cp.v_stride = v_stride;
        cp.dst_stride = padded_bpr(w) / 4;
        write_pod(queue, &gpu.color_ubo, &cp);

        gpu.preview_ring = (gpu.preview_ring + 1) % RING;
        let tex = gpu.preview_textures[gpu.preview_ring].clone();

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vmx-preview"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("color"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.color_pipeline);
            pass.set_bind_group(0, &gpu.color_bind, &[]);
            pass.dispatch_workgroups((w / 2).div_ceil(8).max(1), h.div_ceil(8).max(1), 1);
        }
        enc.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &gpu.bgra_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr(w)),
                    rows_per_image: Some(h),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let index = queue.submit(Some(enc.finish()));
        wait(device, &index);
        Ok(GpuFrame {
            texture: tex,
            width: w,
            height: h,
            submission_index: index,
        })
    }

    fn dispatch_encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Result<()> {
        let w = self.size.width as u32;
        let h = self.size.height as u32;
        let src_bpr = padded_bpr(w);
        let y_stride = self.planes.stride[0] as u32;
        let u_stride = self.planes.stride[1] as u32;
        let v_stride = self.planes.stride[2] as u32;
        let aligned_h = align_up(self.size.height, 16) as u32;
        let y_bx = y_stride / 8;
        let y_by = aligned_h / 8;
        let u_bx = u_stride / 8;
        let u_by = aligned_h / 8;
        let rgb = if matches!(self.color_space, ColorSpace::Bt601)
            || (self.color_space == ColorSpace::Undefined && self.size.height < 720)
        {
            &RGB_YUV_601
        } else {
            &RGB_YUV_709
        };
        let y_len = y_stride * aligned_h;
        let u_len = u_stride * aligned_h;
        let v_len = v_stride * aligned_h;
        let gpu = self.gpu.as_ref().ok_or_else(|| gpu_err("gpu session"))?;
        let ep = EncodeParams {
            width: w,
            height: h,
            src_stride: src_bpr / 4,
            y_stride,
            u_stride,
            v_stride,
            y_blocks_x: y_bx,
            y_blocks_y: y_by,
            u_blocks_x: u_bx,
            u_blocks_y: u_by,
            add_y: -128,
            add_uv: 0,
            u_plane_off: y_len,
            v_plane_off: y_len + u_len,
            u_coeff_off: gpu.y_coeff_count,
            v_coeff_off: gpu.y_coeff_count + gpu.u_coeff_count,
            u_r: i32::from(rgb[1].r),
            u_g: i32::from(rgb[1].g),
            u_b: i32::from(rgb[1].b),
            v_r: i32::from(rgb[2].r),
            v_g: i32::from(rgb[2].g),
            v_b: i32::from(rgb[2].b),
            y_r: i32::from(rgb[0].r),
            y_g: i32::from(rgb[0].g),
            y_b: i32::from(rgb[0].b),
            a_plane_off: y_len + u_len + v_len,
            a_coeff_off: gpu.y_coeff_count + gpu.u_coeff_count * 2,
            src_rgba: u32::from(matches!(
                texture.format(),
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
            )),
        };
        write_pod(queue, &gpu.enc_ubo, &ep);

        let idx = self.decode_matrix_idx;
        let mut matrix = [0i32; 192];
        for (dst, &src) in matrix.iter_mut().zip(self.encode_presets[idx].iter()) {
            *dst = i32::from(src);
        }
        queue.write_buffer(&gpu.enc_tables, 192 * 4, bytemuck::bytes_of(&matrix));

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vmx-encode"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &gpu.src_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(src_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("enc-color"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.color_encode_pipeline);
            pass.set_bind_group(0, &gpu.enc_bind, &[]);
            pass.dispatch_workgroups((w / 2).div_ceil(8).max(1), h.div_ceil(8).max(1), 1);
        }
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdct"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.fdct_y_pipeline);
            pass.set_bind_group(0, &gpu.enc_bind, &[]);
            pass.dispatch_workgroups((y_bx * y_by).div_ceil(64).max(1), 1, 1);
            pass.set_pipeline(&gpu.fdct_u_pipeline);
            pass.dispatch_workgroups((u_bx * u_by).div_ceil(64).max(1), 1, 1);
            pass.set_pipeline(&gpu.fdct_v_pipeline);
            pass.dispatch_workgroups((u_bx * u_by).div_ceil(64).max(1), 1, 1);
            pass.set_pipeline(&gpu.fdct_a_pipeline);
            pass.dispatch_workgroups((y_bx * y_by).div_ceil(64).max(1), 1, 1);
        }
        let bytes = u64::from(gpu.y_coeff_count + gpu.u_coeff_count * 2 + gpu.a_coeff_count) * 2;
        enc.copy_buffer_to_buffer(&gpu.coeffs, 0, &gpu.coeff_read, 0, bytes.max(4));
        let index = queue.submit(Some(enc.finish()));
        wait(device, &index);

        let y_n = gpu.y_coeff_count as usize;
        let u_n = gpu.u_coeff_count as usize;
        let a_n = gpu.a_coeff_count as usize;
        let [y_blocks, u_blocks, v_blocks, a_blocks] =
            map_packed_planes(device, &gpu.coeff_read, y_n, u_n, a_n);
        encode_slices_from_coeffs(
            &mut self.slices,
            [
                self.planes.stride[0],
                self.planes.stride[1],
                self.planes.stride[2],
                self.planes.stride[3],
            ],
            &y_blocks,
            &u_blocks,
            &v_blocks,
            Some(&a_blocks),
            self.dc_shift,
        );
        Ok(())
    }
}

fn plane_bytes_to_u32(data: &[u8], stride: u32, height: u32) -> Vec<u32> {
    let mut out = vec![0u32; (stride * height) as usize];
    let n = out.len().min(data.len());
    for i in 0..n {
        out[i] = u32::from(data[i]);
    }
    out
}

fn map_packed_planes(
    device: &wgpu::Device,
    buf: &wgpu::Buffer,
    y_n: usize,
    u_n: usize,
    a_n: usize,
) -> [Vec<[i16; 64]>; 4] {
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait);
    let data = slice.get_mapped_range();
    let vals: &[i16] = bytemuck::cast_slice(&data);
    let unpack = |off: usize, count: usize| -> Vec<[i16; 64]> {
        let n = count / 64;
        let mut out = Vec::with_capacity(n);
        for b in 0..n {
            let mut block = [0i16; 64];
            let src = off + b * 64;
            block.copy_from_slice(&vals[src..src + 64]);
            out.push(block);
        }
        out
    };
    let y = unpack(0, y_n);
    let u = unpack(y_n, u_n);
    let v = unpack(y_n + u_n, u_n);
    let a = unpack(y_n + u_n * 2, a_n);
    drop(data);
    buf.unmap();
    [y, u, v, a]
}

/// Read a BGRA texture back to tightly packed CPU bytes (tests / bench).
pub fn read_texture_bgra(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let bpr = padded_bpr(width);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bpr * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback"),
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let index = queue.submit(Some(enc.finish()));
    wait(device, &index);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait);
    let data = slice.get_mapped_range();
    let mut out = vec![0u8; (width * height * 4) as usize];
    let row = (width * 4) as usize;
    for y in 0..height as usize {
        let src = y * bpr as usize;
        let dst = y * row;
        out[dst..dst + row].copy_from_slice(&data[src..src + row]);
    }
    drop(data);
    buf.unmap();
    Ok(out)
}

/// Upload tightly packed BGRA into a new texture on `device`.
pub fn upload_bgra_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    let tex = make_bgra_texture(device, width, height);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    tex
}
