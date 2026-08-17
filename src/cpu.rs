//! Runtime CPU feature detection and the dispatch mask.
//!
//! The bit values match the C `WpdX86CpuFlags` / `WpdArmCpuFlags` enums, because
//! `tools/wpd.c --cpumask` and checkasm both name them on the command line.
//!
//! Detection itself goes through `std`'s `is_*_feature_detected!` macros, which
//! are safe and already do the awkward parts correctly — the OSXSAVE and XCR0
//! checks on x86, `getauxval` on Linux, `sysctl` on Apple. Where `std` has no
//! stable detection (32-bit arm), `/proc/self/auxv` is parsed directly, which
//! is a file read rather than a libc call and so stays safe too.

use core::sync::atomic::{AtomicU32, Ordering};

/// Feature bits, matching the C enums bit for bit.
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

    /// Whether every bit of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The feature set the compiler was told to target.
    ///
    /// Detection starts from these so that a build with, say, `-C
    /// target-cpu=native` can constant-fold the dispatch away; see
    /// [`flags`] and the `trim_dsp` feature.
    pub const fn compile_time() -> Self {
        let mut f = Self::NONE;

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            // Listed high to low: each level implies the ones below it, exactly
            // as wpd_get_default_cpu_flags() does in src/cpu.h.
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
            // The ARMv6 assembly is only assembled when the target baseline
            // supports it, so this is a compile-time property, not a runtime
            // one. The build script sets the cfg.
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
            // std checks OSXSAVE and XCR0 before reporting AVX2, which is the
            // part the equivalent C in src/x86/cpu.c had to spell out.
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
            // The assembly assumes BMI1/BMI2 alongside AVX2, as the C did.
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

/// Detection on 32-bit arm, where `std` has no stable feature detection.
///
/// `/proc/self/auxv` is the same table `getauxval` reads, as pairs of
/// pointer-sized words terminated by an `AT_NULL` key, so parsing it needs no
/// libc call and stays within safe Rust.
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

/// Runs feature detection. Idempotent, and safe to call from anywhere.
pub fn init() {
    DETECTED.store(CpuFlags::detect().bits(), Ordering::Release);
}

/// Restricts dispatch to `mask`, for checkasm and `wpd --cpumask`.
pub fn set_mask(mask: u32) {
    MASK.store(mask, Ordering::Relaxed);
}

/// What detection found, less anything [`set_mask`] removed.
///
/// This is the plain answer, without the `trim_dsp` union; the C ABI wrapper
/// needs it separately because `wpd_get_cpu_flags()` in `src/cpu.h` applies
/// that union itself, where it stays a compile-time constant.
#[inline]
pub fn detected_and_masked() -> CpuFlags {
    let detected = CpuFlags::from_bits(DETECTED.load(Ordering::Acquire));
    let mask = CpuFlags::from_bits(MASK.load(Ordering::Relaxed));
    detected.intersection(mask)
}

/// The features dispatch may use.
///
/// With `trim_dsp` the compile-time feature set is unioned in, so that a DSP
/// init this is inlined into can drop the fallbacks the build target could
/// never have reached. A binary cannot run on a CPU below its own target
/// anyway.
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
        // The binary is running, so every feature it was built for exists.
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
        // trim_dsp unions the compile-time set back in on purpose.
        let expected = if cfg!(feature = "trim_dsp") {
            CpuFlags::compile_time()
        } else {
            CpuFlags::NONE
        };
        assert_eq!(flags(), expected);
        set_mask(u32::MAX);
    }
}
