use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CpuFlags(u32);

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
impl CpuFlags {
    pub const SSE: Self = Self(1 << 0);
    pub const SSE2: Self = Self(1 << 1);
    pub const SSSE3: Self = Self(1 << 2);
    pub const SSE41: Self = Self(1 << 3);
    pub const AVX2: Self = Self(1 << 4);
}

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
impl CpuFlags {
    pub const ARMV6: Self = Self(1 << 0);
    pub const NEON: Self = Self(1 << 1);
    pub const DOTPROD: Self = Self(1 << 2);
    pub const I8MM: Self = Self(1 << 3);
}

impl CpuFlags {
    pub const NONE: Self = Self(0);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn compile_time() -> Self {
        let mut f = Self::NONE;

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if cfg!(target_feature = "avx2") {
                f = f.union(Self::AVX2);
            }
            if cfg!(target_feature = "sse4.1") || cfg!(target_feature = "avx2") {
                f = f.union(Self::SSE41);
            }
            if cfg!(target_feature = "ssse3") || cfg!(target_feature = "sse4.1") {
                f = f.union(Self::SSSE3);
            }
            if cfg!(target_feature = "sse2") || cfg!(target_feature = "ssse3") {
                f = f.union(Self::SSE2);
            }
            if cfg!(target_feature = "sse") || cfg!(target_feature = "sse2") {
                f = f.union(Self::SSE);
            }
        }

        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            if cfg!(target_arch = "aarch64") || cfg!(target_feature = "neon") {
                f = f.union(Self::NEON);
            }
            if cfg!(target_feature = "dotprod") {
                f = f.union(Self::DOTPROD);
            }
            if cfg!(target_feature = "i8mm") {
                f = f.union(Self::I8MM);
            }
            if cfg!(wpd_asm_armv6) {
                f = f.union(Self::ARMV6);
            }
        }

        f
    }

    #[cfg(not(feature = "asm"))]
    fn detect() -> Self {
        Self::NONE
    }

    #[cfg(feature = "asm")]
    fn detect() -> Self {
        let mut f = Self::compile_time();

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if std::arch::is_x86_feature_detected!("sse") {
                f = f.union(Self::SSE);
            }
            if std::arch::is_x86_feature_detected!("sse2") {
                f = f.union(Self::SSE2);
            }
            if std::arch::is_x86_feature_detected!("ssse3") {
                f = f.union(Self::SSSE3);
            }
            if std::arch::is_x86_feature_detected!("sse4.1") {
                f = f.union(Self::SSE41);
            }
            if std::arch::is_x86_feature_detected!("avx2")
                && std::arch::is_x86_feature_detected!("bmi1")
                && std::arch::is_x86_feature_detected!("bmi2")
            {
                f = f.union(Self::AVX2);
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                f = f.union(Self::NEON);
            }
            if std::arch::is_aarch64_feature_detected!("dotprod") {
                f = f.union(Self::DOTPROD);
            }
            if std::arch::is_aarch64_feature_detected!("i8mm") {
                f = f.union(Self::I8MM);
            }
        }

        #[cfg(target_arch = "arm")]
        {
            f = f.union(auxv::arm_flags());
        }

        f
    }
}

#[cfg(all(target_arch = "arm", feature = "asm"))]
mod auxv {
    use super::CpuFlags;

    const AT_NULL: u32 = 0;
    const AT_HWCAP: u32 = 16;
    const AT_HWCAP2: u32 = 26;

    const HWCAP_NEON: u32 = 1 << 12;
    const HWCAP_ASIMDDP: u32 = 1 << 24;
    const HWCAP_I8MM: u32 = 1 << 27;

    fn read() -> Option<(u32, u32)> {
        let raw = std::fs::read("/proc/self/auxv").ok()?;
        let mut hwcap = 0;
        let mut hwcap2 = 0;
        for entry in raw.chunks_exact(8) {
            let key = u32::from_ne_bytes(entry[0..4].try_into().ok()?);
            let val = u32::from_ne_bytes(entry[4..8].try_into().ok()?);
            match key {
                AT_NULL => break,
                AT_HWCAP => hwcap = val,
                AT_HWCAP2 => hwcap2 = val,
                _ => {}
            }
        }
        Some((hwcap, hwcap2))
    }

    pub fn arm_flags() -> CpuFlags {
        let mut f = CpuFlags::NONE;
        let Some((hwcap, _hwcap2)) = read() else {
            return f;
        };
        if hwcap & HWCAP_NEON != 0 {
            f = f.union(CpuFlags::NEON);
        }
        if hwcap & HWCAP_ASIMDDP != 0 {
            f = f.union(CpuFlags::DOTPROD);
        }
        if hwcap & HWCAP_I8MM != 0 {
            f = f.union(CpuFlags::I8MM);
        }
        f
    }
}

static DETECTED: AtomicU32 = AtomicU32::new(0);
static MASK: AtomicU32 = AtomicU32::new(u32::MAX);

pub fn init() {
    DETECTED.store(CpuFlags::detect().bits(), Ordering::Release);
}

pub fn set_mask(mask: u32) {
    MASK.store(mask, Ordering::Relaxed);
}

#[inline]
pub fn detected_and_masked() -> CpuFlags {
    let detected = CpuFlags::from_bits(DETECTED.load(Ordering::Acquire));
    let mask = CpuFlags::from_bits(MASK.load(Ordering::Relaxed));
    detected.intersection(mask)
}

#[inline]
pub fn flags() -> CpuFlags {
    let flags = detected_and_masked();
    if cfg!(feature = "trim_dsp") {
        flags.union(CpuFlags::compile_time())
    } else {
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_time_levels_imply_lower_ones() {
        let f = CpuFlags::compile_time();
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if f.contains(CpuFlags::AVX2) {
                assert!(f.contains(CpuFlags::SSE41));
            }
            if f.contains(CpuFlags::SSE41) {
                assert!(f.contains(CpuFlags::SSSE3));
            }
            if f.contains(CpuFlags::SSSE3) {
                assert!(f.contains(CpuFlags::SSE2));
            }
            if f.contains(CpuFlags::SSE2) {
                assert!(f.contains(CpuFlags::SSE));
            }
        }
        let _ = f;
    }

    #[test]
    fn detection_is_a_superset_of_the_compile_time_baseline() {
        init();
        let detected = CpuFlags::from_bits(DETECTED.load(Ordering::Acquire));
        if cfg!(feature = "asm") {
            assert!(detected.contains(CpuFlags::compile_time()));
        }
    }

    #[test]
    fn mask_removes_features() {
        init();
        set_mask(0);
        let expected = if cfg!(feature = "trim_dsp") {
            CpuFlags::compile_time()
        } else {
            CpuFlags::NONE
        };
        assert_eq!(flags(), expected);
        set_mask(u32::MAX);
    }
}
