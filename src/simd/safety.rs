//! SIMD safety notes for `vmx::simd`.
//!
//! # Safety requirements
//!
//! - All `std::arch` intrinsics are gated by `is_x86_feature_detected!` / aarch64 cfg.
//! - Callers must ensure plane buffers have length covering `offset + slice_height * stride`.
//! - AVX2 path is disabled when chroma width is not divisible by 16 (matches libvmx).
//! - The SSE4.2 encoder uses an intrinsic FDCT/quantization path; its final
//!   zigzag layout is scalar-indexed for auditability. AVX2, NEON, and IDCT
//!   still delegate to the scalar oracle after feature checks.
//! - UYVY↔planar color conversion uses SSSE3/SSE2 when available.
//! - Slice encode/decode run on a CPU thread pool sized from the bitrate table
//!   (not an async runtime — Tokio is for I/O only).

#![allow(dead_code)]

/// Documented entry for safety audit tooling.
pub const SIMD_SAFETY_VERSION: &str = "0.3.0-sse-fdct-parallel";
