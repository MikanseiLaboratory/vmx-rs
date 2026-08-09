//! Runtime CPU feature detection and path selection.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuFeatures {
    pub sse42: bool,
    pub avx2: bool,
    pub bmi2: bool,
    pub neon: bool,
}

impl CpuFeatures {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let sse42 = is_x86_feature_detected!("sse4.2");
            let avx2 = is_x86_feature_detected!("avx2");
            let bmi2 = is_x86_feature_detected!("bmi2");
            // Match libvmx: AVX2 path requires both AVX2 and BMI2.
            return Self {
                sse42,
                avx2: avx2 && bmi2,
                bmi2,
                neon: false,
            };
        }
        #[cfg(target_arch = "aarch64")]
        {
            return Self {
                sse42: false,
                avx2: false,
                bmi2: false,
                neon: true,
            };
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::default()
        }
    }

    pub fn preferred_path(self) -> SimdPath {
        if self.avx2 {
            SimdPath::Avx2
        } else if self.sse42 {
            SimdPath::Sse128
        } else if self.neon {
            SimdPath::Neon
        } else {
            SimdPath::Scalar
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdPath {
    Scalar,
    Sse128,
    Avx2,
    Neon,
}
