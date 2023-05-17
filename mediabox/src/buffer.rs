use std::cmp;
use std::io::{self, Seek, SeekFrom};

use crate::io::SyncReader;

/// Partial consumption buffer for any reader.
pub struct GrowableBufferedReader {
    inner: SyncReader,
    buf_pos: usize,
    pos: usize,
    end: usize,
    // Position in the stream of the buffer's beginning
    index: usize,
}

impl GrowableBufferedReader {
    /// Creates a new `AccReader` instance.
    pub fn new(inner: SyncReader) -> GrowableBufferedReader {
        let reader = GrowableBufferedReader {
            inner,
            buf_pos: 0,
            pos: 0,
            end: 0,
            index: 0,
        };

        reader
    }

    /// Resets the buffer to the current position.
    ///
    /// All data before the current position is lost.
    pub fn reset_buffer_position(&mut self, buf: &mut [u8]) {
        if self.end - self.pos > 0 {
            buf.copy_within(self.pos..self.end, 0);
        }

        self.buf_pos += self.pos;
        self.end -= self.pos;
        self.pos = 0;
    }

    /// Returns buffer capacity.
    pub fn capacity(&self) -> usize {
        self.end - self.pos
    }

    pub fn ensure_additional(&mut self, buf: &mut Vec<u8>, more: usize) {
        let len = self.end - self.pos;

        self.ensure_capacity(buf, len + more);
    }

    pub fn ensure_capacity(&mut self, buf: &mut Vec<u8>, len: usize) {
        let capacity_left = buf.len() - self.pos;

        if capacity_left >= len {
            return;
        }

        if len <= buf.len() {
            self.reset_buffer_position(buf);
            return;
        }

        buf.resize(len, 0);
    }

    pub fn data<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.pos..self.end]
    }

    pub fn len(&self) -> usize {
        self.end - self.pos
    }

    pub fn fill_buf(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if self.pos != 0 || self.end != buf.len() {
            self.reset_buffer_position(buf);

            let read = match self.inner {
                SyncReader::Stream(ref mut r) => r.read(&mut buf[self.end..])?,
                SyncReader::Seekable(ref mut r) => r.read(&mut buf[self.end..])?,
            };

            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Reached end of stream",
                ));
            }

            self.end += read;
        }

        Ok(())
    }
}

impl Seek for GrowableBufferedReader {
    fn seek(&mut self, mut pos: SeekFrom) -> io::Result<u64> {
        if let SeekFrom::Current(pos) = &mut pos {
            let abs_pos = (self.buf_pos + self.pos) as i64;
            let abs_end = (self.buf_pos + self.end) as i64;

            let new_pos = abs_pos + *pos;

            if new_pos > abs_end {
                let seek = new_pos - abs_end;

                *pos = seek;
            } else if new_pos < self.buf_pos as i64 {
                let seek = *pos - (self.end - self.pos) as i64;

                *pos = seek;
            } else {
                self.pos = (new_pos - self.buf_pos as i64) as usize;
                return Ok(new_pos as u64);
            }
        }

        let pos = self.inner.seek(pos)?;

        self.buf_pos = pos as usize;
        self.pos = 0;
        self.end = 0;

        Ok(pos)
    }
}

pub trait Buffered {
    fn consume(&mut self, len: usize);
}

impl Buffered for GrowableBufferedReader {
    fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.end);
        self.index += amt;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use test_case::test_case;

    use std::io::Cursor;

    #[derive(Debug)]
    enum Op<'a> {
        Assert(&'a [u8]),
        Seek(SeekFrom),
        Fill,
        Consume(usize),
    }

    #[test_case(
        5,
        b"0123456789",
        &[
            Op::Fill,
            Op::Assert(b"01234"),
            Op::Seek(SeekFrom::Current(1)),
            Op::Assert(b"1234"),
            Op::Fill,
            Op::Assert(b"12345"),
            Op::Seek(SeekFrom::Current(5)),
            Op::Assert(b""),
            Op::Fill,
            Op::Assert(b"6789"),
            Op::Seek(SeekFrom::Current(-2)),
            Op::Assert(b""),
            Op::Fill,
            Op::Assert(b"45678"),
        ]
    )]
    #[test_case(
        10,
        b"abc",
        &[Op::Assert(b""),]
    )]
    #[test_case(
        10,
        b"abc",
        &[
            Op::Fill,
            Op::Assert(b"abc"),
            Op::Seek(SeekFrom::Current(1)),
            Op::Assert(b"bc"),
            Op::Seek(SeekFrom::Current(1)),
            Op::Assert(b"c"),
            Op::Seek(SeekFrom::Current(1)),
            Op::Assert(b""),
            Op::Seek(SeekFrom::Current(-3)),
            Op::Assert(b"abc"),
        ]
    )]
    #[test_case(
        10,
        b"abc",
        &[
            Op::Fill,
            Op::Assert(b"abc"),
            Op::Consume(1),
            Op::Assert(b"bc"),
            Op::Consume(1),
            Op::Assert(b"c"),
            Op::Consume(1),
            Op::Assert(b""),
        ]
    )]
    fn buffered_reader(capacity: usize, data: &'static [u8], ops: &[Op]) {
        let cur = Cursor::new(data);
        let reader = SyncReader::Seekable(Box::new(cur));
        let mut reader = GrowableBufferedReader::new(reader);
        let mut buf = vec![0u8; capacity];

        for op in ops {
            match op {
                Op::Assert(data) => assert_eq!(reader.data(&buf), *data),
                Op::Seek(seek) => {
                    reader.seek(*seek).unwrap();
                }
                Op::Fill => reader.fill_buf(&mut buf).unwrap(),
                Op::Consume(c) => {
                    reader.consume(*c);
                }
            }
        }
    }
}
