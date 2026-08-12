//! SIMD safety notes for `vmx::simd`.
//!
//! # Safety requirements
//!
//! - All `std::arch` intrinsics are gated by `is_x86_feature_detected!` /
//!   `is_aarch64_feature_detected!` (or aarch64 baseline NEON) before
//!   `#[target_feature]` kernels are invoked.
//! - Callers must ensure plane buffers have length covering
//!   `offset + slice_height * stride`.
//! - Hardware capabilities are stored separately from the selected [`crate::simd::SimdPath`].
//!   AVX2 is only **selected** when AVX2+BMI2 are present **and** chroma width is
//!   divisible by 16 (matches libvmx); capability flags themselves are not cleared.
//! - Hot-path slice encode/decode must use the path fixed at [`crate::Codec`] creation —
//!   do not re-run CPUID inside plane loops.
//! - The SSE4.2 encoder uses an intrinsic FDCT/quantization path. SSE4.1 decode uses
//!   SIMD dequant + packus with scalar IDCT row/column oracles for bit-exactness.
//! - AVX2 / NEON plane kernels must remain bit-compatible with the scalar oracle.
//! - UYVY↔planar color conversion uses SSSE3/SSE2 when available.
//! - Slice encode/decode run on a CPU thread pool sized from host parallelism
//!   (not an async runtime — Tokio is for I/O only).

#![allow(dead_code)]

/// Documented entry for safety audit tooling.
pub const SIMD_SAFETY_VERSION: &str = "0.5.0-avx2-neon-dispatch";
