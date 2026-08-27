pub(crate) mod pack;

use crate::codec::slice::{decode_slices_coeffs, encode_slices_from_coeffs};
use crate::color::convert::select_yuv_rgb;
use crate::error::{Result, VmxError};
use crate::instance::Codec;
use crate::tables::{
    FTAB1_128, FTAB2_128, FTAB3_128, FTAB4_128, RGB_YUV_601, RGB_YUV_709, ZIGZAG_INV,
};
use crate::types::{ColorSpace, align_up};

const RING: usize = 3;
const COPY_ALIGN: u32 = 256;

/// Decoded (or preview) frame on the caller's device.
#[derive(Debug, Clone)]
pub struct GpuFrame {
    /// `Bgra8Unorm` texture (`TEXTURE_BINDING | COPY_DST | COPY_SRC`, plus
    /// `STORAGE_BINDING` when the adapter exposes `BGRA8UNORM_STORAGE`).
    pub texture: wgpu::Texture,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Submit index for this decode. Same-queue work after this submit can
    /// sample [`Self::texture`]; CPU readback still waits via [`read_texture_bgra`].
    pub submission_index: wgpu::SubmissionIndex,
}

/// Try to obtain a headless wgpu device (WARP / llvmpipe / discrete). `None` if none.
pub fn request_headless_device()
-> Option<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    fn try_instance(
        backends: wgpu::Backends,
        force_fallback: bool,
        power: wgpu::PowerPreference,
    ) -> Option<(wgpu::Instance, wgpu::Adapter)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: power,
            compatible_surface: None,
            force_fallback_adapter: force_fallback,
            ..Default::default()
        }))
        .ok()?;
        Some((instance, adapter))
    }

    // Callers should pass their own device. Headless tests prefer the native
    // high-performance backend (DX12 on Windows) rather than OpenGL.
    let primary = if cfg!(target_os = "windows") {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::PRIMARY | wgpu::Backends::GL
    };
    let (instance, adapter) = try_instance(primary, false, wgpu::PowerPreference::HighPerformance)
        .or_else(|| {
            try_instance(
                wgpu::Backends::PRIMARY | wgpu::Backends::GL,
                false,
                wgpu::PowerPreference::HighPerformance,
            )
        })
        .or_else(|| {
            try_instance(
                wgpu::Backends::PRIMARY | wgpu::Backends::GL,
                true,
                wgpu::PowerPreference::LowPower,
            )
        })?;
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
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
        ..Default::default()
    }))
    .ok()?;
    Some((instance, adapter, device, queue))
}

pub(crate) fn wait(device: &wgpu::Device, index: &wgpu::SubmissionIndex) {
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: Some(index.clone()),
        timeout: None,
    });
}

#[inline(always)]
fn padded_bpr(width: u32) -> u32 {
    let raw = width.saturating_mul(4);
    raw.div_ceil(COPY_ALIGN) * COPY_ALIGN
}

#[inline(always)]
fn y_stride(width: u32) -> u32 {
    align_up(width as i32, 8) as u32
}

#[inline(always)]
fn uv_stride(width: u32) -> u32 {
    align_up((width / 2) as i32, 8) as u32
}

#[inline(always)]
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
    u_word_off: u32,
    v_word_off: u32,
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

#[allow(dead_code)]
pub(crate) struct GpuSession {
    width: u32,
    height: u32,
    storage_out: bool,
    y_blocks_x: u32,
    u_blocks_x: u32,
    u_blocks_y: u32,
    u_word_off: u32,
    v_word_off: u32,
    coeff_in: wgpu::Buffer,
    matrix_buf: wgpu::Buffer,
    last_matrix: [i32; 64],
    has_matrix: bool,
    fused_ubo: wgpu::Buffer,
    fused_pipeline: wgpu::ComputePipeline,
    fused_binds: Vec<wgpu::BindGroup>,
    bgra_buf: Option<wgpu::Buffer>,
    pack: Vec<u8>,
    textures: Vec<wgpu::Texture>,
    preview_textures: Vec<wgpu::Texture>,
    preview_cpu: Vec<u8>,
    ring: usize,
    preview_ring: usize,
    encode_layout: wgpu::BindGroupLayout,
    fdct_y_pipeline: wgpu::ComputePipeline,
    fdct_u_pipeline: wgpu::ComputePipeline,
    fdct_v_pipeline: wgpu::ComputePipeline,
    fdct_a_pipeline: wgpu::ComputePipeline,
    enc_ubo: wgpu::Buffer,
    enc_tables: wgpu::Buffer,
    coeffs: wgpu::Buffer,
    coeff_read: wgpu::Buffer,
    last_enc_matrix_idx: Option<usize>,
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

fn storage_tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::Bgra8Unorm,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn sampled_tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
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

fn compile_opts() -> wgpu::PipelineCompilationOptions<'static> {
    wgpu::PipelineCompilationOptions {
        zero_initialize_workgroup_memory: false,
        ..Default::default()
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
        compilation_options: compile_opts(),
        cache: None,
    })
}

impl GpuSession {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        preview_w: u32,
        preview_h: u32,
    ) -> Self {
        let storage_out = device
            .features()
            .contains(wgpu::Features::BGRA8UNORM_STORAGE);

        let decode_src: std::borrow::Cow<str> = if storage_out {
            concat!(
                include_str!("shaders/decode.wgsl"),
                include_str!("shaders/decode_tex.wgsl")
            )
            .into()
        } else {
            concat!(
                include_str!("shaders/decode.wgsl"),
                include_str!("shaders/decode_buf.wgsl")
            )
            .into()
        };
        let fused_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-decode"),
            source: wgpu::ShaderSource::Wgsl(decode_src),
        });
        let encode_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vmx-encode"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/encode.wgsl").into()),
        });

        let (fused_layout, fused_entry) = if storage_out {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vmx-fused-tex-bgl"),
                entries: &[
                    ubo_entry(0),
                    storage_entry(1, true),
                    storage_entry(2, true),
                    storage_tex_entry(3),
                ],
            });
            (layout, "main_tex")
        } else {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vmx-fused-buf-bgl"),
                entries: &[
                    ubo_entry(0),
                    storage_entry(1, true),
                    storage_entry(2, true),
                    storage_entry(4, false),
                ],
            });
            (layout, "main_buf")
        };
        let encode_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vmx-encode-bgl"),
            entries: &[
                ubo_entry(0),
                sampled_tex_entry(1),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        });

        let fused_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vmx-fused-pl"),
            bind_group_layouts: &[Some(&fused_layout)],
            immediate_size: 0,
        });
        let encode_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vmx-encode-pl"),
            bind_group_layouts: &[Some(&encode_layout)],
            immediate_size: 0,
        });

        let fused_pipeline =
            compute_pipeline(device, &fused_pl, &fused_mod, fused_entry, "vmx-fused-pipe");
        let fdct_y_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_y", "vmx-fdct-y");
        let fdct_u_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_u", "vmx-fdct-u");
        let fdct_v_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_v", "vmx-fdct-v");
        let fdct_a_pipeline =
            compute_pipeline(device, &encode_pl, &encode_mod, "fdct_a", "vmx-fdct-a");

        let ys = y_stride(width);
        let us = uv_stride(width);
        let ah = aligned_height(height);
        let y_bx = (ys / 8).max(1);
        let y_by = (ah / 8).max(1);
        let u_bx = (us / 8).max(1);
        let u_by = (ah / 8).max(1);
        let y_pack = y_bx * y_by * pack::PACK_BYTES as u32;
        let u_pack = u_bx * u_by * pack::PACK_BYTES as u32;
        let v_pack = u_pack;
        let u_word_off = y_pack / 4;
        let v_word_off = (y_pack + u_pack) / 4;

        let coeff_in = buf(
            device,
            u64::from(y_pack + u_pack + v_pack),
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            "decode-coeffs",
        );
        let matrix_buf = buf(
            device,
            64 * 4,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            "decode-matrix",
        );
        let fused_ubo = buf(
            device,
            48,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "fused-ubo",
        );

        let dst_stride = padded_bpr(width) / 4;
        let bgra_buf = if storage_out {
            None
        } else {
            Some(buf(
                device,
                u64::from(dst_stride) * u64::from(height.max(preview_h)) * 4,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "bgra-out",
            ))
        };

        let textures = (0..RING)
            .map(|_| make_bgra_texture(device, width, height, storage_out))
            .collect::<Vec<_>>();
        let preview_textures = (0..RING)
            .map(|_| make_bgra_texture(device, preview_w.max(2), preview_h.max(2), false))
            .collect::<Vec<_>>();

        let fused_binds = if storage_out {
            textures
                .iter()
                .map(|tex| {
                    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("fused-tex-bg"),
                        layout: &fused_layout,
                        entries: &[
                            bind(0, &fused_ubo),
                            bind(1, &matrix_buf),
                            bind(2, &coeff_in),
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                        ],
                    })
                })
                .collect()
        } else {
            let bgra = bgra_buf.as_ref().expect("buf path");
            vec![device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fused-buf-bg"),
                layout: &fused_layout,
                entries: &[
                    bind(0, &fused_ubo),
                    bind(1, &matrix_buf),
                    bind(2, &coeff_in),
                    bind(4, bgra),
                ],
            })]
        };

        let y_coeff_count = y_bx * y_by * 64;
        let u_coeff_count = u_bx * u_by * 64;
        let a_coeff_count = y_coeff_count;
        let coeff_i16 = y_coeff_count + u_coeff_count * 2 + a_coeff_count;

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
            let mut data = enc_tables
                .slice(..)
                .get_mapped_range_mut()
                .expect("enc-tables map");
            let mut tables = Vec::with_capacity(384);
            for t in [&FTAB1_128, &FTAB2_128, &FTAB3_128, &FTAB4_128] {
                tables.extend(t.iter().map(|&v| i32::from(v)));
            }
            tables.extend(ZIGZAG_INV.iter().map(|&v| i32::from(v)));
            tables.resize(384, 0);
            data.copy_from_slice(bytemuck::cast_slice(&tables));
        }
        enc_tables.unmap();
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

        Self {
            width,
            height,
            storage_out,
            y_blocks_x: y_bx,
            u_blocks_x: u_bx,
            u_blocks_y: u_by,
            u_word_off,
            v_word_off,
            coeff_in,
            matrix_buf,
            last_matrix: [0; 64],
            has_matrix: false,
            fused_ubo,
            fused_pipeline,
            fused_binds,
            bgra_buf,
            pack: Vec::new(),
            textures,
            preview_textures,
            preview_cpu: Vec::new(),
            ring: 0,
            preview_ring: 0,
            encode_layout,
            fdct_y_pipeline,
            fdct_u_pipeline,
            fdct_v_pipeline,
            fdct_a_pipeline,
            enc_ubo,
            enc_tables,
            coeffs,
            coeff_read,
            last_enc_matrix_idx: None,
            y_coeff_count,
            u_coeff_count,
            a_coeff_count,
        }
    }
}

fn make_bgra_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    storage: bool,
) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC;
    if storage {
        usage |= wgpu::TextureUsages::STORAGE_BINDING;
    }
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
        usage,
        view_formats: &[],
    })
}

fn gpu_err(msg: impl Into<String>) -> VmxError {
    VmxError::Gpu(msg.into())
}

fn yuv_coeffs(cs: ColorSpace, height: i32) -> (i32, i32, i32, i32, i32) {
    let t = select_yuv_rgb(cs, height);
    (
        i32::from(t[0]),
        i32::from(t[1]),
        i32::from(t[2]),
        i32::from(t[3]),
        i32::from(t[4]),
    )
}

fn can_sample_encode(texture: &wgpu::Texture) -> bool {
    texture
        .usage()
        .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        && matches!(
            texture.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
        )
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
    ///
    /// Returns after `queue.submit`. Later submits on the same queue can sample
    /// the texture; CPU readback still waits in [`read_texture_bgra`].
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
        decode_slices_coeffs(&mut self.slices, strides, self.dc_shift, &mut gpu.pack);
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
    ///
    /// Preview is 1/8 resolution, so compute dispatch cannot beat SIMD color
    /// convert. This path is CPU `decode_preview_bgra` plus `queue.write_texture`
    /// into the ring texture (same work as the bench's "CPU equivalent of GPU
    /// preview"). The upload is submitted; later work on the same queue can
    /// sample the texture without a CPU wait.
    pub fn decode_preview_to_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<GpuFrame> {
        self.ensure_gpu(device);
        let pw = self.preview_size.width as u32;
        let ph = self.preview_size.height as u32;
        let stride = pw as usize * 4;
        let need = stride * ph as usize;
        let mut gpu = self.gpu.take().expect("gpu");
        gpu.preview_cpu.resize(need, 0);
        let mut preview_cpu = std::mem::take(&mut gpu.preview_cpu);
        self.gpu = Some(gpu);
        self.decode_preview_bgra(&mut preview_cpu, stride)?;

        let mut gpu = self.gpu.take().expect("gpu");
        gpu.preview_ring = (gpu.preview_ring + 1) % RING;
        let tex = gpu.preview_textures[gpu.preview_ring].clone();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &preview_cpu,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pw * 4),
                rows_per_image: Some(ph),
            },
            wgpu::Extent3d {
                width: pw,
                height: ph,
                depth_or_array_layers: 1,
            },
        );
        gpu.preview_cpu = preview_cpu;
        self.gpu = Some(gpu);
        let index = queue.submit([]);
        Ok(GpuFrame {
            texture: tex,
            width: pw,
            height: ph,
            submission_index: index,
        })
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
        if !can_sample_encode(texture) {
            if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
                return Err(gpu_err("texture must have TEXTURE_BINDING or COPY_SRC"));
            }
            let w = desc.width;
            let h = desc.height;
            let pixels = read_texture_bgra(device, queue, texture, w, h)?;
            self.image_format = crate::types::ImageFormat::Bgra;
            return self.encode_bgra(&pixels, w as usize * 4);
        }
        self.ensure_gpu(device);
        self.image_format = crate::types::ImageFormat::Bgra;
        self.dispatch_encode(device, queue, texture)
    }

    fn dispatch_decode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        matrix: &[i32],
    ) -> Result<GpuFrame> {
        let gpu = self.gpu.as_mut().ok_or_else(|| gpu_err("gpu session"))?;
        let t_up = std::time::Instant::now();

        let mut m = [0i32; 64];
        for (dst, src) in m.iter_mut().zip(matrix.iter()) {
            *dst = *src;
        }
        if !gpu.has_matrix || gpu.last_matrix != m {
            queue.write_buffer(&gpu.matrix_buf, 0, bytemuck::bytes_of(&m));
            gpu.last_matrix = m;
            gpu.has_matrix = true;
        }

        write_bytes(queue, &gpu.coeff_in, &gpu.pack);
        let t_enc = std::time::Instant::now();

        let w = self.size.width as u32;
        let h = self.size.height as u32;
        let (yuv_y, yuv_r, yuv_gu, yuv_gv, yuv_b) = yuv_coeffs(self.color_space, self.size.height);
        let fused = FusedParams {
            y_blocks_x: gpu.y_blocks_x,
            u_blocks_x: gpu.u_blocks_x,
            width: w,
            height: h,
            dst_stride: padded_bpr(w) / 4,
            yuv_y,
            yuv_r,
            yuv_gu,
            yuv_gv,
            yuv_b,
            u_word_off: gpu.u_word_off,
            v_word_off: gpu.v_word_off,
        };
        write_pod(queue, &gpu.fused_ubo, &fused);

        gpu.ring = (gpu.ring + 1) % RING;
        let tex = gpu.textures[gpu.ring].clone();
        let bind_idx = if gpu.storage_out { gpu.ring } else { 0 };
        let tiles = gpu.u_blocks_x.saturating_mul(gpu.u_blocks_y).max(1);

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vmx-decode"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("decode"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.fused_pipeline);
            pass.set_bind_group(0, &gpu.fused_binds[bind_idx], &[]);
            pass.dispatch_workgroups(tiles, 1, 1);
        }
        if let Some(bgra) = gpu.bgra_buf.as_ref() {
            enc.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: bgra,
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
        }
        let index = queue.submit(Some(enc.finish()));
        if std::env::var_os("VMX_GPU_TRACE").is_some() {
            eprintln!(
                "gpu_trace upload={:.3}ms encode={:.3}ms storage={}",
                t_enc.duration_since(t_up).as_secs_f64() * 1e3,
                t_enc.elapsed().as_secs_f64() * 1e3,
                gpu.storage_out
            );
        }
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
        let gpu = self.gpu.as_mut().ok_or_else(|| gpu_err("gpu session"))?;
        let ep = EncodeParams {
            width: w,
            height: h,
            src_stride: padded_bpr(w) / 4,
            y_stride,
            u_stride,
            v_stride,
            y_blocks_x: y_bx,
            y_blocks_y: y_by,
            u_blocks_x: u_bx,
            u_blocks_y: u_by,
            add_y: -128,
            add_uv: 0,
            u_plane_off: 0,
            v_plane_off: 0,
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
            a_plane_off: 0,
            a_coeff_off: gpu.y_coeff_count + gpu.u_coeff_count * 2,
            src_rgba: 0,
        };
        write_pod(queue, &gpu.enc_ubo, &ep);

        let idx = self.decode_matrix_idx;
        if gpu.last_enc_matrix_idx != Some(idx) {
            let mut matrix = [0i32; 192];
            for (dst, &src) in matrix.iter_mut().zip(self.encode_presets[idx].iter()) {
                *dst = i32::from(src);
            }
            queue.write_buffer(&gpu.enc_tables, 192 * 4, bytemuck::bytes_of(&matrix));
            gpu.last_enc_matrix_idx = Some(idx);
        }

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let enc_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("enc-bg"),
            layout: &gpu.encode_layout,
            entries: &[
                bind(0, &gpu.enc_ubo),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                bind(2, &gpu.enc_tables),
                bind(3, &gpu.coeffs),
            ],
        });

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vmx-encode"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdct"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &enc_bind, &[]);
            pass.set_pipeline(&gpu.fdct_y_pipeline);
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
        queue.submit(Some(enc.finish()));

        let y_n = gpu.y_coeff_count as usize;
        let u_n = gpu.u_coeff_count as usize;
        let a_n = gpu.a_coeff_count as usize;
        encode_from_mapped(
            device,
            &gpu.coeff_read,
            &mut self.slices,
            [
                self.planes.stride[0],
                self.planes.stride[1],
                self.planes.stride[2],
                self.planes.stride[3],
            ],
            y_n,
            u_n,
            a_n,
            self.dc_shift,
        );
        Ok(())
    }
}

fn encode_from_mapped(
    device: &wgpu::Device,
    buf: &wgpu::Buffer,
    slices: &mut [crate::codec::slice::SliceSet],
    strides: [usize; 4],
    y_n: usize,
    u_n: usize,
    a_n: usize,
    dc_shift: i32,
) {
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    {
        let data = slice.get_mapped_range().expect("coeff map");
        let vals: &[i16] = bytemuck::cast_slice(&data);
        encode_slices_from_coeffs(slices, strides, vals, y_n, u_n, a_n, dc_shift);
    }
    buf.unmap();
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
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("readback map");
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
    let tex = make_bgra_texture(device, width, height, false);
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
