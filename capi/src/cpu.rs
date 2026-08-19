//! C ABI for CPU detection, as declared by `src/cpu.h`.

use std::ffi::c_uint;

/// Runs feature detection. The decoder entry points call this before touching
/// any DSP table.
#[no_mangle]
pub extern "C" fn wpd_init_cpu() {
    wpd::cpu::init();
}

/// Restricts dispatch to `mask`. checkasm walks the CPU tiers with this, and
/// `wpd --cpumask` exposes it.
#[no_mangle]
pub extern "C" fn wpd_set_cpu_flags_mask(mask: c_uint) {
    wpd::cpu::set_mask(mask);
}

/// The detected feature bits with the mask applied.
///
/// `wpd_get_cpu_flags()` in `src/cpu.h` wraps this and adds the compile-time
/// baseline under `trim_dsp`. That union stays on the C side so it remains a
/// constant at the DSP init call sites, which is the whole point of trimming.
#[no_mangle]
pub extern "C" fn wpd_get_cpu_flags_raw() -> c_uint {
    wpd::cpu::detected_and_masked().bits()
}
