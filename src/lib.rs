//! Pure Rust VMX1 video codec — byte-compatible with libvmx.
//!
//! No native library linkage. SIMD paths use `std::arch` with runtime dispatch.

#![allow(clippy::too_many_arguments)]

mod bitrate;
mod bitstream;
mod codec;
mod color;
mod container;
mod error;
mod instance;
pub mod simd;
mod tables;
mod thread_pool;
mod types;

pub use error::{Result, VmxError};
pub use instance::{Codec, Config};
pub use types::{
    align_up, ColorSpace, Format, ImageFormat, Profile, Size, ALIGNMENT, BITS_SIZE,
    DECODE_MATRIX_COUNT, ENCODE_MATRIX_COUNT, MAX_HEIGHT, MAX_PLANES, MAX_Q, MAX_WIDTH, MIN_HEIGHT,
    MIN_WIDTH, QUALITY_COUNT, SLICE_HEIGHT,
};

/// Convenience alias matching historical naming.
pub type Encoder = Codec;
/// Convenience alias matching historical naming.
pub type Decoder = Codec;
