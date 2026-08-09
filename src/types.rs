//! Public types and constants matching libvmx.

/// Bit buffer width used by the exp-Golomb reader/writer.
pub const BITS_SIZE: i32 = 64;
/// Slice height in pixels.
pub const SLICE_HEIGHT: i32 = 16;
/// Number of quality presets.
pub const QUALITY_COUNT: usize = 25;
/// Maximum number of image planes.
pub const MAX_PLANES: usize = 4;
/// Decode quantization matrix length.
pub const DECODE_MATRIX_COUNT: usize = 64;
/// Encode quantization matrix length.
pub const ENCODE_MATRIX_COUNT: usize = 192;
/// Plane / block alignment.
pub const ALIGNMENT: i32 = 16;
/// Minimum supported width.
pub const MIN_WIDTH: i32 = 16;
/// Minimum supported height.
pub const MIN_HEIGHT: i32 = 16;
/// Maximum supported width (8K).
pub const MAX_WIDTH: i32 = 7680;
/// Maximum supported height (8K).
pub const MAX_HEIGHT: i32 = 4320;
/// Maximum quality index (`QUALITY_COUNT - 1`).
pub const MAX_Q: i32 = 24;

/// Align `value` up to a multiple of `align`.
#[inline]
pub const fn align_up(value: i32, align: i32) -> i32 {
    ((value + align - 1) / align) * align
}

/// Image dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

impl Size {
    /// Create a new size.
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// Progressive vs interlaced frame layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum Format {
    /// Progressive scan.
    #[default]
    Progressive = 0,
    /// Interlaced scan.
    Interlaced = 1,
}

/// Compression profile / quality preset (values match `VMX_PROFILE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum Profile {
    /// Default profile.
    #[default]
    Default = 0,
    /// Low quality.
    Lq = 33,
    /// Standard quality.
    Sq = 66,
    /// High quality.
    Hq = 99,
    /// OMT low quality.
    OmtLq = 133,
    /// OMT standard quality.
    OmtSq = 166,
    /// OMT high quality.
    OmtHq = 199,
}

/// Color space for RGB↔YUV conversion (values match `VMX_COLORSPACE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum ColorSpace {
    /// Undefined — BT601 for SD (height &lt; 720), BT709 for HD.
    #[default]
    Undefined = 0,
    /// BT.601.
    Bt601 = 601,
    /// BT.709.
    Bt709 = 709,
}

/// Encoded bitstream layout (values match `VMX_CODEC_FORMAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum CodecFormat {
    /// No format set.
    #[default]
    None = 0,
    /// Progressive bitstream.
    Progressive = 1,
    /// Interlaced bitstream.
    Interlaced = 2,
    /// Extended bitstream.
    Extended = 3,
}

/// Uncompressed image pixel format (values match `VMX_IMAGE_FORMAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum ImageFormat {
    /// UYVY 4:2:2 packed.
    #[default]
    Uyvy = 0,
    /// YUY2 4:2:2 packed.
    Yuy2 = 1,
    /// NV12 4:2:0 semi-planar.
    Nv12 = 2,
    /// YV12 4:2:0 planar.
    Yv12 = 3,
    /// Planar YUV 4:2:2.
    YuvPlanar422 = 4,
    /// BGRA 32bpp.
    Bgra = 5,
    /// BGRX 32bpp (no alpha).
    Bgrx = 6,
    /// UYVA (UYVY + alpha plane).
    Uyva = 7,
    /// P216 16-bit 4:2:2.
    P216 = 8,
    /// PA16 (P216 + 16-bit alpha).
    Pa16 = 9,
}
