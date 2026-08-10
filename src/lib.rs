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
    ALIGNMENT, BITS_SIZE, ColorSpace, DECODE_MATRIX_COUNT, ENCODE_MATRIX_COUNT, Format,
    ImageFormat, MAX_HEIGHT, MAX_PLANES, MAX_Q, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH, Profile,
    QUALITY_COUNT, SLICE_HEIGHT, Size, align_up,
};

/// Convenience alias matching historical naming.
pub type Encoder = Codec;
/// Convenience alias matching historical naming.
pub type Decoder = Codec;

/// Hidden Criterion entry points (not a stable API).
///
/// `benches/*.rs` are separate crates, so they need `pub` kernels. Correctness
/// stays covered by private `#[cfg(test)]` units next to each implementation.
#[doc(hidden)]
pub mod kernels {
    pub use crate::codec::dct::{fdct_quant_zig, zig_invquant_idct};
    pub use crate::color::convert::{planar_to_uyvy_scalar, uyvy_to_planar_scalar};
    pub use crate::types::Size;

    #[cfg(target_arch = "x86_64")]
    pub use crate::color::convert::{planar_to_uyvy_sse2, uyvy_to_planar_ssse3};
    #[cfg(target_arch = "x86_64")]
    pub use crate::simd::sse128::{fdct_quant_zig_sse, zig_invquant_idct_sse};
}
