use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Write};
use std::process::ExitCode;

use image_webp::{DecodingError, WebPDecoder};

const IMAGE_WEBP_VERSION: &str = env!("IMAGE_WEBP_VERSION");

/* image-webp decodes to rgb or rgba only; the other layouts are shuffled out
 * of those, the way libwebpdec converts what libwebp cannot emit directly.
 * off[] holds where alpha, red, green and blue sit in a pixel, -1 if absent. */
struct Layout {
    name: &'static str,
    bpp: usize,
    off: [isize; 4],
}

const fn layout(name: &'static str, bpp: usize, off: [isize; 4]) -> Layout {
    Layout { name, bpp, off }
}

const LAYOUTS: &[Layout] = &[
    layout("rgba", 4, [3, 0, 1, 2]),
    layout("rgb", 3, [-1, 0, 1, 2]),
    layout("argb", 4, [0, 1, 2, 3]),
    layout("bgra", 4, [3, 2, 1, 0]),
    layout("bgr", 3, [-1, 2, 1, 0]),
];

fn find_layout(name: &str) -> Option<&'static Layout> {
    LAYOUTS.iter().find(|l| l.name == name)
}

fn print_banner() {
    eprintln!(
        "imagewebpdec by Halide Compression, LLC | image-webp {IMAGE_WEBP_VERSION}"
    );
}

fn usage(app: &str, reason: Option<&str>) {
    if let Some(reason) = reason {
        eprintln!("\n{reason}");
    }
    eprint!(
        "\nusage:  {app} [options] input output\n\
         \noptions:\n\
         \x20-h, --help\n\
         \x20   view help menu\n\
         \x20-r, --repeat u32\n\
         \x20   repeat decode for benchmarking (1..INT_MAX); default 1\n\
         \x20-f, --fmt str\n\
         \x20   output pixel format; default auto. one of\n\
         \x20   auto, rgba, rgb, argb, bgra, bgr\n"
    );
}

fn parse_repeat(value: &str) -> Option<i32> {
    if value.starts_with('-') {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|v| *v >= 1 && *v <= i32::MAX as u64)
        .map(|v| v as i32)
}

fn parse_format(value: &str) -> Option<Option<&'static Layout>> {
    if value == "auto" {
        return Some(None);
    }
    find_layout(value).map(Some)
}

#[derive(Default)]
struct Options {
    repeat: i32,
    want: Option<&'static Layout>,
    positional: Vec<String>,
}

enum Parsed {
    Ok(Options),
    Help,
    Bad(&'static str),
}

const MISSING: &str = "unknown option or missing option value";
const BAD_REPEAT: &str = "invalid repeat value; expected 1..INT_MAX";
const BAD_FORMAT: &str = "invalid output pixel format";

fn parse_args(argv: &[String]) -> Parsed {
    let mut o = Options {
        repeat: 1,
        ..Default::default()
    };
    let mut i = 1;
    let mut only_operands = false;

    while i < argv.len() {
        let arg = argv[i].clone();

        i += 1;
        if only_operands || arg == "-" || !arg.starts_with('-') {
            o.positional.push(arg);
            continue;
        }
        if arg == "--" {
            only_operands = true;
            continue;
        }

        let (name, attached) = if let Some(long) = arg.strip_prefix("--") {
            match long.split_once('=') {
                Some((n, v)) => (n.to_owned(), Some(v.to_owned())),
                None => (long.to_owned(), None),
            }
        } else {
            let letters = &arg[1..];

            (
                letters[..1].to_owned(),
                (letters.len() > 1).then(|| letters[1..].to_owned()),
            )
        };

        let takes_value = match name.as_str() {
            "h" | "help" => {
                return Parsed::Help;
            }
            "r" | "repeat" | "f" | "fmt" => true,
            _ => return Parsed::Bad(MISSING),
        };
        let value = if takes_value {
            let next = attached.or_else(|| {
                let v = argv.get(i).cloned();

                if v.is_some() {
                    i += 1;
                }
                v
            });

            match next {
                Some(v) => v,
                None => return Parsed::Bad(MISSING),
            }
        } else {
            String::new()
        };

        match name.as_str() {
            "r" | "repeat" => match parse_repeat(&value) {
                Some(v) => o.repeat = v,
                None => return Parsed::Bad(BAD_REPEAT),
            },
            "f" | "fmt" => match parse_format(&value) {
                Some(want) => o.want = want,
                None => return Parsed::Bad(BAD_FORMAT),
            },
            _ => return Parsed::Bad(MISSING),
        }
    }
    Parsed::Ok(o)
}

fn write_frame(
    sink: &mut dyn Write,
    pixels: &[u8],
    have: &Layout,
    want: &Layout,
    width: usize,
    height: usize,
) -> std::io::Result<()> {
    if have.name == want.name {
        return sink.write_all(pixels);
    }

    let mut row = vec![0u8; width * want.bpp];

    for y in 0..height {
        let src = &pixels[y * width * have.bpp..][..width * have.bpp];

        for x in 0..width {
            for (&dst, &src_off) in want.off.iter().zip(have.off.iter()) {
                let Ok(dst) = usize::try_from(dst) else {
                    continue;
                };

                row[want.bpp * x + dst] = match usize::try_from(src_off) {
                    Ok(off) => src[have.bpp * x + off],
                    Err(_) => 0xff,
                };
            }
        }
        sink.write_all(&row)?;
    }
    Ok(())
}

/* The other harnesses print bare strerror() text; drop rust's suffix. */
fn errmsg(e: &std::io::Error) -> String {
    let text = e.to_string();

    match text.find(" (os error ") {
        Some(cut) => text[..cut].to_owned(),
        None => text,
    }
}

fn read_file(name: &str) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();

    if name == "-" {
        std::io::stdin().read_to_end(&mut data)?;
    } else {
        File::open(name)?.read_to_end(&mut data)?;
    }
    Ok(data)
}

fn decode(
    data: &[u8],
    mut sink: Option<&mut dyn Write>,
    want: Option<&'static Layout>,
) -> Result<i32, DecodingError> {
    let mut decoder = WebPDecoder::new(Cursor::new(data))?;
    let (width, height) = decoder.dimensions();
    let have = find_layout(if decoder.has_alpha() { "rgba" } else { "rgb" }).unwrap();
    let want = want.unwrap_or(have);
    let size = decoder
        .output_buffer_size()
        .ok_or(DecodingError::ImageTooLarge)?;
    let mut pixels = vec![0u8; size];
    let mut frames = 0;

    loop {
        if decoder.is_animated() {
            match decoder.read_frame(&mut pixels) {
                Ok(_) => {}
                Err(DecodingError::NoMoreFrames) => break,
                Err(e) => return Err(e),
            }
        } else {
            decoder.read_image(&mut pixels)?;
        }
        if let Some(sink) = sink.as_deref_mut() {
            write_frame(sink, &pixels, have, want, width as usize, height as usize)?;
        }
        frames += 1;
        if !decoder.is_animated() {
            break;
        }
    }
    Ok(frames)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let app = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "imagewebpdec".into());

    print_banner();

    let opts = match parse_args(&argv) {
        Parsed::Ok(o) => o,
        Parsed::Help => {
            usage(&app, None);
            return ExitCode::SUCCESS;
        }
        Parsed::Bad(reason) => {
            usage(&app, Some(reason));
            return ExitCode::from(2);
        }
    };

    if opts.positional.len() != 2 {
        usage(
            &app,
            Some(if opts.positional.len() < 2 {
                "input and output are required"
            } else {
                "unexpected argument"
            }),
        );
        return ExitCode::from(2);
    }

    let input_name = &opts.positional[0];
    let output_name = &opts.positional[1];
    let data = match read_file(input_name) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{input_name}: {}", errmsg(&e));
            return ExitCode::FAILURE;
        }
    };
    let mut output = if output_name == "/dev/null" {
        None
    } else {
        match File::create(output_name) {
            Ok(f) => Some(BufWriter::new(f)),
            Err(e) => {
                eprintln!("{output_name}: {}", errmsg(&e));
                return ExitCode::FAILURE;
            }
        }
    };
    let mut frames = 0;

    for iter in 0..opts.repeat {
        let sink: Option<&mut dyn Write> = match output.as_mut() {
            Some(w) if iter == 0 => Some(w),
            _ => None,
        };

        frames = match decode(&data, sink, opts.want) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{input_name}: {e}");
                return ExitCode::FAILURE;
            }
        };
    }
    if frames == 0 {
        eprintln!("{input_name}: no image data found");
        return ExitCode::FAILURE;
    }
    if let Some(mut output) = output {
        if let Err(e) = output.flush() {
            eprintln!("write: {}", errmsg(&e));
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
