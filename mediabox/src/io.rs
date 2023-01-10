use tokio::io::AsyncReadExt;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

#[cfg(feature = "fs")]
use tokio::fs::File;

use anyhow::Context;
use downcast::{downcast, Any};
use fluent_uri::Uri;

use std::{io::SeekFrom, path::Path};

use crate::Span;

pub trait WriteSeek: Any + AsyncWrite + AsyncSeek + Unpin + Sync + Send + 'static {}
pub trait Write: Any + AsyncWrite + Unpin + Sync + Send {}

downcast!(dyn WriteSeek);
downcast!(dyn Write);

impl<T> WriteSeek for T where T: AsyncWrite + AsyncSeek + Unpin + Sync + Send + Sync + 'static {}
impl<T> Write for T where T: AsyncWrite + Unpin + Sync + Send + Sync + 'static {}

pub enum Writer {
    Seekable(Box<dyn WriteSeek>),
    Stream(Box<dyn Write>),
}

pub trait ReadSeek: Any + AsyncRead + AsyncSeek + Unpin + Send + Sync + 'static {}
pub trait Read: Any + AsyncRead + Unpin + Send + Sync + 'static {}

downcast!(dyn ReadSeek);
downcast!(dyn Read);

impl<T> ReadSeek for T where T: AsyncRead + AsyncSeek + Unpin + Send + Sync + 'static {}
impl<T> Read for T where T: AsyncRead + Unpin + Send + Sync + 'static {}

pub enum Reader {
    Seekable(Box<dyn ReadSeek>),
    Stream(Box<dyn Read>),
}
pub trait SyncReadSeek: std::io::Read + Seek {}
impl<T> SyncReadSeek for T where T: std::io::Read + Seek {}

pub enum SyncReader {
    Seekable(Box<dyn SyncReadSeek>),
    Stream(Box<dyn std::io::Read>),
}
impl Seek for SyncReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SyncReader::Seekable(reader) => reader.seek(pos),
            SyncReader::Stream(_) => Err(io::Error::new(io::ErrorKind::Other, "woops")),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("Stream is not readable")]
    NotReadable,

    #[error("Stream is not writeable")]
    NotWriteable,

    #[error("Stream is not seekable")]
    NotSeekable,

    #[error("Unsupported URI scheme {0:?}")]
    UnsupportedScheme(String),

    #[error("Failed to parse URI: {0}")]
    Uri(#[from] fluent_uri::ParseError),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Misc(#[from] anyhow::Error),
}

pub struct Io {
    uri: Uri<String>,
    writer: Option<Writer>,
    reader: Option<Reader>,
}

fn uri_from_path(path: &Path) -> Result<Uri<String>, IoError> {
    let uri = path.to_str().map(|s| s.to_string()).unwrap_or_default();
    let uri = Uri::parse_from(uri).map_err(|e| e.1)?;

    Ok(uri)
}

#[cfg(feature = "fs")]
impl Io {
    pub async fn create_file<P: AsRef<Path>>(path: P) -> Result<Self, IoError> {
        let uri = uri_from_path(path.as_ref())?;
        let file = File::create(path).await?;

        Ok(Io {
            uri,
            writer: Some(Writer::Seekable(Box::new(file))),
            reader: None,
        })
    }

    pub async fn open_file<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Result<Self, IoError> {
        let uri = uri_from_path(path.as_ref())?;
        let file = File::open(&path)
            .await
            .with_context(|| format!("Failed to open file {path:?}"))?;

        Ok(Io {
            uri,
            writer: None,
            reader: Some(Reader::Seekable(Box::new(file))),
        })
    }
}

impl Io {
    pub fn null() -> Self {
        Io {
            uri: Uri::parse_from(String::new()).unwrap(),
            writer: None,
            reader: None,
        }
    }

    pub async fn open(uri: String) -> Result<Self, IoError> {
        let uri = Uri::parse_from(uri).map_err(|e| e.1)?;

        match uri.scheme().map(|s| s.as_str()) {
            Some("file") | None => {}
            Some(scheme) => {
                return Err(IoError::UnsupportedScheme(scheme.to_string()));
            }
        }

        todo!()
    }

    pub fn from_stream(writer: Box<dyn Write>) -> Self {
        Io {
            uri: Uri::parse_from(String::new()).unwrap(),
            writer: Some(Writer::Stream(writer)),
            reader: None,
        }
    }

    pub fn from_reader(reader: Box<dyn Read>) -> Self {
        Io {
            uri: Uri::parse_from(String::new()).unwrap(),
            writer: None,
            reader: Some(Reader::Stream(reader)),
        }
    }

    pub async fn write_span(&mut self, span: Span) -> Result<(), IoError> {
        use tokio::io::AsyncWriteExt;

        let writer = self.writer.as_mut().ok_or(IoError::NotWriteable)?;

        match writer {
            Writer::Seekable(writer) => {
                // TODO: replace with write_vectored
                for span in span.to_byte_spans() {
                    writer.write_all(&span[..]).await?
                }
            }
            Writer::Stream(writer) => {
                // TODO: replace with write_vectored
                for span in span.to_byte_spans() {
                    writer.write_all(&span[..]).await?
                }
            }
        };

        Ok(())
    }

    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        use tokio::io::AsyncWriteExt;

        let writer = self.writer.as_mut().ok_or(IoError::NotWriteable)?;

        match writer {
            Writer::Seekable(writer) => writer.write_all(bytes).await?,
            Writer::Stream(writer) => writer.write_all(bytes).await?,
        }

        Ok(())
    }

    pub fn into_reader(self) -> Result<Reader, IoError> {
        let reader = self.reader.ok_or(IoError::NotReadable)?;

        Ok(reader)
    }

    pub fn reader(&mut self) -> Result<&mut (dyn AsyncRead + Unpin), IoError> {
        let reader = self.reader.as_mut().ok_or(IoError::NotReadable)?;

        match reader {
            Reader::Seekable(reader) => Ok(reader),
            Reader::Stream(reader) => Ok(reader),
        }
    }

    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        let reader = self.reader.as_mut().ok_or(IoError::NotWriteable)?;

        match reader {
            Reader::Seekable(reader) => reader.read_exact(buf).await?,
            Reader::Stream(reader) => reader.read_exact(buf).await?,
        };

        Ok(())
    }

    pub async fn read_probe(&mut self) -> Result<&[u8], IoError> {
        let reader = self.reader.as_mut().ok_or(IoError::NotWriteable)?;

        /*let inner_bytes = match reader {
            Reader::Seekable(reader) => reader.fill_buf().await?,
            Reader::Stream(reader) => reader.fill_buf().await?,
        };

        Ok(inner_bytes)*/

        todo!()
    }

    pub async fn skip(&mut self, amt: u64) -> Result<(), IoError> {
        use tokio::io::{self, AsyncSeekExt};

        let reader = self.reader.as_mut().ok_or(IoError::NotWriteable)?;

        match reader {
            Reader::Seekable(reader) => reader.seek(SeekFrom::Current(amt as i64)).await?,
            Reader::Stream(reader) => io::copy(&mut reader.take(amt), &mut io::sink()).await?,
        };

        Ok(())
    }

    pub async fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        use tokio::io::AsyncSeekExt;

        let writer = self.writer.as_mut().ok_or(IoError::NotWriteable)?;

        let pos = match writer {
            Writer::Seekable(writer) => writer.seek(pos).await?,
            _ => return Err(IoError::NotSeekable)?,
        };

        Ok(pos)
    }

    pub fn seekable(&self) -> bool {
        matches!(self.writer, Some(Writer::Seekable(_)))
    }

    pub fn into_writer<T: 'static>(&mut self) -> Result<Box<T>, IoError> {
        let writer = self.writer.take().ok_or(IoError::NotWriteable)?;

        let writer = match writer {
            Writer::Seekable(writer) => writer
                .downcast::<T>()
                .expect("Invalid write type requested"),
            Writer::Stream(writer) => writer
                .downcast::<T>()
                .expect("Invalid write type requested"),
        };

        Ok(writer)
    }
}

use std::cmp;
use std::io::{self, Seek};
use std::iter;
use std::iter::Iterator;

/// Partial consumption buffer for any reader.
pub struct GrowableBufferedReader {
    inner: SyncReader,
    buf: Vec<u8>,
    buf_pos: usize,
    pos: usize,
    end: usize,
    // Position in the stream of the buffer's beginning
    index: usize,
}

impl GrowableBufferedReader {
    /// Creates a new `AccReader` instance.
    pub fn new(inner: SyncReader) -> GrowableBufferedReader {
        GrowableBufferedReader::with_capacity(4096, inner)
    }

    /// Creates a new `AccReader` instance of a determined capacity
    /// for a reader.
    pub fn with_capacity(cap: usize, inner: SyncReader) -> GrowableBufferedReader {
        GrowableBufferedReader {
            inner,
            buf: iter::repeat(0).take(cap).collect::<Vec<_>>(),
            buf_pos: 0,
            pos: 0,
            end: 0,
            index: 0,
        }
    }

    /// Resets the buffer to the current position.
    ///
    /// All data before the current position is lost.
    pub fn reset_buffer_position(&mut self) {
        if self.end - self.pos > 0 {
            self.buf.copy_within(self.pos..self.end, 0);
        }

        self.end -= self.pos;
        self.pos = 0;
    }

    /// Returns buffer data.
    pub fn current_slice(&self) -> &[u8] {
        &self.buf[self.pos..self.end]
    }

    /// Returns buffer capacity.
    pub fn capacity(&self) -> usize {
        self.end - self.pos
    }

    pub fn ensure_additional(&mut self, more: usize) {
        let len = self.end - self.pos;

        self.ensure_capacity(len + more);
    }

    pub fn ensure_capacity(&mut self, len: usize) {
        let capacity_left = self.buf.len() - self.pos;

        if capacity_left >= len {
            return;
        }

        if len <= self.buf.len() {
            self.reset_buffer_position();
            return;
        }

        self.buf.resize(len, 0);
    }

    pub fn fill_buf(&mut self) -> io::Result<()> {
        if self.pos != 0 || self.end != self.buf.len() {
            self.reset_buffer_position();

            let read = match self.inner {
                SyncReader::Stream(ref mut r) => r.read(&mut self.buf[self.end..])?,
                SyncReader::Seekable(ref mut r) => r.read(&mut self.buf[self.end..])?,
            };

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
    fn data(&self) -> &[u8];
    fn consume(&mut self, len: usize);
}

impl Buffered for GrowableBufferedReader {
    fn data(&self) -> &[u8] {
        &self.buf[self.pos..self.end]
    }
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
    #[test]
    fn buffered_reader(capacity: usize, data: &'static [u8], ops: &[Op]) {
        let cur = Cursor::new(data);
        let reader = SyncReader::Seekable(Box::new(cur));
        let mut buf = GrowableBufferedReader::with_capacity(capacity, reader);

        for op in ops {
            match op {
                Op::Assert(data) => assert_eq!(buf.data(), *data),
                Op::Seek(seek) => {
                    buf.seek(*seek).unwrap();
                }
                Op::Fill => buf.fill_buf().unwrap(),
                Op::Consume(c) => {
                    buf.consume(*c);
                }
            }
        }
    }
}
