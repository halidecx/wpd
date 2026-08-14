//! The muxers: raw planes, md5, ppm, pam and y4m.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::mem::MaybeUninit;
use std::slice;

use wpd_capi::dsp::yuv::{wpd_argb_to_yuv444, wpd_yuv_dsp_init, WPDYUVDSP};

use crate::md5::{hex, Md5};
use crate::sys::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Muxer {
    Raw,
    Ppm,
    Pam,
    Y4m,
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
    file: Option<BufWriter<Box<dyn Write>>>,
    md5: Md5,
    frames: i32,
    width: i32,
    height: i32,
    pub has_alpha: bool,
    format: WPDPixelFormat,
    yuvdsp: Box<WPDYUVDSP>,
}

pub const PIXEL_FORMATS: &[(&str, WPDPixelFormat)] = &[
    ("yuv420p", WPD_PIX_FMT_YUV420P),
    ("yuva420p", WPD_PIX_FMT_YUVA420P),
    ("argb", WPD_PIX_FMT_ARGB),
    ("rgba", WPD_PIX_FMT_RGBA),
    ("bgra", WPD_PIX_FMT_BGRA),
    ("rgb", WPD_PIX_FMT_RGB),
    ("bgr", WPD_PIX_FMT_BGR),
    ("Argb", WPD_PIX_FMT_ARGB_PRE),
    ("rgbA", WPD_PIX_FMT_RGBA_PRE),
    ("bgrA", WPD_PIX_FMT_BGRA_PRE),
    ("rgb565", WPD_PIX_FMT_RGB565),
    ("rgba4444", WPD_PIX_FMT_RGBA4444),
    ("rgbA4444", WPD_PIX_FMT_RGBA4444_PRE),
    ("bgr565", WPD_PIX_FMT_BGR565),
    ("bgra4444", WPD_PIX_FMT_BGRA4444),
    ("bgrA4444", WPD_PIX_FMT_BGRA4444_PRE),
];

pub fn format_name(format: WPDPixelFormat) -> &'static str {
    PIXEL_FORMATS
        .iter()
        .find(|(_, f)| *f == format)
        .map_or("unknown", |(name, _)| name)
}

/// The extension of `filename`, if it has one after the last path separator.
fn extension(filename: &str) -> Option<&str> {
    let start = filename.rfind(['/', '\\']).map_or(0, |i| i + 1);

    filename[start..]
        .rfind('.')
        .map(|i| &filename[start + i + 1..])
}

fn new_yuvdsp() -> Box<WPDYUVDSP> {
    let mut dsp = Box::new(MaybeUninit::<WPDYUVDSP>::uninit());

    unsafe {
        wpd_yuv_dsp_init(dsp.as_mut_ptr());
        Box::from_raw(Box::into_raw(dsp).cast::<WPDYUVDSP>())
    }
}

impl Output {
    /// Mirrors `output_open`: a null `filename` is only valid with the md5
    /// muxer, which is how `--verify` runs with no output at all.
    pub fn open(muxer: Option<&str>, filename: Option<&str>) -> io::Result<Self> {
        let chosen = match muxer {
            Some(m) => m.to_owned(),
            None => filename
                .and_then(extension)
                .filter(|e| matches!(*e, "ppm" | "pam" | "y4m"))
                .unwrap_or("raw")
                .to_owned(),
        };
        let mut out = Self {
            kind: Kind::Null,
            muxer: Muxer::Raw,
            file: None,
            md5: Md5::new(),
            frames: 0,
            width: 0,
            height: 0,
            has_alpha: false,
            format: WPD_PIX_FMT_NONE,
            yuvdsp: new_yuvdsp(),
        };

        if chosen == "md5" {
            out.kind = Kind::Md5;
            if filename.is_none() {
                return Ok(out);
            }
        } else {
            let name = filename.unwrap_or("");

            out.kind = if name == "/dev/null" {
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

        let name = filename.unwrap_or("");
        let sink: Box<dyn Write> = if name == "-" {
            Box::new(io::stdout())
        } else {
            Box::new(File::create(name)?)
        };

        out.file = Some(BufWriter::new(sink));
        Ok(out)
    }

    /// The sink `--verify` uses: an md5 accumulator with nowhere to print it.
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
            format: WPD_PIX_FMT_NONE,
            yuvdsp: new_yuvdsp(),
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

    /// Mirrors `output_select_format`: the structured muxers each demand one
    /// pixel format, and disagreeing with an explicit `-f` is an error.
    pub fn select_format(
        &mut self,
        info: &WPDImageInfo,
        pixel_format: &mut Option<&'static str>,
        format: &mut WPDPixelFormat,
    ) -> Result<(), ()> {
        let (required_name, required) = match self.muxer {
            Muxer::Ppm => ("rgb", WPD_PIX_FMT_RGB),
            Muxer::Pam => ("rgba", WPD_PIX_FMT_RGBA),
            Muxer::Y4m => {
                if info.coding == WPD_CODING_LOSSLESS && *format == WPD_PIX_FMT_NONE {
                    self.has_alpha = info.has_alpha != 0;
                    ("argb", WPD_PIX_FMT_ARGB)
                } else if *format == WPD_PIX_FMT_YUV420P
                    || *format == WPD_PIX_FMT_YUVA420P
                {
                    return Ok(());
                } else if *format != WPD_PIX_FMT_NONE {
                    eprintln!("y4m requires yuv420p or yuva420p output");
                    return Err(());
                } else if info.has_alpha != 0 {
                    ("yuva420p", WPD_PIX_FMT_YUVA420P)
                } else {
                    ("yuv420p", WPD_PIX_FMT_YUV420P)
                }
            }
            Muxer::Raw => return Ok(()),
        };

        if *format != WPD_PIX_FMT_NONE && *format != required {
            eprintln!(
                "{} requires {} output",
                if self.muxer == Muxer::Ppm {
                    "ppm"
                } else {
                    "pam"
                },
                required_name
            );
            return Err(());
        }
        *pixel_format = Some(required_name);
        *format = required;
        Ok(())
    }

    fn write_plane(
        &mut self,
        data: *const u8,
        stride: isize,
        width: i32,
        height: i32,
    ) -> io::Result<()> {
        for y in 0..height as isize {
            let row = unsafe {
                slice::from_raw_parts(data.offset(y * stride), width as usize)
            };

            self.write(row)?;
        }
        Ok(())
    }

    /// Doubles a 4:2:0 chroma plane up to full resolution, which is what y4m's
    /// 444alpha layout wants.
    fn write_chroma_444(
        &mut self,
        data: *const u8,
        stride: isize,
        width: i32,
        height: i32,
    ) -> io::Result<()> {
        let mut row = vec![0u8; width as usize];

        for y in 0..height {
            let src = unsafe {
                slice::from_raw_parts(
                    data.offset((y / 2) as isize * stride),
                    (width as usize).div_ceil(2),
                )
            };

            for (x, o) in row.iter_mut().enumerate() {
                *o = src[x / 2];
            }
            self.write(&row)?;
        }
        Ok(())
    }

    fn write_argb_444(&mut self, frame: &WPDFrame) -> io::Result<()> {
        let pixels = frame.width as usize * frame.height as usize;
        let mut y = vec![0u8; pixels];
        let mut u = vec![0u8; pixels];
        let mut v = vec![0u8; pixels];

        unsafe {
            wpd_argb_to_yuv444(
                &*self.yuvdsp,
                y.as_mut_ptr(),
                frame.width as isize,
                u.as_mut_ptr(),
                v.as_mut_ptr(),
                frame.width as isize,
                frame.data[0],
                frame.stride[0],
                frame.width,
                frame.height,
            )
        };
        self.write(&y)?;
        self.write(&u)?;
        self.write(&v)
    }

    fn write_argb_alpha(&mut self, frame: &WPDFrame) -> io::Result<()> {
        let mut row = vec![0u8; frame.width as usize];

        for y in 0..frame.height as isize {
            let src = unsafe {
                slice::from_raw_parts(
                    frame.data[0].offset(y * frame.stride[0]),
                    4 * frame.width as usize,
                )
            };

            for (x, o) in row.iter_mut().enumerate() {
                *o = src[4 * x];
            }
            self.write(&row)?;
        }
        Ok(())
    }

    pub fn write_frame(
        &mut self,
        frame: &WPDFrame,
        pixel_format: Option<&str>,
    ) -> Result<(), ()> {
        let pixel_format = pixel_format.unwrap_or_else(|| format_name(frame.format));

        self.write_frame_inner(frame, pixel_format).map_err(|_| ())
    }

    fn write_frame_inner(
        &mut self,
        frame: &WPDFrame,
        pixel_format: &str,
    ) -> io::Result<()> {
        let fail = |msg: String| io::Error::other(msg);

        match self.muxer {
            Muxer::Ppm | Muxer::Pam => {
                let ppm = self.muxer == Muxer::Ppm;
                let required = if ppm {
                    WPD_PIX_FMT_RGB
                } else {
                    WPD_PIX_FMT_RGBA
                };

                if frame.format != required {
                    eprintln!(
                        "{} requires {} output",
                        if ppm { "ppm" } else { "pam" },
                        if ppm { "rgb" } else { "rgba" }
                    );
                    return Err(fail("wrong format".into()));
                }
                let header = if ppm {
                    format!("P6\n{} {}\n255\n", frame.width, frame.height)
                } else {
                    format!(
                        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\n\
                         TUPLTYPE RGB_ALPHA\nENDHDR\n",
                        frame.width, frame.height
                    )
                };

                self.write(header.as_bytes())?;
                self.write_plane(
                    frame.data[0],
                    frame.stride[0],
                    frame.width * if ppm { 3 } else { 4 },
                    frame.height,
                )
            }
            Muxer::Y4m => self.write_y4m(frame),
            Muxer::Raw => self.write_raw(frame, pixel_format),
        }
    }

    fn write_y4m(&mut self, frame: &WPDFrame) -> io::Result<()> {
        if frame.format != WPD_PIX_FMT_YUV420P
            && frame.format != WPD_PIX_FMT_YUVA420P
            && frame.format != WPD_PIX_FMT_ARGB
        {
            eprintln!("y4m requires yuv420p, yuva420p or argb output");
            return Err(io::Error::other("wrong format"));
        }
        if self.frames == 0 {
            self.width = frame.width;
            self.height = frame.height;
            self.format = frame.format;

            let colour = if frame.format == WPD_PIX_FMT_YUVA420P
                || (frame.format == WPD_PIX_FMT_ARGB && self.has_alpha)
            {
                "444alpha"
            } else if frame.format == WPD_PIX_FMT_ARGB {
                "444"
            } else {
                "420jpeg"
            };
            let header = format!(
                "YUV4MPEG2 W{} H{} F0:0 Ip A0:0 C{colour}\n",
                frame.width, frame.height
            );

            self.write(header.as_bytes())?;
        } else if frame.width != self.width
            || frame.height != self.height
            || frame.format != self.format
        {
            eprintln!("y4m frames must have one size and format");
            return Err(io::Error::other("size or format changed"));
        }
        self.frames += 1;
        self.write(b"FRAME\n")?;

        if frame.format == WPD_PIX_FMT_ARGB {
            self.write_argb_444(frame)?;
            if self.has_alpha {
                self.write_argb_alpha(frame)?;
            }
            return Ok(());
        }

        self.write_plane(frame.data[0], frame.stride[0], frame.width, frame.height)?;

        if frame.format == WPD_PIX_FMT_YUVA420P {
            self.write_chroma_444(
                frame.data[1],
                frame.stride[1],
                frame.width,
                frame.height,
            )?;
            self.write_chroma_444(
                frame.data[2],
                frame.stride[2],
                frame.width,
                frame.height,
            )?;
            self.write_plane(frame.data[3], frame.stride[3], frame.width, frame.height)
        } else {
            let cw = (frame.width + 1) / 2;
            let ch = (frame.height + 1) / 2;

            self.write_plane(frame.data[1], frame.stride[1], cw, ch)?;
            self.write_plane(frame.data[2], frame.stride[2], cw, ch)
        }
    }

    fn write_raw(&mut self, frame: &WPDFrame, pixel_format: &str) -> io::Result<()> {
        if frame.format >= WPD_PIX_FMT_ARGB {
            let bpp = match frame.format {
                WPD_PIX_FMT_RGB | WPD_PIX_FMT_BGR => 3,
                WPD_PIX_FMT_RGB565
                | WPD_PIX_FMT_RGBA4444
                | WPD_PIX_FMT_RGBA4444_PRE
                | WPD_PIX_FMT_BGR565
                | WPD_PIX_FMT_BGRA4444
                | WPD_PIX_FMT_BGRA4444_PRE => 2,
                _ => 4,
            };

            if pixel_format != format_name(frame.format) {
                eprintln!(
                    "cannot convert {} frame to {}",
                    format_name(frame.format),
                    pixel_format
                );
                return Err(io::Error::other("wrong format"));
            }
            return self.write_plane(
                frame.data[0],
                frame.stride[0],
                frame.width * bpp,
                frame.height,
            );
        }

        let planes = match pixel_format {
            "yuv420p" => 3,
            "yuva420p" => 4,
            _ => {
                eprintln!(
                    "cannot convert {} frame to {}",
                    format_name(frame.format),
                    pixel_format
                );
                return Err(io::Error::other("wrong format"));
            }
        };

        if planes == 4 && frame.format != WPD_PIX_FMT_YUVA420P {
            eprintln!("frame has no alpha plane");
            return Err(io::Error::other("no alpha plane"));
        }
        for p in 0..planes {
            let chroma = p == 1 || p == 2;
            let width = if chroma {
                (frame.width + 1) / 2
            } else {
                frame.width
            };
            let height = if chroma {
                (frame.height + 1) / 2
            } else {
                frame.height
            };

            self.write_plane(frame.data[p], frame.stride[p], width, height)?;
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
