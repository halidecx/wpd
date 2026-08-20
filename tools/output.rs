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
    md5: Md5,
    frames: i32,
    width: i32,
    height: i32,
    pub has_alpha: bool,
    format: Format,
    yuvdsp: YuvDsp,
}

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
    pub fn open(muxer: Option<&str>, filename: Option<&OsStr>) -> io::Result<Self> {
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
            Kind::File => self.file.as_mut().unwrap().write_all(data),
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

    fn write_argb_444(&mut self, frame: &Picture<'_>) -> io::Result<()> {
        let width = frame.width() as usize;
        let pixels = width * frame.height() as usize;
        let mut y = vec![0u8; pixels];
        let mut u = vec![0u8; pixels];
        let mut v = vec![0u8; pixels];

        for row in 0..frame.height() as usize {
            let at = row * width;
            let [y, u, v] = [&mut y, &mut u, &mut v].map(|p| &mut p[at..at + width]);

            (self.yuvdsp.argb_to_yuv444)(y, u, v, frame.row(0, row as i32));
        }
        self.write(&y)?;
        self.write(&u)?;
        self.write(&v)
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
}
