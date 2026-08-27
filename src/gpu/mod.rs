mod pack;

use wgpu::util::DeviceExt;

use crate::codec::slice::{decode_slices_coeffs, encode_slices_from_coeffs};
use crate::color::convert::select_yuv_rgb;
use crate::error::{Result, VmxError};
use crate::gpu::pack::PlanePack;
use crate::instance::Codec;
use crate::tables::{
    FTAB1_128, FTAB2_128, FTAB3_128, FTAB4_128, IDCT_ROW_TABLES, RGB_YUV_601, RGB_YUV_709,
    ZIGZAG_INV,
};
use crate::types::ColorSpace;

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

fn make_buffer(
    device: &wgpu::Device,
    data: &[u8],
    usage: wgpu::BufferUsages,
    label: &str,
) -> wgpu::Buffer {
    let mut bytes = data.to_vec();
    if bytes.is_empty() {
        bytes.resize(4, 0);
    }
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &bytes,
        usage,
    })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DecodeParams {
    blocks_x: u32,
    blocks_y: u32,
    stride: u32,
    add_val: i32,
    block_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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

pub(crate) struct GpuSession {
    width: u32,
    height: u32,
    idct_layout: wgpu::BindGroupLayout,
    color_layout: wgpu::BindGroupLayout,
    encode_layout: wgpu::BindGroupLayout,
    idct_pipeline: wgpu::ComputePipeline,
    color_pipeline: wgpu::ComputePipeline,
    color_encode_pipeline: wgpu::ComputePipeline,
    fdct_y_pipeline: wgpu::ComputePipeline,
    fdct_u_pipeline: wgpu::ComputePipeline,
    fdct_v_pipeline: wgpu::ComputePipeline,
    fdct_a_pipeline: wgpu::ComputePipeline,
    idct_tables: Vec<i32>,
    encode_tables: Vec<i32>,
    textures: Vec<wgpu::Texture>,
    preview_textures: Vec<wgpu::Texture>,
    ring: usize,
    preview_ring: usize,
}

impl GpuSession {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        preview_w: u32,
        preview_h: u32,
    ) -> Self {
        let idct_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vmx-idct-bgl"),
            entries: &[
                ubo_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, true),
                storage_entry(4, false),
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

        let idct_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-idct"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/idct.wgsl").into()),
        });
        let color_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-color"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/color.wgsl").into()),
        });
        let encode_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-encode"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/encode.wgsl").into()),
        });

        let idct_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vmx-idct-pl"),
            bind_group_layouts: &[&idct_layout],
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

        let idct_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vmx-idct-pipe"),
            layout: Some(&idct_pl),
            module: &idct_mod,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let color_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vmx-color-pipe"),
            layout: Some(&color_pl),
            module: &color_mod,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let color_encode_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("vmx-enc-color"),
                layout: Some(&encode_pl),
                module: &encode_mod,
                entry_point: Some("color_main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let fdct_y_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vmx-fdct-y"),
            layout: Some(&encode_pl),
            module: &encode_mod,
            entry_point: Some("fdct_y"),
            compilation_options: Default::default(),
            cache: None,
        });
        let fdct_u_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vmx-fdct-u"),
            layout: Some(&encode_pl),
            module: &encode_mod,
            entry_point: Some("fdct_u"),
            compilation_options: Default::default(),
            cache: None,
        });
        let fdct_v_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vmx-fdct-v"),
            layout: Some(&encode_pl),
            module: &encode_mod,
            entry_point: Some("fdct_v"),
            compilation_options: Default::default(),
            cache: None,
        });
        let fdct_a_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vmx-fdct-a"),
            layout: Some(&encode_pl),
            module: &encode_mod,
            entry_point: Some("fdct_a"),
            compilation_options: Default::default(),
            cache: None,
        });

        let mut idct_tables = Vec::with_capacity(384);
        idct_tables.extend(ZIGZAG_INV.iter().map(|&v| i32::from(v)));
        for row in IDCT_ROW_TABLES {
            idct_tables.extend(row.iter().map(|&v| i32::from(v)));
        }
        // decode_matrix appended per dispatch (64 i32) — reserve space in comments only

        let mut encode_tables = Vec::with_capacity(384);
        for t in [&FTAB1_128, &FTAB2_128, &FTAB3_128, &FTAB4_128] {
            encode_tables.extend(t.iter().map(|&v| i32::from(v)));
        }
        encode_tables.extend(ZIGZAG_INV.iter().map(|&v| i32::from(v)));
        // encode_matrix appended per dispatch (192 u32 as i32)

        let textures = (0..RING)
            .map(|_| make_bgra_texture(device, width, height))
            .collect();
        let preview_textures = (0..RING)
            .map(|_| make_bgra_texture(device, preview_w.max(2), preview_h.max(2)))
            .collect();

        Self {
            width,
            height,
            idct_layout,
            color_layout,
            encode_layout,
            idct_pipeline,
            color_pipeline,
            color_encode_pipeline,
            fdct_y_pipeline,
            fdct_u_pipeline,
            fdct_v_pipeline,
            fdct_a_pipeline,
            idct_tables,
            encode_tables,
            textures,
            preview_textures,
            ring: 0,
            preview_ring: 0,
        }
    }
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
        let packs = decode_slices_coeffs(&mut self.slices, strides, self.dc_shift);
        let y_slices: Vec<_> = packs.iter().map(|p| p[0].clone()).collect();
        let u_slices: Vec<_> = packs.iter().map(|p| p[1].clone()).collect();
        let v_slices: Vec<_> = packs.iter().map(|p| p[2].clone()).collect();
        let y = PlanePack::from_slice_blocks(strides[0], 128, &y_slices);
        let u = PlanePack::from_slice_blocks(strides[1], 0, &u_slices);
        let v = PlanePack::from_slice_blocks(strides[2], 0, &v_slices);
        let idx = self.decode_matrix_idx;
        let matrix: Vec<i32> = self.decode_presets[idx]
            .iter()
            .map(|&m| i32::from(m as i16))
            .collect();
        self.dispatch_decode(device, queue, &y, &u, &v, &matrix, false)
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
        self.dispatch_color_only(
            device, queue, &y, &u, &v, y_stride, u_stride, v_stride, pw, ph, true,
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

    fn dispatch_decode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        y: &PlanePack,
        u: &PlanePack,
        v: &PlanePack,
        matrix: &[i32],
        preview: bool,
    ) -> Result<GpuFrame> {
        let gpu = self.gpu.as_mut().ok_or_else(|| gpu_err("gpu session"))?;
        let y_out = idct_plane(device, queue, gpu, y, matrix)?;
        let u_out = idct_plane(device, queue, gpu, u, matrix)?;
        let v_out = idct_plane(device, queue, gpu, v, matrix)?;
        let w = if preview {
            self.preview_size.width as u32
        } else {
            self.size.width as u32
        };
        let h = if preview {
            self.preview_size.height as u32
        } else {
            self.size.height as u32
        };
        self.dispatch_color_from_planes(
            device, queue, &y_out, &u_out, &v_out, y.stride, u.stride, v.stride, w, h, preview,
        )
    }

    fn dispatch_color_only(
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
        preview: bool,
    ) -> Result<GpuFrame> {
        let y_buf = make_buffer(
            device,
            bytemuck::cast_slice(y),
            wgpu::BufferUsages::STORAGE,
            "y",
        );
        let u_buf = make_buffer(
            device,
            bytemuck::cast_slice(u),
            wgpu::BufferUsages::STORAGE,
            "u",
        );
        let v_buf = make_buffer(
            device,
            bytemuck::cast_slice(v),
            wgpu::BufferUsages::STORAGE,
            "v",
        );
        self.dispatch_color_from_planes(
            device, queue, &y_buf, &u_buf, &v_buf, y_stride, u_stride, v_stride, w, h, preview,
        )
    }

    fn dispatch_color_from_planes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        y_buf: &wgpu::Buffer,
        u_buf: &wgpu::Buffer,
        v_buf: &wgpu::Buffer,
        y_stride: u32,
        u_stride: u32,
        v_stride: u32,
        w: u32,
        h: u32,
        preview: bool,
    ) -> Result<GpuFrame> {
        let gpu = self.gpu.as_mut().ok_or_else(|| gpu_err("gpu session"))?;
        let dst_stride = padded_bpr(w) / 4;
        let mut cp = yuv_table(self.color_space, self.size.height);
        cp.width = w;
        cp.height = h;
        cp.y_stride = y_stride;
        cp.u_stride = u_stride;
        cp.v_stride = v_stride;
        cp.dst_stride = dst_stride;
        let ubo = make_buffer(
            device,
            bytemuck::bytes_of(&cp),
            wgpu::BufferUsages::UNIFORM,
            "color-ubo",
        );
        let bgra_size = (dst_stride * h).max(1) * 4;
        let bgra_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bgra-out"),
            size: bgra_size as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("color-bg"),
            layout: &gpu.color_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ubo.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: y_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: u_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: v_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: bgra_buf.as_entire_binding(),
                },
            ],
        });

        let tex = if preview {
            gpu.preview_ring = (gpu.preview_ring + 1) % RING;
            gpu.preview_textures[gpu.preview_ring].clone()
        } else {
            gpu.ring = (gpu.ring + 1) % RING;
            gpu.textures[gpu.ring].clone()
        };

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vmx-color"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("color"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.color_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups((w / 2).div_ceil(8).max(1), h.div_ceil(8).max(1), 1);
        }
        enc.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &bgra_buf,
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
        let src_size = src_bpr * h;
        let src_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("enc-src"),
            size: src_size as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tex-download"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &src_buf,
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
        queue.submit(Some(enc.finish()));
        let _ = device.poll(wgpu::PollType::Wait);

        let y_stride = self.planes.stride[0] as u32;
        let u_stride = self.planes.stride[1] as u32;
        let v_stride = self.planes.stride[2] as u32;
        let aligned_h = crate::types::align_up(self.size.height, 16) as u32;
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
        let a_len = y_len;
        let u_plane_off = y_len;
        let v_plane_off = y_len + u_len;
        let a_plane_off = y_len + u_len + v_len;
        let y_coeff_count = y_bx * y_by * 64;
        let u_coeff_count = u_bx * u_by * 64;
        let a_coeff_count = y_coeff_count;
        let u_coeff_off = y_coeff_count;
        let v_coeff_off = y_coeff_count + u_coeff_count;
        let a_coeff_off = y_coeff_count + u_coeff_count * 2;
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
            u_plane_off,
            v_plane_off,
            u_coeff_off,
            v_coeff_off,
            u_r: i32::from(rgb[1].r),
            u_g: i32::from(rgb[1].g),
            u_b: i32::from(rgb[1].b),
            v_r: i32::from(rgb[2].r),
            v_g: i32::from(rgb[2].g),
            v_b: i32::from(rgb[2].b),
            y_r: i32::from(rgb[0].r),
            y_g: i32::from(rgb[0].g),
            y_b: i32::from(rgb[0].b),
            a_plane_off,
            a_coeff_off,
            src_rgba: u32::from(matches!(
                texture.format(),
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
            )),
        };
        let gpu = self.gpu.as_ref().ok_or_else(|| gpu_err("gpu session"))?;
        let ubo = make_buffer(
            device,
            bytemuck::bytes_of(&ep),
            wgpu::BufferUsages::UNIFORM,
            "enc-ubo",
        );
        let idx = self.decode_matrix_idx;
        let mut tables = gpu.encode_tables.clone();
        tables.extend(self.encode_presets[idx].iter().map(|&v| i32::from(v)));
        let tables_buf = make_buffer(
            device,
            bytemuck::cast_slice(&tables),
            wgpu::BufferUsages::STORAGE,
            "enc-tables",
        );
        let yuv = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv"),
            size: ((y_len + u_len + v_len + a_len) * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let coeff_total = y_coeff_count + u_coeff_count * 2 + a_coeff_count;
        let coeffs = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("coeffs"),
            size: (coeff_total * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("enc-bg"),
            layout: &gpu.encode_layout,
            entries: &[
                bind(0, &ubo),
                bind(1, &src_buf),
                bind(2, &tables_buf),
                bind(3, &yuv),
                bind(4, &coeffs),
            ],
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encode"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("enc-color"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.color_encode_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups((w / 2).div_ceil(8).max(1), h.div_ceil(8).max(1), 1);
        }
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdct"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.fdct_y_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups((y_bx * y_by).div_ceil(64).max(1), 1, 1);
            pass.set_pipeline(&gpu.fdct_u_pipeline);
            pass.dispatch_workgroups((u_bx * u_by).div_ceil(64).max(1), 1, 1);
            pass.set_pipeline(&gpu.fdct_v_pipeline);
            pass.dispatch_workgroups((u_bx * u_by).div_ceil(64).max(1), 1, 1);
            pass.set_pipeline(&gpu.fdct_a_pipeline);
            pass.dispatch_workgroups((y_bx * y_by).div_ceil(64).max(1), 1, 1);
        }
        let y_read = staging(device, (y_coeff_count * 4) as u64);
        let u_read = staging(device, (u_coeff_count * 4) as u64);
        let v_read = staging(device, (u_coeff_count * 4) as u64);
        let a_read = staging(device, (a_coeff_count * 4) as u64);
        enc.copy_buffer_to_buffer(&coeffs, 0, &y_read, 0, (y_coeff_count * 4) as u64);
        enc.copy_buffer_to_buffer(
            &coeffs,
            (u_coeff_off * 4) as u64,
            &u_read,
            0,
            (u_coeff_count * 4) as u64,
        );
        enc.copy_buffer_to_buffer(
            &coeffs,
            (v_coeff_off * 4) as u64,
            &v_read,
            0,
            (u_coeff_count * 4) as u64,
        );
        enc.copy_buffer_to_buffer(
            &coeffs,
            (a_coeff_off * 4) as u64,
            &a_read,
            0,
            (a_coeff_count * 4) as u64,
        );
        let index = queue.submit(Some(enc.finish()));
        wait(device, &index);

        let y_blocks = map_i16_blocks(device, &y_read, y_coeff_count as usize);
        let u_blocks = map_i16_blocks(device, &u_read, u_coeff_count as usize);
        let v_blocks = map_i16_blocks(device, &v_read, u_coeff_count as usize);
        let a_blocks = map_i16_blocks(device, &a_read, a_coeff_count as usize);
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

fn bind<'a>(binding: u32, buf: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

fn staging(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size: size.max(4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn map_i16_blocks(device: &wgpu::Device, buf: &wgpu::Buffer, i32_count: usize) -> Vec<[i16; 64]> {
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait);
    let data = slice.get_mapped_range();
    let vals: &[i32] = bytemuck::cast_slice(&data);
    let n = i32_count / 64;
    let mut out = Vec::with_capacity(n);
    for b in 0..n {
        let mut block = [0i16; 64];
        for i in 0..64 {
            block[i] = vals[b * 64 + i] as i16;
        }
        out.push(block);
    }
    drop(data);
    buf.unmap();
    out
}

fn plane_bytes_to_u32(data: &[u8], stride: u32, height: u32) -> Vec<u32> {
    let mut out = vec![0u32; (stride * height) as usize];
    let n = out.len().min(data.len());
    for i in 0..n {
        out[i] = u32::from(data[i]);
    }
    out
}

fn idct_plane(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    gpu: &GpuSession,
    pack: &PlanePack,
    matrix: &[i32],
) -> Result<wgpu::Buffer> {
    let params = DecodeParams {
        blocks_x: pack.blocks_x.max(1),
        blocks_y: pack.blocks_y.max(1),
        stride: pack.stride,
        add_val: pack.add_val,
        block_count: pack.block_count().max(1),
        _pad0: 0,
        _pad1: 0,
        _pad2: 0,
    };
    let ubo = make_buffer(
        device,
        bytemuck::bytes_of(&params),
        wgpu::BufferUsages::UNIFORM,
        "idct-ubo",
    );
    let mut tables = gpu.idct_tables.clone();
    tables.extend(matrix.iter().copied());
    let tables_buf = make_buffer(
        device,
        bytemuck::cast_slice(&tables),
        wgpu::BufferUsages::STORAGE,
        "idct-tables",
    );
    let n = pack.dc.len().max(1);
    let mut headers = vec![[0u32; 4]; n];
    for (i, header) in headers.iter_mut().enumerate().take(pack.dc.len()) {
        *header = [
            pack.dc[i] as u32,
            pack.valid.get(i).copied().unwrap_or(0),
            pack.nnz.get(i).copied().unwrap_or(0),
            pack.ac_off.get(i).copied().unwrap_or(0),
        ];
    }
    let headers_buf = make_buffer(
        device,
        bytemuck::cast_slice(&headers),
        wgpu::BufferUsages::STORAGE,
        "headers",
    );
    let mut packed = Vec::with_capacity(pack.ac_idx.len().max(1) * 2);
    for i in 0..pack.ac_idx.len() {
        packed.push(pack.ac_idx[i] as i32);
        packed.push(*pack.ac_val.get(i).unwrap_or(&0));
    }
    if packed.is_empty() {
        packed.extend_from_slice(&[0, 0]);
    }
    let ac_buf = make_buffer(
        device,
        bytemuck::cast_slice(&packed),
        wgpu::BufferUsages::STORAGE,
        "packed-ac",
    );
    let plane_len = (pack.stride * pack.blocks_y.max(1) * 8).max(1);
    let plane_out = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("plane-out"),
        size: (plane_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("idct-bg"),
        layout: &gpu.idct_layout,
        entries: &[
            bind(0, &ubo),
            bind(1, &tables_buf),
            bind(2, &headers_buf),
            bind(3, &ac_buf),
            bind(4, &plane_out),
        ],
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("idct"),
    });
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("idct"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&gpu.idct_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(params.block_count.div_ceil(64).max(1), 1, 1);
    }
    let index = queue.submit(Some(enc.finish()));
    wait(device, &index);
    Ok(plane_out)
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
