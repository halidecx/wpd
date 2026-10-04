use std::io::{self, Write};

use wpd::api::ImageInfo;

struct Chunk<'a> {
    tag: &'a [u8],
    offset: usize,
    size: u32,
    payload: &'a [u8],
    complete: bool,
}

struct Chunks<'a> {
    data: &'a [u8],
    offset: usize,
    end: usize,
}

impl<'a> Chunks<'a> {
    fn new(data: &'a [u8]) -> Self {
        let end =
            if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
                (u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64 + 8)
                    .min(data.len() as u64) as usize
            } else {
                0
            };

        Self {
            data,
            offset: 12,
            end,
        }
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.end.saturating_sub(self.offset) < 8 {
            return None;
        }
        let offset = self.offset;
        let size =
            u32::from_le_bytes(self.data[offset + 4..offset + 8].try_into().unwrap());
        let padded = size as u64 + u64::from(size & 1);
        let available = self.end - (offset + 8);
        let complete = padded <= available as u64;
        let payload = &self.data[offset + 8..offset + 8 + available.min(size as usize)];

        self.offset = if complete {
            offset + 8 + padded as usize
        } else {
            self.end
        };
        Some(Chunk {
            tag: &self.data[offset..offset + 4],
            offset,
            size,
            payload,
            complete,
        })
    }
}

/* FourCCs are bytes, including on damaged inputs. Escape each byte outside
 * printable ASCII rather than replacing it with a Unicode replacement char. */
fn write_fourcc(w: &mut impl Write, tag: &[u8]) -> io::Result<()> {
    w.write_all(b"\"")?;
    for &b in tag {
        match b {
            b'"' | b'\\' => w.write_all(&[b'\\', b])?,
            0x20..=0x7e => w.write_all(&[b])?,
            _ => write!(w, "\\u{b:04x}")?,
        }
    }
    w.write_all(b"\"")
}

pub fn write_json(
    mut w: impl Write,
    image: &ImageInfo,
    data: &[u8],
    has_icc: bool,
) -> io::Result<()> {
    write!(w, "{{\"width\":{},\"height\":{},\"frame_count\":{},\"loop_count\":{},\"has_alpha\":{},\"has_icc\":{},\"durations_ms\":[",
        image.width, image.height, image.frame_count, image.loop_count, image.has_alpha, has_icc)?;
    if image.is_animation {
        let mut first = true;

        for chunk in Chunks::new(data) {
            if chunk.tag != b"ANMF" || chunk.payload.len() < 16 {
                continue;
            }
            if !first {
                w.write_all(b",")?;
            }
            first = false;
            let duration = u32::from_le_bytes([
                chunk.payload[12],
                chunk.payload[13],
                chunk.payload[14],
                0,
            ]);

            write!(w, "{duration}")?;
        }
    } else {
        w.write_all(b"0")?;
    }
    w.write_all(b"],\"chunks\":[")?;
    for (i, chunk) in Chunks::new(data).enumerate() {
        if i != 0 {
            w.write_all(b",")?;
        }
        w.write_all(b"{\"fourcc\":")?;
        write_fourcc(&mut w, chunk.tag)?;
        write!(
            w,
            ",\"offset\":{},\"size\":{},\"complete\":{}}}",
            chunk.offset, chunk.size, chunk.complete
        )?;
    }
    w.write_all(b"]}\n")?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_fourcc_bytes_are_valid_json_strings() {
        let mut out = Vec::new();

        write_fourcc(&mut out, &[0, b'"', b'\\', 255]).unwrap();
        assert_eq!(out, b"\"\\u0000\\\"\\\\\\u00ff\"");
    }

    #[test]
    fn a_chunk_larger_than_the_remaining_input_is_reported_once() {
        let mut data = b"RIFF\x14\0\0\0WEBPTEST\xff\xff\xff\xff".to_vec();

        data.extend_from_slice(b"tail");
        let chunks: Vec<_> = Chunks::new(&data).collect();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].size, u32::MAX);
        assert_eq!(chunks[0].payload, b"tail");
        assert!(!chunks[0].complete);
        for cut in 0..data.len() {
            let _ = Chunks::new(&data[..cut]).count();
        }
    }

    #[test]
    fn padding_and_the_riff_boundary_do_not_become_chunks() {
        let data = b"RIFF\x0e\0\0\0WEBPTEST\x01\0\0\0x\0EXTRA123";
        let chunks: Vec<_> = Chunks::new(data).collect();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].payload, b"x");
        assert!(chunks[0].complete);
    }
}
