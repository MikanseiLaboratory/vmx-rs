//! Runtime CPU feature detection and path selection.
//!
//! Hardware capabilities (`SimdCapabilities`) are detected once and never
//! rewritten by image geometry. The selected execution path (`SimdPath`) is
//! resolved at [`Codec`](crate::Codec) creation from both capabilities and
//! content constraints (UV width % 16 for AVX2), matching libvmx.

use core::fmt;

/// Detected CPU SIMD / bit-manipulation features.
///
/// These reflect the host CPU only. Image-dependent constraints (for example
/// chroma width) do **not** clear these flags — they only affect [`SimdPath`]
/// selection via [`SimdCapabilities::select_path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimdCapabilities {
    pub ssse3: bool,
    pub sse42: bool,
    pub avx2: bool,
    pub bmi2: bool,
    pub avx512: bool,
    pub neon: bool,
    /// AArch64 SVE (FEAT_SVE).
    pub sve: bool,
    /// AArch64 SVE2 (FEAT_SVE2).
    pub sve2: bool,
}

/// Historical alias retained for compatibility with earlier vmx-rs APIs.
pub type CpuFeatures = SimdCapabilities;

impl SimdCapabilities {
    /// Detect host CPU features once. Safe to call from any thread.
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            Self {
                ssse3: is_x86_feature_detected!("ssse3"),
                sse42: is_x86_feature_detected!("sse4.2"),
                avx2: is_x86_feature_detected!("avx2"),
                bmi2: is_x86_feature_detected!("bmi2"),
                avx512: is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw"),
                neon: false,
                sve: false,
                sve2: false,
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // NEON / ASIMD is part of the AArch64 baseline used by Rust targets.
            // Prefer the runtime macro when available so diagnostics stay honest.
            let neon = {
                #[cfg(any(target_os = "linux", target_os = "android"))]
                {
                    std::arch::is_aarch64_feature_detected!("neon")
                }
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                {
                    true
                }
            };
            let (sve, sve2) = {
                #[cfg(any(target_os = "linux", target_os = "android"))]
                {
                    (
                        std::arch::is_aarch64_feature_detected!("sve"),
                        std::arch::is_aarch64_feature_detected!("sve2"),
                    )
                }
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                {
                    (false, false)
                }
            };
            Self {
                ssse3: false,
                sse42: false,
                avx2: false,
                bmi2: false,
                avx512: false,
                neon,
                sve,
                sve2,
            }
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::default()
        }
    }

    /// True when AVX2 plane kernels may be used (libvmx: AVX2 **and** BMI2).
    #[inline]
    pub fn avx2_bmi2(self) -> bool {
        self.avx2 && self.bmi2
    }

    /// True when AVX-512F+BW plane kernels may be used (also requires BMI2).
    #[inline]
    pub fn avx512_bmi2(self) -> bool {
        self.avx512 && self.bmi2
    }

    /// True when the SSE128 path is available (libvmx minimum: SSE4.2 + SSSE3).
    #[inline]
    pub fn sse128(self) -> bool {
        self.sse42 && self.ssse3
    }

    /// Select the execution path for a frame with the given chroma width
    /// (`width / 2` for 4:2:2).
    ///
    /// Priority:
    /// - x86_64: AVX-512 (if AVX512F+BW+BMI2 and `uv_width % 32 == 0`) →
    ///   AVX2 (if AVX2+BMI2 and `uv_width % 16 == 0`) → SSE128 → Scalar
    /// - aarch64: SVE (opt-in `sve` feature + FEAT_SVE) → Neon → Scalar
    /// - other: Scalar
    pub fn select_path(self, uv_width: usize) -> SimdPath {
        Self::select_path_with(self, uv_width)
    }

    /// Pure selection used by tests — inject arbitrary capabilities.
    pub fn select_path_with(caps: Self, uv_width: usize) -> SimdPath {
        #[cfg(target_arch = "x86_64")]
        {
            let _ = (caps.neon, caps.sve, caps.sve2);
            if caps.avx512_bmi2() && uv_width.is_multiple_of(32) {
                return SimdPath::Avx512;
            }
            if caps.avx2_bmi2() && uv_width.is_multiple_of(16) {
                return SimdPath::Avx2;
            }
            if caps.sse128() {
                return SimdPath::Sse128;
            }
            #[cfg(feature = "portable-simd")]
            {
                SimdPath::Portable
            }
            #[cfg(not(feature = "portable-simd"))]
            {
                SimdPath::Scalar
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            let _ = uv_width;
            #[cfg(feature = "sve")]
            if caps.sve {
                return SimdPath::Sve;
            }
            if caps.neon {
                return SimdPath::Neon;
            }
            #[cfg(feature = "portable-simd")]
            {
                SimdPath::Portable
            }
            #[cfg(not(feature = "portable-simd"))]
            {
                SimdPath::Scalar
            }
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            let _ = (caps, uv_width);
            #[cfg(feature = "portable-simd")]
            {
                let _ = caps;
                SimdPath::Portable
            }
            #[cfg(not(feature = "portable-simd"))]
            {
                SimdPath::Scalar
            }
        }
    }

    /// Legacy helper: preferred path ignoring image geometry (UV gate).
    /// Prefer [`Self::select_path`] for codec construction.
    pub fn preferred_path(self) -> SimdPath {
        self.select_path(16)
    }
}

/// Actually selected SIMD / scalar execution path for a [`crate::Codec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SimdPath {
    /// Pure Rust reference kernels.
    #[default]
    Scalar,
    /// SSE4.2 / SSSE3 128-bit path (x86_64).
    Sse128,
    /// AVX2 + BMI2 256-bit path (x86_64).
    Avx2,
    /// AVX-512F+BW 512-bit path (x86_64, four 8×8 blocks).
    Avx512,
    /// AArch64 NEON 128-bit path.
    Neon,
    /// AArch64 SVE/SVE2 path (nightly `sve` feature). Plane DCT uses NEON 8×8;
    /// color conversion can use scalable SVE kernels.
    #[cfg(feature = "sve")]
    Sve,
    /// Nightly `std::simd` portable path (`portable-simd` feature).
    #[cfg(feature = "portable-simd")]
    Portable,
}

impl SimdPath {
    /// Stable diagnostic name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Sse128 => "sse128",
            Self::Avx2 => "avx2",
            Self::Avx512 => "avx512",
            Self::Neon => "neon",
            #[cfg(feature = "sve")]
            Self::Sve => "sve",
            #[cfg(feature = "portable-simd")]
            Self::Portable => "portable",
        }
    }
}

impl fmt::Display for SimdPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_requires_bmi2_and_uv_multiple_of_16() {
        let caps = SimdCapabilities {
            ssse3: true,
            sse42: true,
            avx2: true,
            bmi2: true,
            avx512: false,
            neon: false,
            sve: false,
            sve2: false,
        };
        assert_eq!(
            SimdCapabilities::select_path_with(caps, 960),
            SimdPath::Avx2
        );
        assert_eq!(
            SimdCapabilities::select_path_with(caps, 961),
            SimdPath::Sse128
        );

        let no_bmi = SimdCapabilities {
            bmi2: false,
            ..caps
        };
        assert_eq!(
            SimdCapabilities::select_path_with(no_bmi, 960),
            SimdPath::Sse128
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse128_requires_both_sse42_and_ssse3() {
        let only_sse42 = SimdCapabilities {
            ssse3: false,
            sse42: true,
            avx2: false,
            bmi2: false,
            avx512: false,
            neon: false,
            sve: false,
            sve2: false,
        };
        #[cfg(feature = "portable-simd")]
        let expected_fallback = SimdPath::Portable;
        #[cfg(not(feature = "portable-simd"))]
        let expected_fallback = SimdPath::Scalar;
        assert_eq!(
            SimdCapabilities::select_path_with(only_sse42, 960),
            expected_fallback
        );

        let both = SimdCapabilities {
            ssse3: true,
            sse42: true,
            ..only_sse42
        };
        assert_eq!(
            SimdCapabilities::select_path_with(both, 960),
            SimdPath::Sse128
        );
    }

    #[test]
    fn simd_path_display_names() {
        assert_eq!(SimdPath::Scalar.to_string(), "scalar");
        assert_eq!(SimdPath::Sse128.to_string(), "sse128");
        assert_eq!(SimdPath::Avx2.to_string(), "avx2");
        assert_eq!(SimdPath::Avx512.to_string(), "avx512");
        assert_eq!(SimdPath::Neon.to_string(), "neon");
        #[cfg(feature = "sve")]
        assert_eq!(SimdPath::Sve.to_string(), "sve");
        #[cfg(feature = "portable-simd")]
        assert_eq!(SimdPath::Portable.to_string(), "portable");
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_selects_neon_when_capable() {
        let caps = SimdCapabilities {
            neon: true,
            ..Default::default()
        };
        assert_eq!(SimdCapabilities::select_path_with(caps, 7), SimdPath::Neon);
    }

    #[cfg(all(target_arch = "aarch64", feature = "sve"))]
    #[test]
    fn aarch64_prefers_sve_when_feature_and_capable() {
        let caps = SimdCapabilities {
            neon: true,
            sve: true,
            sve2: true,
            ..Default::default()
        };
        assert_eq!(SimdCapabilities::select_path_with(caps, 7), SimdPath::Sve);
    }
}
