//! The RIFF walk, which is the first thing any input reaches.
//!
//! Both entry points are driven: the one-shot information read, and the
//! resumable scan a stream uses, fed in two pieces so the resume path is
//! exercised as well as the complete one.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wpd::container::Scan;

fuzz_target!(|data: &[u8]| {
    let _ = wpd::container::get_info(data);

    let mut scan = Scan::new();
    let split = data.len() / 2;

    let _ = scan.headers(&data[..split], 0, true, true);
    let _ = scan.headers(data, 0, false, true);

    for i in 0..4 {
        let _ = scan.frame(i);
    }
});
