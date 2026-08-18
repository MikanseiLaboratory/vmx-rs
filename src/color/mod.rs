pub mod convert;
#[cfg(feature = "portable-simd")]
pub mod portable;
pub mod simd;

pub use simd::ColorSimdPath;
