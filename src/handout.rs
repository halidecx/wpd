use crate::image::Format;
use crate::picture::Frame;

pub enum Pixels<'a> {
    Own(Frame<'a>),
    Sink,
    None,
}

pub struct Handout<'a> {
    pub pixels: Pixels<'a>,
    pub format: Format,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    pub timestamp: i64,
    pub pos_x: i32,
    pub pos_y: i32,
    pub dispose_to_background: bool,
    pub blend: bool,
    pub has_alpha: bool,
}

impl Default for Handout<'_> {
    fn default() -> Self {
        Handout {
            pixels: Pixels::None,
            format: Format::Argb,
            width: 0,
            height: 0,
            duration: 0,
            timestamp: 0,
            pos_x: 0,
            pos_y: 0,
            dispose_to_background: false,
            blend: true,
            has_alpha: false,
        }
    }
}

impl<'a> Handout<'a> {
    pub fn planes(&self) -> usize {
        self.format.nb_components()
    }

    pub fn frame(&self) -> Option<&Frame<'a>> {
        match &self.pixels {
            Pixels::Own(frame) => Some(frame),
            _ => None,
        }
    }
}

pub trait RowSink {
    fn fits(&self, p: usize, row_len: usize, rows: i32) -> bool;

    fn row(&mut self, p: usize, y: i32, len: usize) -> &mut [u8];
}
