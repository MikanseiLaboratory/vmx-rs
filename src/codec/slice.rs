//! Slice-parallel encode/decode.

use crate::bitstream::SliceData;
use crate::codec::plane::{PlaneView, decode_plane_scalar};
use crate::simd::sse128::encode_plane;
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
                encode_slice_range(planes, chunk, encode_matrix, dc_shift, plane_count);
            });
        }
        _ => encode_slice_range(planes, slices, encode_matrix, dc_shift, plane_count),
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
            decode_plane_scalar(
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
            plane_ptrs,
            plane_lens,
            strides,
            slices,
            decode_matrix,
            dc_shift,
        ),
    }
}
