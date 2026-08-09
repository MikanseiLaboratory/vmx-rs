//! Static tables and constants ported from `vmxcodec_common.h`.

#![allow(dead_code)]

#[allow(dead_code, clippy::all)]
mod generated {
    include!("generated/tables_data.rs");
}

pub use generated::{
    GolombLookup, GolombZeroCode, BITS_LEFT_LOOKUP, FTAB1_128, FTAB2_128, FTAB3_128, FTAB4_128,
    GOLOMB_LENGTH_LUT, GOLOMB_LOOKUP_LUT, GOLOMB_ZERO_CODE_LUT, QUANT_MATRIX,
};

pub const QUALITY: [i32; 25] = [
    1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 40, 44, 48, 52, 56, 64,
];

pub const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52,
    45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

pub const ZIGZAG_INV: [u8; 64] = [
    0, 1, 5, 6, 14, 15, 27, 28, 2, 4, 7, 13, 16, 26, 29, 42, 3, 8, 12, 17, 25, 30, 41, 43, 9, 11,
    18, 24, 31, 40, 44, 53, 10, 19, 23, 32, 39, 45, 52, 54, 20, 22, 33, 38, 46, 51, 55, 60, 21, 34,
    37, 47, 50, 56, 59, 61, 35, 36, 48, 49, 57, 58, 62, 63,
];

/// Bitrate table: [profile, resolution_height, target_mbps, dc_shift, min_q, threads]
pub const BITRATE_TABLE: [[i32; 6]; 36] = [
    [99, 4320, 1320, 0, 80, 8],
    [199, 4320, 1200, 0, 52, 8],
    [66, 4320, 660, 3, 60, 8],
    [166, 4320, 600, 3, 52, 8],
    [33, 4320, 440, 3, 60, 8],
    [133, 4320, 400, 3, 52, 8],
    [99, 2160, 800, 0, 80, 4],
    [199, 2160, 600, 0, 52, 4],
    [66, 2160, 400, 3, 60, 4],
    [166, 2160, 300, 3, 52, 4],
    [33, 2160, 266, 3, 60, 4],
    [133, 2160, 200, 3, 52, 4],
    [99, 1440, 504, 0, 80, 4],
    [199, 1440, 450, 0, 52, 4],
    [66, 1440, 252, 3, 60, 4],
    [166, 1440, 300, 0, 52, 4],
    [33, 1440, 168, 3, 60, 4],
    [133, 1440, 120, 3, 52, 4],
    [99, 1080, 260, 0, 80, 2],
    [199, 1080, 260, 0, 52, 2],
    [66, 1080, 130, 3, 60, 2],
    [166, 1080, 200, 0, 52, 2],
    [33, 1080, 86, 3, 60, 2],
    [133, 1080, 86, 3, 52, 2],
    [99, 720, 136, 0, 80, 2],
    [199, 720, 136, 0, 52, 2],
    [66, 720, 68, 3, 60, 2],
    [166, 720, 68, 3, 52, 2],
    [33, 720, 45, 3, 60, 2],
    [133, 720, 45, 3, 52, 2],
    [99, 0, 72, 0, 80, 2],
    [199, 0, 72, 0, 52, 2],
    [66, 0, 36, 3, 60, 2],
    [166, 0, 36, 3, 52, 2],
    [33, 0, 24, 3, 60, 2],
    [133, 0, 24, 3, 52, 2],
];

pub const BR_PROFILE: usize = 0;
pub const BR_RESOLUTION: usize = 1;
pub const BR_TARGET: usize = 2;
pub const BR_SHIFT: usize = 3;
pub const BR_MINQ: usize = 4;
pub const BR_THREADS: usize = 5;

#[derive(Clone, Copy)]
pub struct ShortRgb {
    pub r: i16,
    pub g: i16,
    pub b: i16,
}

pub const RGB_YUV_709: [ShortRgb; 3] = [
    ShortRgb {
        r: 47,
        g: 157,
        b: 16,
    },
    ShortRgb {
        r: -26,
        g: -86,
        b: 112,
    },
    ShortRgb {
        r: 112,
        g: -102,
        b: -10,
    },
];

pub const RGB_YUV_601: [ShortRgb; 3] = [
    ShortRgb {
        r: 66,
        g: 129,
        b: 25,
    },
    ShortRgb {
        r: -38,
        g: -74,
        b: 112,
    },
    ShortRgb {
        r: 112,
        g: -94,
        b: -18,
    },
];

/// YUV→RGB BT.709 scaled ×16384: [Y, R, GU, GV, B]
pub const YUV_RGB_709: [i16; 5] = [19077, 29372, 3494, 8731, 17305];
pub const YUV_RGB_601: [i16; 5] = [19077, 26149, 6419, 13320, 16525];

// DCT / IDCT shift constants
pub const BITS_INV_ACC: i32 = 5;
pub const SHIFT_INV_ROW: i32 = 11;
pub const SHIFT_INV_COL: i32 = 6;
pub const IRND_INV_ROW: i32 = 1024;
pub const IRND_INV_COL: i32 = 32;
pub const IRND_INV_CORR: i32 = 31;
pub const BITS_FRW_ACC: i32 = 3;
pub const SHIFT_FRW_COL: i32 = 3;
pub const SHIFT_FRW_ROW: i32 = 16;
pub const RND_FRW_ROW: i32 = 32768;
pub const FDCT_ROUND1: i16 = 1;
pub const FDCT_TAN1: i16 = 13036;
pub const FDCT_TAN2: i16 = 27146;
/// Intentional u16 wrap of tan(3π/16)−1
pub const FDCT_TAN3: i16 = -21746; // 43790 as i16
pub const FDCT_SQRT2: i16 = 23170;

#[inline]
pub fn golomb_code_length(input: u32) -> u32 {
    if input as usize >= GOLOMB_LENGTH_LUT.len() {
        let bl = 32 - input.leading_zeros();
        return bl * 2 - 1;
    }
    GOLOMB_LENGTH_LUT[input as usize] as u32
}
