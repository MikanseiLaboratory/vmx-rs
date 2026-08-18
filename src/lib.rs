//! Pure Rust VMX1 video codec — byte-compatible with libvmx.
//!
//! No native library linkage. SIMD paths use `std::arch` with runtime dispatch.
//! Optional nightly `portable-simd` feature adds a `std::simd` fallback path.

#![allow(clippy::too_many_arguments)]
#![cfg_attr(feature = "portable-simd", feature(portable_simd))]

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

pub use color::ColorSimdPath;
pub use container::preview_bitstream_length;
pub use error::{Result, VmxError};
pub use instance::{Codec, Config};
pub use simd::{SimdCapabilities, SimdPath};
pub use types::{
    ALIGNMENT, BITS_SIZE, ColorSpace, DECODE_MATRIX_COUNT, ENCODE_MATRIX_COUNT, Format,
    ImageFormat, MAX_HEIGHT, MAX_PLANES, MAX_Q, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH, Profile,
    QUALITY_COUNT, SLICE_HEIGHT, Size, align_up,
};

/// Convenience alias matching historical naming.
pub type Encoder = Codec;
/// Convenience alias matching historical naming.
pub type Decoder = Codec;
