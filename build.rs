//! Emits the `arm32_neon` cfg used by the AArch32 NEON kernels.
//!
//! Those kernels need `#![feature(stdarch_arm_neon_intrinsics)]` and
//! `#![feature(stdarch_arm_feature_detection)]`, which are nightly-only
//! (rust-lang/rust#111800). Enabling the `arm32-neon` cargo feature therefore
//! requires a nightly toolchain; on stable the feature is simply not compiled
//! and ARMv7 falls back to the scalar kernels.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(arm32_neon)");
    if std::env::var_os("CARGO_FEATURE_ARM32_NEON").is_some() {
        println!("cargo:rustc-cfg=arm32_neon");
    }
}
