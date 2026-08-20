use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::driver;
use crate::handout::Handout;
use crate::image::Format;
use crate::picture::Frame;

pub use crate::container::Coding;
pub use crate::error::{Error, Result};
pub use crate::info::{FrameInfo, ImageInfo};
pub use crate::options::Options;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animation {
    #[default]
    Composited,
    Subframe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata {
    Iccp,
    Exif,
    Xmp,
}

pub struct Picture<'a> {
    out: Handout<'a>,
}

impl<'a> Picture<'a> {
    fn pixels(&self) -> Option<&Frame<'a>> {
        self.out.frame()
    }

    pub fn width(&self) -> i32 {
        self.out.width
    }

    pub fn height(&self) -> i32 {
        self.out.height
    }

    pub fn format(&self) -> Format {
        self.out.format
    }

    pub fn duration(&self) -> i32 {
        self.out.duration
    }

    pub fn timestamp(&self) -> i64 {
        self.out.timestamp
    }

    pub fn position(&self) -> (i32, i32) {
        (self.out.pos_x, self.out.pos_y)
    }

    pub fn has_alpha(&self) -> bool {
        self.out.has_alpha
    }

    pub fn dispose_to_background(&self) -> bool {
        self.out.dispose_to_background
    }

    pub fn blend(&self) -> bool {
        !self.out.no_blend
    }

    pub fn planes(&self) -> usize {
        self.out.planes()
    }

    pub fn rows(&self, plane: usize) -> i32 {
        self.pixels().map_or(0, |img| img.rows(plane))
    }

    pub fn row(&self, plane: usize, y: i32) -> &'a [u8] {
        assert!(plane < self.planes(), "no such plane");

        let img = self.pixels().expect("no pixels");

        assert!(y >= 0 && y < img.rows(plane), "no such row");
        img.row(plane, y)
    }

    pub fn rows_of(&self, plane: usize) -> impl Iterator<Item = &'a [u8]> + '_ {
        (0..self.rows(plane)).map(move |y| self.row(plane, y))
    }
}

pub struct Decoder<'a> {
    inner: Box<driver::Decoder<'a>>,
    failed: Option<String>,
    input: PhantomData<&'a [u8]>,
}

pub struct UpdatedDecoder<'d, 'a> {
    decoder: &'d mut Decoder<'a>,
}

impl<'a> Deref for UpdatedDecoder<'_, 'a> {
    type Target = Decoder<'a>;

    fn deref(&self) -> &Self::Target {
        self.decoder
    }
}

impl DerefMut for UpdatedDecoder<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.decoder
    }
}

impl<'d, 'a> UpdatedDecoder<'d, 'a> {
    pub fn into_buffer(self) -> Result<UpdateBuffer<'d, 'a>> {
        let data = self.decoder.driver().take_update_buffer()?;

        Ok(UpdateBuffer {
            decoder: Some(self.decoder),
            data: Some(data),
        })
    }
}

pub struct UpdateBuffer<'d, 'a> {
    decoder: Option<&'d mut Decoder<'a>>,
    data: Option<Vec<u8>>,
}

impl Deref for UpdateBuffer<'_, '_> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        self.data.as_ref().expect("update buffer is attached")
    }
}

impl DerefMut for UpdateBuffer<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.as_mut().expect("update buffer is attached")
    }
}

impl<'d, 'a> UpdateBuffer<'d, 'a> {
    pub fn update(mut self) -> Result<UpdatedDecoder<'d, 'a>> {
        let data = self.data.take().expect("update buffer is attached");
        let decoder = self.decoder.take().expect("update decoder is attached");

        if let Err(e) = decoder.driver().update_owned(data) {
            decoder.driver().open_stream()?;
            return Err(e);
        }
        Ok(UpdatedDecoder { decoder })
    }
}

impl Drop for UpdateBuffer<'_, '_> {
    fn drop(&mut self) {
        let Some(data) = self.data.take() else {
            return;
        };
        let Some(decoder) = self.decoder.take() else {
            return;
        };
        if decoder.driver().update_owned(data).is_err() {
            let _ = decoder.driver().open_stream();
        }
    }
}

impl Default for Decoder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Decoder<'a> {
    pub fn new() -> Self {
        crate::cpu::init();

        Decoder {
            inner: Box::new(driver::Decoder::new()),
            failed: None,
            input: PhantomData,
        }
    }

    fn driver(&mut self) -> &mut driver::Decoder<'a> {
        self.failed = None;
        &mut self.inner
    }

    pub fn set_format(&mut self, format: Format) -> Result<()> {
        self.driver().set_output_format(format as i32)
    }

    pub fn set_animation(&mut self, mode: Animation) -> Result<()> {
        self.driver().set_animation_mode(match mode {
            Animation::Composited => driver::ANIM_COMPOSITED,
            Animation::Subframe => driver::ANIM_SUBFRAME,
        })
    }

    pub fn set_options(&mut self, options: Options) -> Result<()> {
        self.driver().set_core_options(options)
    }

    pub fn open(&mut self, data: &[u8]) -> Result<()> {
        self.driver().open(data)
    }

    pub fn open_borrowed(&mut self, data: &'a [u8]) -> Result<()> {
        self.driver().open_borrowed(data)
    }

    pub fn open_stream(&mut self) -> Result<()> {
        self.driver().open_stream()
    }

    pub fn append(&mut self, chunk: &[u8]) -> Result<()> {
        self.driver().append(chunk)
    }

    pub fn update(&mut self, data: Vec<u8>) -> Result<UpdatedDecoder<'_, 'a>> {
        self.driver().update_owned(data)?;
        Ok(UpdatedDecoder { decoder: self })
    }

    pub fn end_of_stream(&mut self) -> Result<()> {
        self.driver().end_of_stream()
    }

    pub fn info(&mut self) -> Result<ImageInfo> {
        self.driver().image_info()
    }

    pub fn frame_info(&mut self, index: i32) -> Result<FrameInfo> {
        self.driver().frame_entry(index)
    }

    pub fn metadata(&mut self, kind: Metadata) -> Option<&[u8]> {
        let kind = match kind {
            Metadata::Iccp => 1,
            Metadata::Exif => 2,
            Metadata::Xmp => 4,
        };

        self.driver().metadata(kind).ok().flatten()
    }

    pub fn rewind(&mut self) -> Result<()> {
        self.driver().rewind()
    }

    pub fn next_frame(&mut self) -> Result<Option<Picture<'_>>> {
        let mut out = Handout::default();

        self.failed = None;
        match self.inner.next_picture(&mut out) {
            Ok(false) => Ok(None),
            Ok(true) => Ok(Some(Picture { out })),
            Err(failure) => {
                self.failed = Some(driver::described(failure));
                Err(failure.1)
            }
        }
    }

    pub fn partial_frame(&mut self) -> Result<(Picture<'_>, i32)> {
        let mut out = Handout::default();
        let mut rows = 0;

        self.failed = None;
        match self.inner.partial_picture(&mut out, &mut rows) {
            Ok(_) => Ok((Picture { out }, rows)),
            Err(failure) => {
                self.failed = Some(driver::described(failure));
                Err(failure.1)
            }
        }
    }

    pub fn error(&self) -> &str {
        match &self.failed {
            Some(message) => message,
            None => self.inner.error_message(),
        }
    }
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn set_cpu_flags_mask(mask: u32) {
    crate::cpu::set_mask(mask);
}

pub fn info(data: &[u8]) -> Result<ImageInfo> {
    let scanned = crate::container::get_info(data)?;

    Ok(ImageInfo {
        width: scanned.width,
        height: scanned.height,
        has_alpha: scanned.has_alpha,
        is_animation: scanned.animation,
        frame_count: scanned.frame_count,
        loop_count: scanned.loop_count,
        background_argb: scanned.background_argb,
        coding: scanned.coding,
        metadata: scanned.metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_PIXEL: &[u8] = &[
        b'R', b'I', b'F', b'F', 0x1a, 0, 0, 0, b'W', b'E', b'B', b'P', b'V', b'P',
        b'8', b'L', 0x0e, 0, 0, 0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x07, 0x10, 0x11,
        0x11, 0x88, 0x88, 0xfe, 0x07, 0x00,
    ];

    #[test]
    fn a_still_decodes_to_one_picture_and_then_ends() {
        let mut d = Decoder::new();

        d.set_format(Format::Rgba).unwrap();
        d.open(ONE_PIXEL).unwrap();

        let info = d.info().unwrap();

        assert_eq!((info.width, info.height), (1, 1));
        assert!(!info.is_animation);

        {
            let picture = d.next_frame().unwrap().expect("a frame");

            assert_eq!(picture.width(), 1);
            assert_eq!(picture.format(), Format::Rgba);
            assert_eq!(picture.row(0, 0).len(), 4);
            assert_eq!(picture.rows_of(0).count(), 1);
        }
        assert!(d.next_frame().unwrap().is_none());
    }

    #[test]
    fn a_truncated_file_is_refused_rather_than_panicking() {
        let mut d = Decoder::new();

        assert_eq!(d.open(&ONE_PIXEL[..20]).unwrap_err(), Error::Truncated);
        assert!(!d.error().is_empty());
    }

    #[test]
    fn what_is_not_a_webp_file_is_told_apart_from_a_damaged_one() {
        assert_eq!(info(b"not a webp file at all").unwrap_err(), Error::NotWebp);
    }

    #[test]
    fn a_borrowed_open_reads_the_caller_s_bytes_in_place() {
        let mut d = Decoder::new();

        d.open_borrowed(ONE_PIXEL).unwrap();
        assert_eq!(d.info().unwrap().width, 1);
    }

    #[test]
    fn a_stream_hands_out_nothing_until_the_frame_is_whole() {
        let mut d = Decoder::new();

        d.open_stream().unwrap();
        d.append(&ONE_PIXEL[..12]).unwrap();
        assert!(d.next_frame().unwrap().is_none());
        d.append(&ONE_PIXEL[12..]).unwrap();
        d.end_of_stream().unwrap();
        assert!(d.next_frame().unwrap().is_some());
    }

    #[test]
    fn an_updated_stream_reuses_the_callers_growing_buffer() {
        let mut d = Decoder::new();
        let mut data = Vec::with_capacity(ONE_PIXEL.len());

        data.extend_from_slice(&ONE_PIXEL[..12]);
        let allocation = data.as_ptr();

        d.open_stream().unwrap();
        let mut updated = d.update(data).unwrap();

        assert!(updated.info().is_err());

        let mut data = updated.into_buffer().unwrap();

        assert_eq!(data.as_ptr(), allocation);
        data.extend_from_slice(&ONE_PIXEL[12..]);
        assert_eq!(data.as_ptr(), allocation);

        let mut updated = data.update().unwrap();

        updated.end_of_stream().unwrap();
        assert!(updated.next_frame().unwrap().is_some());
    }

    #[test]
    fn dropping_an_update_buffer_reattaches_it() {
        let mut d = Decoder::new();

        d.open_stream().unwrap();
        let updated = d.update(ONE_PIXEL[..12].to_vec()).unwrap();

        {
            let mut data = updated.into_buffer().unwrap();

            data.extend_from_slice(&ONE_PIXEL[12..]);
        }

        d.end_of_stream().unwrap();
        assert!(d.next_frame().unwrap().is_some());
    }
}
