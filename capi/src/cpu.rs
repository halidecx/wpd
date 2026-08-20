use std::ffi::c_uint;

#[no_mangle]
pub extern "C" fn wpd_init_cpu() {
    crate::guard((), wpd::cpu::init);
}

#[no_mangle]
pub extern "C" fn wpd_set_cpu_flags_mask(mask: c_uint) {
    crate::guard((), || wpd::cpu::set_mask(mask));
}

#[no_mangle]
pub extern "C" fn wpd_get_cpu_flags_raw() -> c_uint {
    crate::guard(0, || wpd::cpu::detected_and_masked().bits())
}
