pub mod avx2;
pub mod dispatch;
pub mod neon;
pub mod safety;
pub mod scalar;
pub mod sse128;

pub use dispatch::CpuFeatures;
