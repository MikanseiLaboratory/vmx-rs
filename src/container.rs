//! VMX1 container load/save matching `VMX_LoadFrom` / `VMX_SaveTo`.

use crate::codec::slice::SliceSet;
use crate::error::{Result, VmxError};
use crate::types::{CodecFormat, Format};

pub struct FrameHeader {
    pub format: Format,
    pub quality: i32,
    pub slice_count: usize,
    pub dc_shift: i32,
}

pub fn parse_and_load(
    data: &[u8],
    expected_slices: usize,
    slices: &mut [SliceSet],
) -> Result<FrameHeader> {
    if data.is_empty() {
        return Err(VmxError::InvalidParameters);
    }
    if data.len() < 3 {
        return Err(VmxError::BufferOverflow);
    }

    let mut offset = 0usize;
    let mut dc_shift = 0i32;
    let b0 = data[0];

    if b0 != CodecFormat::Progressive as u8
        && b0 != CodecFormat::Interlaced as u8
        && b0 != CodecFormat::Extended as u8
    {
        return Err(VmxError::InvalidCodecFormat);
    }

    if b0 == CodecFormat::Extended as u8 {
        if data.len() < 5 {
            return Err(VmxError::BufferOverflow);
        }
        offset = 2;
        dc_shift = data[1] as i32;
    }

    let format_byte = data[offset];
    let quality = data[offset + 1] as i32;
    let mut slice_count = data[offset + 2] as usize;

    // 8K special case
    if slice_count == 14 && expected_slices == 270 {
        slice_count = 270;
    }
    if slice_count != expected_slices {
        return Err(VmxError::InvalidSliceCount);
    }

    let format = if format_byte == CodecFormat::Interlaced as u8 {
        Format::Interlaced
    } else {
        Format::Progressive
    };

    let mut b = offset + 3;
    for i in 0..slice_count {
        if b + 4 > data.len() {
            return Err(VmxError::BufferOverflow);
        }
        let len = u32::from_le_bytes(data[b..b + 4].try_into().unwrap()) as usize;
        b += 4;
        if b + len > data.len() {
            return Err(VmxError::BufferOverflow);
        }
        if len > slices[i].dc.max_length {
            return Err(VmxError::BufferOverflow);
        }
        slices[i].dc.stream[..len].copy_from_slice(&data[b..b + len]);
        slices[i].dc.stream_length = len;
        slices[i].dc.pos = len;
        b += len;
    }

    if b < data.len() {
        for i in 0..slice_count {
            if b + 4 > data.len() {
                return Err(VmxError::BufferOverflow);
            }
            let len = u32::from_le_bytes(data[b..b + 4].try_into().unwrap()) as usize;
            b += 4;
            if b + len > data.len() {
                return Err(VmxError::BufferOverflow);
            }
            if len > slices[i].ac.max_length {
                return Err(VmxError::BufferOverflow);
            }
            slices[i].ac.stream[..len].copy_from_slice(&data[b..b + len]);
            slices[i].ac.stream_length = len;
            slices[i].ac.pos = len;
            b += len;
        }
    } else {
        // Preview-only: no AC
        for s in slices.iter_mut().take(slice_count) {
            s.ac.stream_length = 0;
            s.ac.pos = 0;
        }
    }

    Ok(FrameHeader {
        format,
        quality,
        slice_count,
        dc_shift,
    })
}

pub fn save_to(dst: &mut [u8], slices: &[SliceSet], format: Format, quality: i32, dc_shift: i32) -> Result<usize> {
    if dst.len() < 5 {
        return Err(VmxError::BufferOverflow);
    }
    let mut b = 0usize;
    let slice_count = slices.len();
    let sc_byte = if slice_count == 270 { 14u8 } else { slice_count as u8 };

    if dc_shift > 0 {
        dst[0] = CodecFormat::Extended as u8;
        dst[1] = dc_shift as u8;
        dst[2] = if format == Format::Interlaced {
            CodecFormat::Interlaced as u8
        } else {
            CodecFormat::Progressive as u8
        };
        dst[3] = quality as u8;
        dst[4] = sc_byte;
        b = 5;
    } else {
        dst[0] = if format == Format::Interlaced {
            CodecFormat::Interlaced as u8
        } else {
            CodecFormat::Progressive as u8
        };
        dst[1] = quality as u8;
        dst[2] = sc_byte;
        b = 3;
    }

    for s in slices {
        let len = s.dc.encoded_length();
        if b + 4 + len > dst.len() {
            return Err(VmxError::BufferOverflow);
        }
        dst[b..b + 4].copy_from_slice(&(len as u32).to_le_bytes());
        b += 4;
        dst[b..b + len].copy_from_slice(&s.dc.stream[..len]);
        b += len;
    }
    for s in slices {
        let len = s.ac.encoded_length();
        if b + 4 + len > dst.len() {
            return Err(VmxError::BufferOverflow);
        }
        dst[b..b + 4].copy_from_slice(&(len as u32).to_le_bytes());
        b += 4;
        dst[b..b + len].copy_from_slice(&s.ac.stream[..len]);
        b += len;
    }
    Ok(b)
}

pub fn encoded_preview_length(slices: &[SliceSet], format: Format, quality: i32, dc_shift: i32) -> usize {
    let header = if dc_shift > 0 { 5 } else { 3 };
    let mut len = header;
    for s in slices {
        len += 4 + s.dc.encoded_length();
    }
    let _ = (format, quality);
    len
}
