//! A whole file through the public APIs, which is the only target that reaches
//! the driver.
//!
//! The other three enter below it: `container` walks the RIFF without decoding
//! a pixel, and `vp8`/`vp8l` are handed a chunk the driver would have validated
//! first. What a caller can actually provoke is the composition of the two, so
//! this one drives what ships — both entry points, in every output format,
//! taking the first byte as the format so a mutation can move between them.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::ffi::c_void;
use std::mem;
use std::ptr;

use wpd::api::{self, Animation, Decoder, Options};
use wpd::image::Format;
use wpd_capi::decoder::{wpd_decode_into, WPDOutputBuffer};
use wpd_capi::frame::{WPDFrame, WPDOutputPlane};
use wpd_capi::options::WPDDecoderOptions;

const FORMATS: [Format; 16] = [
    Format::Yuv420p,
    Format::Yuva420p,
    Format::Argb,
    Format::Rgba,
    Format::Bgra,
    Format::Rgb,
    Format::Bgr,
    Format::ArgbPre,
    Format::RgbaPre,
    Format::BgraPre,
    Format::Rgb565,
    Format::Rgba4444,
    Format::Rgba4444Pre,
    Format::Bgr565,
    Format::Bgra4444,
    Format::Bgra4444Pre,
];

/// Enough frames to pass the end of any corpus animation, so the exhausted
/// path is reached rather than only the frames a file has.
const FRAMES: usize = 16;

fn byte(data: &[u8], at: usize) -> u8 {
    data.get(at).copied().unwrap_or(0)
}

fn decode_options(data: &[u8]) -> (Options, bool) {
    let flags = byte(data, 1);
    let subframe = flags & 4 != 0;
    let mut options = Options {
        bypass_filtering: flags & 8 != 0,
        no_fancy_upsampling: flags & 16 != 0,
        flip: !subframe && flags & 32 != 0,
        ..Options::default()
    };

    if !subframe {
        if let Ok(info) = api::info(data) {
            if flags & 1 != 0 && info.width > 0 && info.height > 0 {
                let left = i32::from(byte(data, 2)) % info.width;
                let top = i32::from(byte(data, 3)) % info.height;
                let width = 1 + i32::from(byte(data, 4)) % (info.width - left);
                let height = 1 + i32::from(byte(data, 5)) % (info.height - top);

                options.crop = Some((left, top, width, height));
            }
            if flags & 2 != 0 {
                let width = i32::from(byte(data, 6));
                let mut height = i32::from(byte(data, 7));

                if width == 0 && height == 0 {
                    height = 1;
                }
                options.scale = Some((width, height));
            }
        }
    }
    (options, subframe)
}

fn configure(decoder: &mut Decoder<'_>, format: Format, options: Options, subframe: bool) {
    let _ = decoder.set_format(format);
    let _ = decoder.set_options(options);
    if subframe {
        let _ = decoder.set_animation(Animation::Subframe);
    }
}

fn decode_external(data: &[u8], options: Options) {
    let size = usize::from(u16::from_le_bytes([byte(data, 8), byte(data, 9)]));
    let mut storage = vec![0; size];
    let stride = 1
        + isize::try_from(u16::from_le_bytes([byte(data, 10), byte(data, 11)]))
            .unwrap_or(1);
    let empty = WPDOutputPlane {
        data: ptr::null_mut(),
        size: 0,
        stride: 0,
    };
    let buffer = WPDOutputBuffer {
        struct_size: mem::size_of::<WPDOutputBuffer>(),
        plane: [
            WPDOutputPlane {
                data: storage.as_mut_ptr(),
                size: storage.len(),
                stride,
            },
            empty,
            empty,
            empty,
        ],
    };
    let options = WPDDecoderOptions {
        struct_size: mem::size_of::<WPDDecoderOptions>(),
        bypass_filtering: i32::from(options.bypass_filtering),
        no_fancy_upsampling: i32::from(options.no_fancy_upsampling),
        use_cropping: i32::from(options.crop.is_some()),
        crop_left: options.crop.map_or(0, |v| v.0),
        crop_top: options.crop.map_or(0, |v| v.1),
        crop_width: options.crop.map_or(0, |v| v.2),
        crop_height: options.crop.map_or(0, |v| v.3),
        use_scaling: i32::from(options.scale.is_some()),
        scaled_width: options.scale.map_or(0, |v| v.0),
        scaled_height: options.scale.map_or(0, |v| v.1),
        flip: i32::from(options.flip),
    };
    let mut frame = WPDFrame {
        struct_size: mem::size_of::<WPDFrame>(),
        data: [ptr::null(); 4],
        stride: [0; 4],
        width: 0,
        height: 0,
        format: 0,
        duration: 0,
        timestamp: 0,
        private_data: ptr::null_mut::<c_void>(),
        pos_x: 0,
        pos_y: 0,
        dispose: 0,
        blend: 0,
        has_alpha: 0,
    };

    unsafe {
        let _ = wpd_decode_into(
            data.as_ptr(),
            data.len(),
            Format::Argb as i32,
            &options,
            &buffer,
            &mut frame,
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(&first) = data.first() else {
        return;
    };
    let format = FORMATS[first as usize % FORMATS.len()];
    let (options, subframe) = decode_options(data);

    let mut whole = Decoder::new();
    configure(&mut whole, format, options, subframe);

    if whole.open(data).is_ok() {
        let _ = whole.info();
        for i in 0..4 {
            let _ = whole.frame_info(i);
        }
        for _ in 0..FRAMES {
            if whole.next_frame().is_err() {
                break;
            }
        }
    }

    decode_external(data, options);

    /* The same file arriving in pieces, decoded as far as it will go after
    each one, which is what puts a frame boundary inside a chunk. */
    let mut stream = Decoder::new();
    configure(&mut stream, format, options, subframe);

    if stream.open_stream().is_err() {
        return;
    }

    let step = (data.len() / 4).max(1);
    let mut offset = 0;

    while offset < data.len() {
        let end = (offset + step).min(data.len());
        if stream.append(&data[offset..end]).is_err() {
            return;
        }
        offset = end;

        for _ in 0..2 {
            if stream.next_frame().is_err() {
                break;
            }
        }

        let _ = stream.partial_frame();
    }

    let _ = stream.end_of_stream();

    for _ in 0..FRAMES {
        if stream.next_frame().is_err() {
            break;
        }
    }
});
