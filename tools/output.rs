use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, BufWriter, Write};

use wpd::api::{Coding, ImageInfo, Picture};
use wpd::dsp::yuv::{extract_alpha, YuvDsp};
use wpd::image::Format;

use crate::md5::{hex, Md5};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Muxer {
    Raw,
    Ppm,
    Pam,
    Y4m,
}

impl Muxer {
    fn name(self) -> &'static str {
        match self {
            Muxer::Raw => "raw",
            Muxer::Ppm => "ppm",
            Muxer::Pam => "pam",
            Muxer::Y4m => "y4m",
        }
    }

    fn required(self) -> Option<(&'static str, Format)> {
        match self {
            Muxer::Ppm => Some(("rgb", Format::Rgb)),
            Muxer::Pam => Some(("rgba", Format::Rgba)),
            Muxer::Raw | Muxer::Y4m => None,
        }
    }
}

#[derive(PartialEq, Eq)]
enum Kind {
    File,
    Md5,
    Null,
}

pub struct Output {
    kind: Kind,
    pub muxer: Muxer,
    file: Option<Box<dyn Write>>,
    /* Bytes written to a file or stdout so far, and the most a decode may
     * write in total; 0 lifts the limit. A hashed or discarded decode costs
     * nothing downstream, so only Kind::File is budgeted. */
    written: u64,
    limit: u64,
    y4m_stash: usize,
    md5: Md5,
    frames: i32,
    width: i32,
    height: i32,
    pub has_alpha: bool,
    format: Format,
    yuvdsp: YuvDsp,
}

/* Bytes of U and V an ARGB frame may stash while its Y4M luma is written:
 * both chroma planes of 8K UHD, 7680x4320, the largest geometry video
 * tooling that consumes Y4M routinely handles, so every such frame
 * converts once. Beyond it memory stays at this bound and the rows past the
 * stash cost a second or third conversion, which measured 6% of the wall
 * time of a 4096^2 lossless decode to Y4M when paid for every row. */
const Y4M_STASH: usize = 2 * 7680 * 4320;

pub const PIXEL_FORMATS: &[(&str, Format)] = &[
    ("yuv420p", Format::Yuv420p),
    ("yuva420p", Format::Yuva420p),
    ("argb", Format::Argb),
    ("rgba", Format::Rgba),
    ("bgra", Format::Bgra),
    ("rgb", Format::Rgb),
    ("bgr", Format::Bgr),
    ("Argb", Format::ArgbPre),
    ("rgbA", Format::RgbaPre),
    ("bgrA", Format::BgraPre),
    ("rgb565", Format::Rgb565),
    ("rgba4444", Format::Rgba4444),
    ("rgbA4444", Format::Rgba4444Pre),
    ("bgr565", Format::Bgr565),
    ("bgra4444", Format::Bgra4444),
    ("bgrA4444", Format::Bgra4444Pre),
];

pub fn format_name(format: Format) -> &'static str {
    PIXEL_FORMATS
        .iter()
        .find(|(_, f)| *f == format)
        .map_or("unknown", |(name, _)| name)
}

fn extension(filename: &str) -> Option<&str> {
    let start = filename.rfind(['/', '\\']).map_or(0, |i| i + 1);

    filename[start..]
        .rfind('.')
        .map(|i| &filename[start + i + 1..])
}

impl Output {
    pub fn open(
        muxer: Option<&str>,
        filename: Option<&OsStr>,
        limit: u64,
    ) -> io::Result<Self> {
        let chosen = match muxer {
            Some(m) => m.to_owned(),
            None => filename
                .map(OsStr::to_string_lossy)
                .as_deref()
                .and_then(extension)
                .filter(|e| matches!(*e, "ppm" | "pam" | "y4m"))
                .unwrap_or("raw")
                .to_owned(),
        };
        let mut out = Self::null();

        out.limit = limit;
        if chosen == "md5" {
            out.kind = Kind::Md5;
            if filename.is_none() {
                return Ok(out);
            }
        } else {
            let name = filename.unwrap_or(OsStr::new(""));

            out.kind = if name == OsStr::new("/dev/null") {
                Kind::Null
            } else {
                Kind::File
            };
            out.muxer = match chosen.as_str() {
                "ppm" => Muxer::Ppm,
                "pam" => Muxer::Pam,
                "y4m" => Muxer::Y4m,
                _ => Muxer::Raw,
            };
            if out.kind == Kind::Null {
                return Ok(out);
            }
        }

        let name = filename.unwrap_or(OsStr::new(""));

        out.file = Some(if name == OsStr::new("-") {
            Box::new(io::stdout())
        } else {
            Box::new(BufWriter::new(File::create(name)?))
        });
        Ok(out)
    }

    pub fn null() -> Self {
        Self {
            kind: Kind::Null,
            muxer: Muxer::Raw,
            file: None,
            written: 0,
            limit: 0,
            y4m_stash: Y4M_STASH,
            md5: Md5::new(),
            frames: 0,
            width: 0,
            height: 0,
            has_alpha: false,
            format: Format::Argb,
            yuvdsp: YuvDsp::new(),
        }
    }

    pub fn is_null(&self) -> bool {
        self.kind == Kind::Null
    }

    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        match self.kind {
            Kind::Md5 => {
                self.md5.update(data);
                Ok(())
            }
            Kind::File => {
                if self.limit != 0 && data.len() as u64 > self.limit - self.written {
                    eprintln!(
                        "output would exceed the {} byte limit (--max-output)",
                        self.limit
                    );
                    return Err(io::Error::other("output limit exceeded"));
                }
                self.written += data.len() as u64;
                self.file.as_mut().unwrap().write_all(data)
            }
            Kind::Null => Ok(()),
        }
    }

    pub fn close(mut self) -> io::Result<()> {
        if self.kind == Kind::Md5 {
            let digest = hex(&std::mem::take(&mut self.md5).finish());

            if let Some(f) = self.file.as_mut() {
                writeln!(f, "{digest}")?;
            }
        }
        if let Some(mut f) = self.file.take() {
            f.flush()?;
        }
        Ok(())
    }

    pub fn verify(self, expected: &[u8; 16]) -> bool {
        self.md5.finish() == *expected
    }

    pub fn select_format(
        &mut self,
        info: &ImageInfo,
        pixel_format: &mut Option<&'static str>,
        format: &mut Option<Format>,
    ) -> Result<(), ()> {
        let (required_name, required) = match (self.muxer.required(), self.muxer) {
            (Some(one), _) => one,
            (None, Muxer::Y4m) => {
                if info.coding == Coding::Lossless && format.is_none() {
                    self.has_alpha = info.has_alpha;
                    ("argb", Format::Argb)
                } else if matches!(
                    *format,
                    Some(Format::Yuv420p) | Some(Format::Yuva420p)
                ) {
                    return Ok(());
                } else if format.is_some() {
                    eprintln!("y4m requires yuv420p or yuva420p output");
                    return Err(());
                } else if info.has_alpha {
                    ("yuva420p", Format::Yuva420p)
                } else {
                    ("yuv420p", Format::Yuv420p)
                }
            }
            (None, _) => return Ok(()),
        };

        if format.is_some_and(|f| f != required) {
            eprintln!("{} requires {required_name} output", self.muxer.name());
            return Err(());
        }
        *pixel_format = Some(required_name);
        *format = Some(required);
        Ok(())
    }

    fn write_plane(&mut self, frame: &Picture<'_>, plane: usize) -> io::Result<()> {
        for y in 0..frame.rows(plane) {
            let row = frame.row(plane, y);

            self.write(row)?;
        }
        Ok(())
    }

    fn write_chroma_444(
        &mut self,
        frame: &Picture<'_>,
        plane: usize,
    ) -> io::Result<()> {
        let mut row = vec![0u8; frame.width() as usize];

        for y in 0..frame.height() {
            let src = frame.row(plane, y / 2);

            for (x, o) in row.iter_mut().enumerate() {
                *o = src[x / 2];
            }
            self.write(&row)?;
        }
        Ok(())
    }

    /* Y4M wants each plane whole but a conversion yields a row of all three,
     * so the U and V of the first rows are stashed, within a fixed budget,
     * while Y is written. Rows past the stash are converted again for the
     * plane that needs them, and the U stash, once written, is refilled
     * with the V rows the U pass converts anyway. A frame of up to
     * stash / 2 pixels converts once, up to stash pixels twice for the
     * rows past the stash, and anything larger three times for the rest. */
    fn write_argb_444(&mut self, frame: &Picture<'_>) -> io::Result<()> {
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let stashed = height.min(self.y4m_stash / (2 * width));
        let mut y = vec![0u8; width];
        let mut u = vec![0u8; width];
        let mut v = vec![0u8; width];
        let mut us = vec![0u8; stashed * width];
        let mut vs = vec![0u8; stashed * width];
        let convert = self.yuvdsp.argb_to_yuv444;
        let argb = |row: usize| frame.row(0, row as i32);

        for (row, (us, vs)) in us
            .chunks_exact_mut(width)
            .zip(vs.chunks_exact_mut(width))
            .enumerate()
        {
            convert(&mut y, us, vs, argb(row));
            self.write(&y)?;
        }
        for row in stashed..height {
            convert(&mut y, &mut u, &mut v, argb(row));
            self.write(&y)?;
        }
        self.write(&us)?;

        let refilled = stashed.min(height - stashed);

        for (row, vs) in (stashed..).zip(us[..refilled * width].chunks_exact_mut(width))
        {
            convert(&mut y, &mut u, vs, argb(row));
            self.write(&u)?;
        }
        for row in stashed + refilled..height {
            convert(&mut y, &mut u, &mut v, argb(row));
            self.write(&u)?;
        }
        self.write(&vs)?;
        self.write(&us[..refilled * width])?;
        for row in stashed + refilled..height {
            convert(&mut y, &mut u, &mut v, argb(row));
            self.write(&v)?;
        }
        Ok(())
    }

    fn write_argb_alpha(&mut self, frame: &Picture<'_>) -> io::Result<()> {
        let mut row = vec![0u8; frame.width() as usize];

        for y in 0..frame.height() {
            extract_alpha(&mut row, frame.row(0, y));
            self.write(&row)?;
        }
        Ok(())
    }

    pub fn write_frame(
        &mut self,
        frame: &Picture<'_>,
        pixel_format: Option<&str>,
    ) -> io::Result<()> {
        let pixel_format = pixel_format.unwrap_or_else(|| format_name(frame.format()));
        let fail = |msg: String| io::Error::other(msg);

        match (self.muxer.required(), self.muxer) {
            (Some((required_name, required)), _) => {
                if frame.format() != required {
                    eprintln!("{} requires {required_name} output", self.muxer.name());
                    return Err(fail("wrong format".into()));
                }
                let header = if self.muxer == Muxer::Ppm {
                    format!("P6\n{} {}\n255\n", frame.width(), frame.height())
                } else {
                    format!(
                        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\n\
                         TUPLTYPE RGB_ALPHA\nENDHDR\n",
                        frame.width(),
                        frame.height()
                    )
                };

                self.write(header.as_bytes())?;
                self.write_plane(frame, 0)
            }
            (None, Muxer::Y4m) => self.write_y4m(frame),
            (None, _) => self.write_raw(frame, pixel_format),
        }
    }

    fn write_y4m(&mut self, frame: &Picture<'_>) -> io::Result<()> {
        let format = frame.format();

        if !matches!(format, Format::Yuv420p | Format::Yuva420p | Format::Argb) {
            eprintln!("y4m requires yuv420p, yuva420p or argb output");
            return Err(io::Error::other("wrong format"));
        }
        if self.frames == 0 {
            self.width = frame.width();
            self.height = frame.height();
            self.format = format;

            let colour = if format == Format::Yuva420p
                || (format == Format::Argb && self.has_alpha)
            {
                "444alpha"
            } else if format == Format::Argb {
                "444"
            } else {
                "420jpeg"
            };
            let header = format!(
                "YUV4MPEG2 W{} H{} F0:0 Ip A0:0 C{colour}\n",
                frame.width(),
                frame.height()
            );

            self.write(header.as_bytes())?;
        } else if frame.width() != self.width
            || frame.height() != self.height
            || format != self.format
        {
            eprintln!("y4m frames must have one size and format");
            return Err(io::Error::other("size or format changed"));
        }
        self.frames += 1;
        self.write(b"FRAME\n")?;

        if format == Format::Argb {
            self.write_argb_444(frame)?;
            if self.has_alpha {
                self.write_argb_alpha(frame)?;
            }
            return Ok(());
        }

        self.write_plane(frame, 0)?;

        if format == Format::Yuva420p {
            self.write_chroma_444(frame, 1)?;
            self.write_chroma_444(frame, 2)?;
            self.write_plane(frame, 3)
        } else {
            self.write_plane(frame, 1)?;
            self.write_plane(frame, 2)
        }
    }

    fn write_raw(&mut self, frame: &Picture<'_>, pixel_format: &str) -> io::Result<()> {
        let format = frame.format();

        if format.is_packed() {
            if pixel_format != format_name(format) {
                eprintln!(
                    "cannot convert {} frame to {}",
                    format_name(format),
                    pixel_format
                );
                return Err(io::Error::other("wrong format"));
            }
            return self.write_plane(frame, 0);
        }

        let planes = match pixel_format {
            "yuv420p" => 3,
            "yuva420p" => 4,
            _ => {
                eprintln!(
                    "cannot convert {} frame to {}",
                    format_name(format),
                    pixel_format
                );
                return Err(io::Error::other("wrong format"));
            }
        };

        if planes == 4 && format != Format::Yuva420p {
            eprintln!("frame has no alpha plane");
            return Err(io::Error::other("no alpha plane"));
        }
        for p in 0..planes {
            self.write_plane(frame, p)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extension_after_the_last_separator_wins() {
        assert_eq!(extension("out.ppm"), Some("ppm"));
        assert_eq!(extension("dir.ppm/out"), None);
        assert_eq!(extension("dir.ppm/out.y4m"), Some("y4m"));
        assert_eq!(extension("out"), None);
        assert_eq!(extension("a\\b.pam"), Some("pam"));
    }

    fn sink(limit: u64) -> Output {
        let mut out = Output::null();

        out.kind = Kind::File;
        out.file = Some(Box::new(io::sink()));
        out.limit = limit;
        out
    }

    #[test]
    fn a_file_sink_refuses_to_pass_its_byte_budget() {
        let mut out = sink(10);

        out.write(&[0; 4]).unwrap();
        out.write(&[0; 6]).unwrap();
        assert_eq!(out.written, 10);

        let err = out.write(&[0; 1]).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(out.written, 10, "a refused write is not counted");
    }

    #[test]
    fn a_zero_budget_is_unlimited_and_hashing_is_free() {
        let mut out = sink(0);

        out.write(&[0; 1 << 12]).unwrap();
        assert_eq!(out.written, 1 << 12);

        let mut out = Output::null();

        out.kind = Kind::Md5;
        out.limit = 1;
        out.write(&[0; 64]).unwrap();
        assert_eq!(out.written, 0);
    }

    /* Whether the stash holds every row, some, half or none decides only how
     * often a row is converted, never the y4m bytes. */
    fn y4m_md5(bytes: &[u8], stashed: fn(i32) -> i32) -> [u8; 16] {
        let mut decoder = wpd::api::Decoder::new();
        let mut out = Output::null();

        out.kind = Kind::Md5;
        out.muxer = Muxer::Y4m;
        decoder.set_format(Format::Argb).unwrap();
        decoder.open(bytes).unwrap();
        out.has_alpha = decoder.info().unwrap().has_alpha;
        while let Some(frame) = decoder.next_frame().unwrap() {
            out.y4m_stash =
                2 * frame.width() as usize * stashed(frame.height()) as usize;
            out.write_frame(&frame, None).unwrap();
        }
        out.md5.finish()
    }

    #[test]
    fn y4m_argb_bytes_do_not_depend_on_the_stash() {
        if cfg!(miri) {
            return;
        }

        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wpd-test-data");
        let mut files = 0;

        for entry in std::fs::read_dir(dir).expect("wpd-test-data is missing") {
            let path = entry.unwrap().path();

            if path.extension().is_none_or(|e| e != "webp") {
                continue;
            }

            let bytes = std::fs::read(&path).unwrap();
            let whole = y4m_md5(&bytes, |h| h);
            let cases: [fn(i32) -> i32; 6] =
                [|_| 0, |_| 1, |h| h / 3, |h| h / 2, |h| h / 2 + 1, |h| h - 1];

            for stashed in cases {
                assert_eq!(y4m_md5(&bytes, stashed), whole, "{}", path.display());
            }
            files += 1;
        }
        assert!(files > 0, "wpd-test-data contains no WebP files");
    }
}
