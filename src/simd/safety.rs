//! SIMD safety notes for `vmx::simd`.
//!
//! # Safety requirements
//!
//! - All `std::arch` intrinsics are gated by `is_x86_feature_detected!` / aarch64 cfg.
//! - Callers must ensure plane buffers have length covering `offset + slice_height * stride`.
//! - AVX2 path is disabled when chroma width is not divisible by 16 (matches libvmx).
//! - The SSE4.2 encoder uses an intrinsic FDCT/quantization path. SSE4.1 decode uses
//!   SIMD dequant + packus with scalar IDCT row/column oracles for bit-exactness.
//! - UYVY↔planar color conversion uses SSSE3/SSE2 when available.
//! - Slice encode/decode run on a CPU thread pool sized from host parallelism
//!   (not an async runtime — Tokio is for I/O only).

#![allow(dead_code)]

/// Documented entry for safety audit tooling.
pub const SIMD_SAFETY_VERSION: &str = "0.4.0-sse-idct-dequant";
