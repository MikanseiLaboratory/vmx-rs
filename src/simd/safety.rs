//! SIMD safety notes for `vmx::simd`.
//!
//! # Safety requirements
//!
//! - All `std::arch` intrinsics are gated by `is_x86_feature_detected!` / aarch64 cfg.
//! - Callers must ensure plane buffers have length covering `offset + slice_height * stride`.
//! - AVX2 path is disabled when chroma width is not divisible by 16 (matches libvmx).
//! - Until full intrinsic ports land, SSE/AVX2/NEON modules delegate to the scalar oracle
//!   after feature checks — preserving correctness while enabling runtime dispatch wiring.

#![allow(dead_code)]

/// Documented entry for safety audit tooling.
pub const SIMD_SAFETY_VERSION: &str = "0.1.0-scalar-oracle";
