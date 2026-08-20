mod md5;
mod output;

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::process::ExitCode;

use wpd::api::{self, Animation, Decoder, Metadata};
use wpd::image::Format;

use output::{format_name, Muxer, Output, PIXEL_FORMATS};

const VCS_VERSION: &str = env!("WPD_VCS_VERSION");

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const CPU_MASK_NAMES: &str = "sse, sse2, ssse3, sse41, avx2, none";
#[cfg(target_arch = "arm")]
const CPU_MASK_NAMES: &str = "armv6, neon, none";
#[cfg(target_arch = "aarch64")]
const CPU_MASK_NAMES: &str = "neon, none";
#[cfg(not(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "arm",
    target_arch = "aarch64"
)))]
const CPU_MASK_NAMES: &str = "none";

fn cpu_masks() -> Vec<(&'static str, u32)> {
    #[allow(unused_mut)]
    let mut masks: Vec<(&'static str, u32)> = Vec::new();

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        use wpd::cpu::CpuFlags as F;

        let sse = F::SSE.bits();
        let sse2 = sse | F::SSE2.bits();
        let ssse3 = sse2 | F::SSSE3.bits();
        let sse41 = ssse3 | F::SSE41.bits();

        masks.push(("sse", sse));
        masks.push(("sse2", sse2));
        masks.push(("ssse3", ssse3));
        masks.push(("sse41", sse41));
        masks.push(("avx2", sse41 | F::AVX2.bits()));
    }
    #[cfg(target_arch = "arm")]
    {
        use wpd::cpu::CpuFlags as F;

        masks.push(("armv6", F::ARMV6.bits()));
        masks.push(("neon", F::NEON.bits() | F::ARMV6.bits()));
    }
    #[cfg(target_arch = "aarch64")]
    {
        masks.push(("neon", wpd::cpu::CpuFlags::NEON.bits()));
    }
    masks.push(("none", 0));
    masks
}

fn print_banner() {
    eprintln!(
        "wpd by Halide Compression, LLC | {} | {VCS_VERSION}",
        api::version()
    );
}

const USAGE_HEAD: &str = concat!(
    "\noptions:\n",
    " -h, --help\n",
    "    view help menu\n",
    " -r, --repeat u32\n",
    "    repeat decode for benchmarking (1..INT_MAX); default 1\n",
    " -f, --fmt str\n",
    "    output pixel format; default auto. one of\n",
    "    auto, yuv420p, yuva420p,\n",
    "    argb, rgba, bgra, rgb, bgr, Argb, rgbA, bgrA,\n",
    "    rgb565, rgba4444, rgbA4444,\n",
    "    bgr565, bgra4444, bgrA4444\n",
    "    the packed formats convert lossy frames and match the\n",
    "    like-named libwebp colorspace bit-exactly; a lowercase\n",
    "    letter marks the channels alpha is multiplied into, and\n",
    "    the bgr 16-bit ones swap the two bytes of every pixel\n",
    " --muxer str\n",
    "    output muxer (raw, md5, ppm, pam, y4m); default is selected\n",
    "    from a .ppm, .pam or .y4m output extension, or raw\n",
    " --verify md5\n",
    "    verify decoded md5; implies --muxer md5 and no output\n",
    " --cpumask str\n",
    "    restrict the instruction sets used; ",
);

const USAGE_TAIL: &str = concat!(
    "    or a number; default all detected\n",
    " --info\n",
    "    print canvas, animation, the frame table and per-frame\n",
    "    timing to stdout\n",
    " --stream u32\n",
    "    decode incrementally, appending this many bytes at a time,\n",
    "    instead of opening the file whole\n",
    " --subframe\n",
    "    yield each animation sub-frame uncomposited, with its own\n",
    "    dimensions and canvas offset, instead of a finished canvas\n",
    " --loops u32\n",
    "    replay the animation this many times, rewinding between\n",
    "    passes; --stream, which cannot be rewound, reopens instead.\n",
    "    only the first pass is written out. default 1\n",
);

fn usage(app: &str, reason: Option<&str>) {
    if let Some(reason) = reason {
        eprintln!("\n{reason}");
    }
    eprint!("\nusage:  {app} [options] input [output]\n");
    eprint!("{USAGE_HEAD}{CPU_MASK_NAMES},\n{USAGE_TAIL}");
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

fn parse_format(value: &str) -> Option<(Option<&'static str>, Option<Format>)> {
    if value == "auto" {
        return Some((None, None));
    }
    PIXEL_FORMATS
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(name, format)| (Some(*name), Some(*format)))
}

fn parse_cpumask(value: &str) -> Option<u32> {
    if let Some((_, mask)) = cpu_masks().iter().find(|(name, _)| *name == value) {
        return Some(*mask);
    }
    if value.starts_with('-') {
        return None;
    }
    let (digits, radix) = if let Some(rest) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (rest, 16)
    } else if value.len() > 1 && value.starts_with('0') {
        (&value[1..], 8)
    } else {
        (value, 10)
    };

    u32::from_str_radix(digits, radix).ok()
}

fn warn_baseline_cpumask(mask: u32) {
    if cfg!(feature = "trim_dsp") {
        let forced = wpd::cpu::CpuFlags::compile_time().bits() & !mask;

        if forced != 0 {
            eprintln!(
                "warning: cannot disable flags 0x{forced:x} below the build \
                 target; reconfigure with -Dtrim_dsp=false"
            );
        }
    }
}

fn parse_md5(value: &str) -> Option<[u8; 16]> {
    if value.len() != 32 {
        return None;
    }
    let bytes = value.as_bytes();
    let mut digest = [0u8; 16];

    for (i, out) in digest.iter_mut().enumerate() {
        let hi = (bytes[2 * i] as char).to_digit(16)?;
        let lo = (bytes[2 * i + 1] as char).to_digit(16)?;

        *out = (hi * 16 + lo) as u8;
    }
    Some(digest)
}

#[derive(Default)]
struct Options {
    repeat: i32,
    loops: i32,
    stream: usize,
    info: bool,
    subframe: bool,
    muxer: Option<String>,
    verify: Option<String>,
    pixel_format: Option<&'static str>,
    out_format: Option<Format>,
    positional: Vec<OsString>,
}

enum Parsed {
    Ok(Box<Options>),
    Help,
    Bad(&'static str),
}

fn long_option(name: &str) -> Option<&'static str> {
    match name.as_bytes().first() {
        Some(b'h') if "help".starts_with(name) => Some("help"),
        Some(b'r') if "repeat".starts_with(name) => Some("repeat"),
        Some(b'f') if "fmt".starts_with(name) => Some("fmt"),
        Some(b'm') if "muxer".starts_with(name) => Some("muxer"),
        Some(b'v') if "verify".starts_with(name) => Some("verify"),
        Some(b'i') if "info".starts_with(name) => Some("info"),
        Some(b'l') if "loops".starts_with(name) => Some("loops"),
        Some(b'c') if "cpumask".starts_with(name) => Some("cpumask"),
        Some(b's') => {
            match ("subframe".starts_with(name), "stream".starts_with(name)) {
                (true, false) => Some("subframe"),
                (false, true) => Some("stream"),
                _ => None,
            }
        }
        _ => None,
    }
}

fn short_option(opt: char) -> Option<&'static str> {
    match opt {
        'h' => Some("help"),
        'r' => Some("repeat"),
        'f' => Some("fmt"),
        _ => None,
    }
}

fn set_valued(o: &mut Options, name: &str, value: &str) -> Result<(), &'static str> {
    match name {
        "repeat" => o.repeat = parse_repeat(value).ok_or(BAD_REPEAT)?,
        "fmt" => {
            let (pixel_format, out_format) = parse_format(value).ok_or(BAD_FORMAT)?;

            o.pixel_format = pixel_format;
            o.out_format = out_format;
        }
        _ => return Err(MISSING),
    }
    Ok(())
}

#[inline(never)]
fn parse_args(argv: &[OsString]) -> Parsed {
    let mut o = Options {
        repeat: 1,
        loops: 1,
        ..Default::default()
    };
    let mut i = 1;
    let mut only_operands = false;

    while i < argv.len() {
        let arg_os = &argv[i];
        let arg = arg_os.to_string_lossy();

        i += 1;
        if only_operands || arg == "-" || !arg.starts_with('-') {
            o.positional.push(arg_os.clone());
            continue;
        }
        if arg == "--" {
            only_operands = true;
            continue;
        }

        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_owned())),
                None => (long, None),
            };
            let mut value = || match inline.clone() {
                Some(v) => Some(v),
                None => {
                    let v = argv.get(i).map(|v| v.to_string_lossy().into_owned());
                    if v.is_some() {
                        i += 1;
                    }
                    v
                }
            };

            macro_rules! value {
                () => {
                    match value() {
                        Some(v) => v,
                        None => return Parsed::Bad(MISSING),
                    }
                };
            }

            macro_rules! no_value {
                () => {
                    if inline.is_some() {
                        return Parsed::Bad(MISSING);
                    }
                };
            }

            let Some(name) = long_option(name) else {
                return Parsed::Bad(MISSING);
            };

            match name {
                "help" => {
                    no_value!();
                    return Parsed::Help;
                }
                "repeat" | "fmt" => {
                    if let Err(e) = set_valued(&mut o, name, &value!()) {
                        return Parsed::Bad(e);
                    }
                }
                "muxer" => match value!() {
                    v if matches!(
                        v.as_str(),
                        "raw" | "md5" | "ppm" | "pam" | "y4m"
                    ) =>
                    {
                        o.muxer = Some(v)
                    }
                    _ => return Parsed::Bad(BAD_MUXER),
                },
                "verify" => o.verify = Some(value!()),
                "info" => {
                    no_value!();
                    o.info = true;
                }
                "subframe" => {
                    no_value!();
                    o.subframe = true;
                }
                "loops" => match parse_repeat(&value!()) {
                    Some(v) => o.loops = v,
                    None => return Parsed::Bad(BAD_LOOPS),
                },
                "stream" => match parse_repeat(&value!()) {
                    Some(v) => o.stream = v as usize,
                    None => return Parsed::Bad(BAD_STREAM),
                },
                "cpumask" => match parse_cpumask(&value!()) {
                    Some(mask) => {
                        warn_baseline_cpumask(mask);
                        api::set_cpu_flags_mask(mask);
                    }
                    None => return Parsed::Bad(BAD_CPUMASK),
                },
                _ => return Parsed::Bad(MISSING),
            }
            continue;
        }

        let cluster: Vec<char> = arg[1..].chars().collect();
        let mut c = 0;

        while c < cluster.len() {
            let opt = cluster[c];

            c += 1;
            let mut value = |i: &mut usize| {
                if c < cluster.len() {
                    let v: String = cluster[c..].iter().collect();
                    c = cluster.len();
                    Some(v)
                } else {
                    let v = argv.get(*i).map(|v| v.to_string_lossy().into_owned());
                    if v.is_some() {
                        *i += 1;
                    }
                    v
                }
            };

            macro_rules! value {
                () => {
                    match value(&mut i) {
                        Some(v) => v,
                        None => return Parsed::Bad(MISSING),
                    }
                };
            }

            let Some(name) = short_option(opt) else {
                return Parsed::Bad(MISSING);
            };

            if name == "help" {
                return Parsed::Help;
            }
            if let Err(e) = set_valued(&mut o, name, &value!()) {
                return Parsed::Bad(e);
            }
        }
    }
    Parsed::Ok(Box::new(o))
}

const MISSING: &str = "unknown option or missing option value";
const BAD_CPUMASK: &str = "invalid cpu mask";
const BAD_REPEAT: &str = "invalid repeat value; expected 1..INT_MAX";
const BAD_LOOPS: &str = "invalid loop count; expected 1..INT_MAX";
const BAD_STREAM: &str = "invalid stream chunk size; expected 1..INT_MAX";
const BAD_FORMAT: &str = "invalid output pixel format";
const BAD_MUXER: &str = "invalid output muxer; expected raw, md5, ppm, pam or y4m";

fn errmsg(e: &std::io::Error) -> String {
    let text = e.to_string();

    match text.find(" (os error ") {
        Some(cut) => text[..cut].to_owned(),
        None => text,
    }
}

struct DecodeContext<'a> {
    sink: Option<&'a mut Output>,
    pixel_format: Option<&'static str>,
    info: bool,
    frames: i32,
}

fn print_image_info(decoder: &mut Decoder<'_>, printed: &mut bool) {
    let Ok(image) = decoder.info() else {
        return;
    };

    if *printed {
        return;
    }
    *printed = true;
    println!("canvas: {}x{}", image.width, image.height);
    println!("coding: {}", image.coding.name());
    println!("alpha: {}", i32::from(image.has_alpha));
    println!("animation: {}", i32::from(image.is_animation));
    println!("frames: {}", image.frame_count);
    println!("loops: {}", image.loop_count);
    println!("background: 0x{:08x}", image.background_argb);

    for i in 0.. {
        let Ok(entry) = decoder.frame_info(i) else {
            break;
        };

        println!(
            "table {}: {}x{} at {},{} duration {} dispose {} blend {} \
             alpha {} complete {}",
            i,
            entry.width,
            entry.height,
            entry.pos_x,
            entry.pos_y,
            entry.duration,
            i32::from(entry.dispose_to_background),
            i32::from(!entry.blend),
            i32::from(entry.has_alpha),
            i32::from(entry.complete)
        );
    }
}

fn print_metadata(decoder: &mut Decoder<'_>) {
    const KINDS: [(Metadata, &str); 3] = [
        (Metadata::Iccp, "iccp"),
        (Metadata::Exif, "exif"),
        (Metadata::Xmp, "xmp"),
    ];

    for (which, name) in KINDS {
        if let Some(data) = decoder.metadata(which) {
            if !data.is_empty() {
                println!("{name}: {} bytes", data.len());
            }
        }
    }
}

fn drain_frames(decoder: &mut Decoder<'_>, ctx: &mut DecodeContext) -> i32 {
    loop {
        let Ok(next) = decoder.next_frame() else {
            return -1;
        };
        let Some(frame) = next else {
            return 0;
        };

        if ctx.info {
            let (pos_x, pos_y) = frame.position();

            println!(
                "frame {}: {}x{} {} duration {} timestamp {} at {},{} \
                 dispose {} blend {} alpha {}",
                ctx.frames,
                frame.width(),
                frame.height(),
                format_name(frame.format()),
                frame.duration(),
                frame.timestamp(),
                pos_x,
                pos_y,
                i32::from(frame.dispose_to_background()),
                i32::from(!frame.blend()),
                i32::from(frame.has_alpha())
            );
        }
        if let Some(sink) = ctx.sink.as_deref_mut() {
            if let Err(e) = sink.write_frame(&frame, ctx.pixel_format) {
                if e.raw_os_error().is_some() {
                    eprintln!("write: {}", errmsg(&e));
                }
                return -1;
            }
        }
        ctx.frames += 1;
    }
}

fn decode_stream(
    decoder: &mut Decoder<'_>,
    data: &[u8],
    chunk: usize,
    ctx: &mut DecodeContext,
    info_printed: &mut bool,
) -> i32 {
    let mut last_rows = 0;

    if decoder.open_stream().is_err() {
        return -1;
    }
    for part in data.chunks(chunk) {
        if decoder.append(part).is_err() {
            return -1;
        }
        if drain_frames(decoder, ctx) < 0 {
            return -1;
        }
        if ctx.info {
            if let Ok((partial, rows)) = decoder.partial_frame() {
                if rows > 0 && rows != last_rows {
                    println!("partial: {} of {} rows", rows, partial.height());
                    last_rows = rows;
                }
            }
        }
    }
    if decoder.end_of_stream().is_err() {
        return -1;
    }
    if ctx.info {
        print_image_info(decoder, info_printed);
    }
    drain_frames(decoder, ctx)
}

fn new_decoder(
    out_format: Option<Format>,
    pixel_format: Option<&str>,
    subframe: bool,
) -> Option<Decoder<'static>> {
    let mut decoder = Decoder::new();

    if let Some(format) = out_format {
        if decoder.set_format(format).is_err() {
            eprintln!("cannot select {} output", pixel_format.unwrap_or("auto"));
            return None;
        }
    }
    if subframe && decoder.set_animation(Animation::Subframe).is_err() {
        eprintln!("cannot select sub-frame output");
        return None;
    }
    Some(decoder)
}

fn read_file(name: &OsStr) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();

    if name == OsStr::new("-") {
        std::io::stdin().read_to_end(&mut data)?;
    } else {
        std::fs::File::open(name)?.read_to_end(&mut data)?;
    }
    Ok(data)
}

#[cfg(unix)]
fn restore_sigpipe() {
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;

    extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }

    unsafe { signal(SIGPIPE, SIG_DFL) };
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() -> ExitCode {
    restore_sigpipe();

    let argv: Vec<OsString> = std::env::args_os().collect();
    let app = argv
        .first()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "wpd".into());

    print_banner();

    let opts = match parse_args(&argv) {
        Parsed::Ok(o) => o,
        Parsed::Help => {
            usage(&app, None);
            return ExitCode::SUCCESS;
        }
        Parsed::Bad(reason) => {
            let reason = if reason == BAD_CPUMASK {
                format!("invalid cpu mask; expected {CPU_MASK_NAMES}, or a number")
            } else {
                reason.to_owned()
            };

            usage(&app, Some(&reason));
            return ExitCode::from(2);
        }
    };

    if opts.verify.is_some() && opts.muxer.as_deref().is_some_and(|m| m != "md5") {
        usage(&app, Some("verification requires the md5 muxer"));
        return ExitCode::from(2);
    }
    let expected_md5 = match opts.verify.as_deref() {
        Some(v) => match parse_md5(v) {
            Some(d) => Some(d),
            None => {
                usage(
                    &app,
                    Some("invalid md5; expected exactly 32 hexadecimal digits"),
                );
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    let verifying = expected_md5.is_some();
    let operands = opts.positional.len();
    let max = if verifying { 1 } else { 2 };

    if operands < 1 || operands > max || (!verifying && !opts.info && operands != 2) {
        let reason = if verifying {
            if operands < 1 {
                "input is required"
            } else {
                "verification does not accept output"
            }
        } else if operands < 1 {
            "input is required"
        } else {
            "unexpected argument"
        };

        usage(&app, Some(reason));
        return ExitCode::from(2);
    }

    let input_name = &opts.positional[0];
    let output_name = if verifying || operands < 2 {
        None
    } else {
        Some(opts.positional[1].as_os_str())
    };

    run(&opts, input_name, output_name, expected_md5)
}

fn run(
    opts: &Options,
    input_name: &OsStr,
    output_name: Option<&OsStr>,
    expected_md5: Option<[u8; 16]>,
) -> ExitCode {
    let data = match read_file(input_name) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {}", input_name.to_string_lossy(), errmsg(&e));
            return ExitCode::FAILURE;
        }
    };

    let opened = expected_md5.is_some() || output_name.is_some();
    let mut output = if opened {
        let muxer = if expected_md5.is_some() {
            Some("md5")
        } else {
            opts.muxer.as_deref()
        };

        match Output::open(muxer, output_name) {
            Ok(o) => o,
            Err(e) => {
                eprintln!(
                    "{}: {}",
                    output_name.unwrap_or(OsStr::new("")).to_string_lossy(),
                    errmsg(&e)
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        Output::null()
    };

    let mut pixel_format = opts.pixel_format;
    let mut out_format = opts.out_format;

    if opened && output.muxer != Muxer::Raw {
        let Ok(image) = api::info(&data) else {
            eprintln!("{}: cannot read image header", input_name.to_string_lossy());
            return ExitCode::FAILURE;
        };

        if output
            .select_format(&image, &mut pixel_format, &mut out_format)
            .is_err()
        {
            return ExitCode::FAILURE;
        }
    }

    let writes = opened && !output.is_null();
    let mut frames = 0;

    for iter in 0..opts.repeat {
        let mut info_printed = false;
        let mut ctx = DecodeContext {
            sink: if iter == 0 && writes {
                Some(&mut output)
            } else {
                None
            },
            pixel_format,
            info: opts.info && iter == 0,
            frames: 0,
        };

        let Some(mut decoder) = new_decoder(out_format, pixel_format, opts.subframe)
        else {
            return ExitCode::FAILURE;
        };
        let mut ret = 0;

        if opts.stream != 0 {
            for loop_index in 0..opts.loops {
                if loop_index > 0 {
                    let Some(next) =
                        new_decoder(out_format, pixel_format, opts.subframe)
                    else {
                        return ExitCode::FAILURE;
                    };

                    decoder = next;
                    ctx.sink = None;
                    ctx.frames = 0;
                }
                ret = decode_stream(
                    &mut decoder,
                    &data,
                    opts.stream,
                    &mut ctx,
                    &mut info_printed,
                );
                if ret < 0 {
                    break;
                }
            }
        } else if decoder.open(&data).is_err() {
            ret = -1;
        } else {
            if ctx.info {
                print_image_info(&mut decoder, &mut info_printed);
            }
            ret = drain_frames(&mut decoder, &mut ctx);

            let mut loop_index = 1;

            while loop_index < opts.loops && ret >= 0 {
                ctx.sink = None;
                ctx.frames = 0;
                ret = if decoder.rewind().is_err() { -1 } else { 0 };
                if ret >= 0 {
                    ret = drain_frames(&mut decoder, &mut ctx);
                }
                loop_index += 1;
            }
        }
        if ctx.info && ret >= 0 {
            print_metadata(&mut decoder);
        }
        frames = ctx.frames;
        if ret < 0 {
            eprintln!("{}: {}", input_name.to_string_lossy(), decoder.error());
            return ExitCode::FAILURE;
        }
    }

    if frames == 0 {
        eprintln!("{}: no image data found", input_name.to_string_lossy());
        return ExitCode::FAILURE;
    }
    if let Some(expected) = expected_md5 {
        return if output.verify(&expected) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if !opened {
        return ExitCode::SUCCESS;
    }
    match output.close() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "write: {}", errmsg(&e));
            ExitCode::FAILURE
        }
    }
}
