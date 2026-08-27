//! Slice-parallel encode/decode.

use crate::bitstream::SliceData;
use crate::codec::plane::PlaneView;
use crate::simd::dispatch::SimdPath;
use crate::simd::{decode_plane, encode_plane};
use crate::thread_pool::ThreadPool;
use crate::types::SLICE_HEIGHT;

pub struct SliceSet {
    pub dc: SliceData,
    pub ac: SliceData,
    pub offset: [usize; 4],
    pub offset16: [usize; 4],
    pub pixel_height: i32,
    pub pixel_height_interlaced: i32,
    pub lower_field: bool,
    pub temp_block: [i16; 64],
}

impl SliceSet {
    pub fn new(dc_cap: usize, ac_cap: usize) -> Self {
        Self {
            dc: SliceData::new(dc_cap),
            ac: SliceData::new(ac_cap),
            offset: [0; 4],
            offset16: [0; 4],
            pixel_height: SLICE_HEIGHT,
            pixel_height_interlaced: SLICE_HEIGHT,
            lower_field: false,
            temp_block: [0; 64],
        }
    }

    pub fn reset(&mut self) {
        self.dc.reset();
        self.ac.reset();
    }
}

pub struct PlaneBuffers {
    pub data: [Vec<u8>; 4],
    pub stride: [usize; 4],
    #[allow(dead_code)]
    pub width: [i32; 4],
    #[allow(dead_code)]
    pub height: [i32; 4],
}

fn encode_slice_range(
    path: SimdPath,
    planes: &PlaneBuffers,
    slices: &mut [SliceSet],
    encode_matrix: &[u16],
    dc_shift: i32,
    plane_count: usize,
) {
    let plane_count = plane_count.clamp(1, 4);
    for slice in slices.iter_mut() {
        slice.reset();
        for pi in 0..plane_count {
            // SAFETY: encode only reads plane bytes. PlaneView uses &mut for API symmetry
            // with decode. Slice offsets address disjoint row bands.
            let data = unsafe {
                std::slice::from_raw_parts_mut(
                    planes.data[pi].as_ptr() as *mut u8,
                    planes.data[pi].len(),
                )
            };
            encode_plane(
                path,
                &PlaneView {
                    index: pi,
                    data,
                    stride: planes.stride[pi],
                    offset: slice.offset[pi],
                },
                &mut slice.dc,
                &mut slice.ac,
                encode_matrix,
                dc_shift,
                &mut slice.temp_block,
            );
        }
    }
}

pub fn encode_slices(
    path: SimdPath,
    planes: &PlaneBuffers,
    slices: &mut [SliceSet],
    encode_matrix: &[u16],
    dc_shift: i32,
    plane_count: usize,
    pool: Option<&ThreadPool>,
) {
    let plane_count = plane_count.clamp(1, 4);
    match pool {
        Some(pool) if pool.thread_count() > 1 && slices.len() > 1 => {
            pool.parallel_chunks_mut(slices, |chunk| {
                encode_slice_range(path, planes, chunk, encode_matrix, dc_shift, plane_count);
            });
        }
        _ => encode_slice_range(path, planes, slices, encode_matrix, dc_shift, plane_count),
    }
}

fn prepare_slice_bitstream(slice: &mut SliceSet) {
    slice.dc.pos = 0;
    slice.dc.bits_left = crate::types::BITS_SIZE;
    slice.dc.temp = 0;
    slice.dc.temp_read = {
        let mut buf = [0u8; 8];
        let n = 8.min(slice.dc.stream.len());
        buf[..n].copy_from_slice(&slice.dc.stream[..n]);
        u64::from_be_bytes(buf)
    };
    slice.ac.pos = 0;
    slice.ac.bits_left = crate::types::BITS_SIZE;
    slice.ac.temp = 0;
    slice.ac.temp_read = {
        let mut buf = [0u8; 8];
        let n = 8.min(slice.ac.stream.len());
        buf[..n].copy_from_slice(&slice.ac.stream[..n]);
        u64::from_be_bytes(buf)
    };
}

fn decode_slice_range(
    path: SimdPath,
    plane_ptrs: [usize; 3],
    plane_lens: [usize; 3],
    strides: [usize; 3],
    slices: &mut [SliceSet],
    decode_matrix: &[u16],
    dc_shift: i32,
) {
    for slice in slices.iter_mut() {
        prepare_slice_bitstream(slice);
        for pi in 0..3 {
            // SAFETY: each slice writes a disjoint row band; callers split slices across threads.
            let data = unsafe {
                std::slice::from_raw_parts_mut(plane_ptrs[pi] as *mut u8, plane_lens[pi])
            };
            let mut view = PlaneView {
                index: pi,
                data,
                stride: strides[pi],
                offset: slice.offset[pi],
            };
            decode_plane(
                path,
                &mut view,
                &mut slice.dc,
                &mut slice.ac,
                decode_matrix,
                dc_shift,
                &mut slice.temp_block,
            );
        }
    }
}

pub fn decode_slices(
    path: SimdPath,
    planes: &mut PlaneBuffers,
    slices: &mut [SliceSet],
    decode_matrix: &[u16],
    dc_shift: i32,
    pool: Option<&ThreadPool>,
) {
    let plane_ptrs = [
        planes.data[0].as_mut_ptr() as usize,
        planes.data[1].as_mut_ptr() as usize,
        planes.data[2].as_mut_ptr() as usize,
    ];
    let plane_lens = [
        planes.data[0].len(),
        planes.data[1].len(),
        planes.data[2].len(),
    ];
    let strides = [planes.stride[0], planes.stride[1], planes.stride[2]];

    match pool {
        Some(pool) if pool.thread_count() > 1 && slices.len() > 1 => {
            pool.parallel_chunks_mut(slices, |chunk| {
                decode_slice_range(
                    path,
                    plane_ptrs,
                    plane_lens,
                    strides,
                    chunk,
                    decode_matrix,
                    dc_shift,
                );
            });
        }
        _ => decode_slice_range(
            path,
            plane_ptrs,
            plane_lens,
            strides,
            slices,
            decode_matrix,
            dc_shift,
        ),
    }
}

/// Decode each slice then immediately pack that band to BGRA (cache-friendly).
pub fn decode_slices_fused_bgra(
    path: SimdPath,
    color_path: crate::color::simd::ColorSimdPath,
    planes: &mut PlaneBuffers,
    slices: &mut [SliceSet],
    decode_matrix: &[u16],
    dc_shift: i32,
    pool: Option<&ThreadPool>,
    dst: &mut [u8],
    dst_stride: usize,
    width: i32,
    table: &[i16; 5],
) {
    let plane_ptrs = [
        planes.data[0].as_mut_ptr() as usize,
        planes.data[1].as_mut_ptr() as usize,
        planes.data[2].as_mut_ptr() as usize,
    ];
    let plane_lens = [
        planes.data[0].len(),
        planes.data[1].len(),
        planes.data[2].len(),
    ];
    let strides = [planes.stride[0], planes.stride[1], planes.stride[2]];
    let dst_ptr = dst.as_mut_ptr() as usize;
    let dst_len = dst.len();
    let y_stride = planes.stride[0];

    let pack_chunk = |chunk: &mut [SliceSet]| {
        for slice in chunk.iter_mut() {
            prepare_slice_bitstream(slice);
            for pi in 0..3 {
                // SAFETY: disjoint slice bands across threads; single-threaded within chunk.
                let data = unsafe {
                    std::slice::from_raw_parts_mut(plane_ptrs[pi] as *mut u8, plane_lens[pi])
                };
                let mut view = PlaneView {
                    index: pi,
                    data,
                    stride: strides[pi],
                    offset: slice.offset[pi],
                };
                decode_plane(
                    path,
                    &mut view,
                    &mut slice.dc,
                    &mut slice.ac,
                    decode_matrix,
                    dc_shift,
                    &mut slice.temp_block,
                );
            }
            let y_row0 = slice.offset[0] / y_stride;
            let rows = slice.pixel_height.max(0) as usize;
            // SAFETY: dst covers full frame; each slice writes its own row band.
            let dst_band = unsafe { std::slice::from_raw_parts_mut(dst_ptr as *mut u8, dst_len) };
            let y =
                unsafe { std::slice::from_raw_parts(plane_ptrs[0] as *const u8, plane_lens[0]) };
            let u =
                unsafe { std::slice::from_raw_parts(plane_ptrs[1] as *const u8, plane_lens[1]) };
            let v =
                unsafe { std::slice::from_raw_parts(plane_ptrs[2] as *const u8, plane_lens[2]) };
            crate::color::convert::yuv422_band_to_bgra_with_path(
                color_path,
                y,
                strides[0],
                u,
                strides[1],
                v,
                strides[2],
                y_row0,
                rows,
                width as usize,
                dst_band,
                dst_stride,
                table,
            );
        }
    };

    match pool {
        Some(pool) if pool.thread_count() > 1 && slices.len() > 1 => {
            pool.parallel_chunks_mut(slices, pack_chunk);
        }
        _ => pack_chunk(slices),
    }
}

/// Entropy-decode all slices into zigzag coefficient blocks (Y/U/V).
#[cfg(feature = "wgpu")]
pub fn decode_slices_coeffs(
    slices: &mut [SliceSet],
    strides: [usize; 3],
    dc_shift: i32,
) -> Vec<[Vec<crate::codec::plane::CoeffBlock>; 3]> {
    let n = slices.len();
    let mut out = Vec::with_capacity(n);
    for slice in slices.iter_mut() {
        prepare_slice_bitstream(slice);
        let mut dest = [Vec::new(), Vec::new(), Vec::new()];
        for pi in 0..3 {
            crate::codec::plane::decode_plane_coeffs(
                pi,
                strides[pi],
                &mut slice.dc,
                &mut slice.ac,
                dc_shift,
                &mut slice.temp_block,
                &mut dest[pi],
            );
        }
        out.push(dest);
    }
    out
}

/// Golomb-encode full-frame zigzag blocks into slices (Y/U/V).
#[cfg(feature = "wgpu")]
pub fn encode_slices_from_coeffs(
    slices: &mut [SliceSet],
    strides: [usize; 4],
    y_blocks: &[[i16; 64]],
    u_blocks: &[[i16; 64]],
    v_blocks: &[[i16; 64]],
    a_blocks: Option<&[[i16; 64]]>,
    dc_shift: i32,
) {
    let y_bx = strides[0] / 8;
    let u_bx = strides[1] / 8;
    let v_bx = strides[2] / 8;
    let a_bx = strides[3] / 8;
    let by_per_slice = (SLICE_HEIGHT as usize) / 8;
    for (si, slice) in slices.iter_mut().enumerate() {
        slice.reset();
        let y0 = si * by_per_slice * y_bx;
        let y1 = y0 + by_per_slice * y_bx;
        let u0 = si * by_per_slice * u_bx;
        let u1 = u0 + by_per_slice * u_bx;
        let v0 = si * by_per_slice * v_bx;
        let v1 = v0 + by_per_slice * v_bx;
        crate::codec::plane::encode_plane_from_blocks(
            0,
            strides[0],
            &y_blocks[y0.min(y_blocks.len())..y1.min(y_blocks.len())],
            &mut slice.dc,
            &mut slice.ac,
            dc_shift,
        );
        crate::codec::plane::encode_plane_from_blocks(
            1,
            strides[1],
            &u_blocks[u0.min(u_blocks.len())..u1.min(u_blocks.len())],
            &mut slice.dc,
            &mut slice.ac,
            dc_shift,
        );
        crate::codec::plane::encode_plane_from_blocks(
            2,
            strides[2],
            &v_blocks[v0.min(v_blocks.len())..v1.min(v_blocks.len())],
            &mut slice.dc,
            &mut slice.ac,
            dc_shift,
        );
        if let Some(a_blocks) = a_blocks {
            let a0 = si * by_per_slice * a_bx;
            let a1 = a0 + by_per_slice * a_bx;
            crate::codec::plane::encode_plane_from_blocks(
                3,
                strides[3],
                &a_blocks[a0.min(a_blocks.len())..a1.min(a_blocks.len())],
                &mut slice.dc,
                &mut slice.ac,
                dc_shift,
            );
        }
    }
}
