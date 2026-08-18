pub mod avx2;
pub mod dispatch;
pub mod neon;
pub mod plane_dispatch;
#[cfg(feature = "portable-simd")]
pub mod portable;
pub mod safety;
pub mod scalar;
pub mod sse128;

#[cfg(test)]
mod path_tests;

pub use dispatch::{CpuFeatures, SimdCapabilities, SimdPath};
pub use plane_dispatch::{decode_plane, encode_plane};
