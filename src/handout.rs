use crate::image::Format;
use crate::picture::Frame;

#[derive(Default)]
pub enum Pixels<'a> {
    Own(Frame<'a>),
    Sink,
    #[default]
    None,
}

#[derive(Default)]
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
    pub no_blend: bool,
    pub has_alpha: bool,
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
