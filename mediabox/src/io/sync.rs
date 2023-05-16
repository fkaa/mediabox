use std::io::{self, Read, Seek, SeekFrom, Write};

use downcast::{downcast, Any};

pub trait SyncReadSeek: Read + Seek {}
impl<T> SyncReadSeek for T where T: Read + Seek {}

pub enum SyncReader {
    Seekable(Box<dyn SyncReadSeek>),
    Stream(Box<dyn Read>),
}
impl Seek for SyncReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SyncReader::Seekable(reader) => reader.seek(pos),
            SyncReader::Stream(_) => Err(io::Error::new(io::ErrorKind::Other, "woops")),
        }
    }
}

pub trait SyncWriteSeek: Any + Write + Seek + 'static {}
impl<T> SyncWriteSeek for T where T: Any + Write + Seek + 'static {}

pub trait SyncWrite: Any + Write + Seek + 'static {}
impl<T> SyncWrite for T where T: Any + Write + Seek + 'static {}

downcast!(dyn SyncWriteSeek);
downcast!(dyn SyncWrite);

pub enum SyncWriter {
    Seekable(Box<dyn SyncWriteSeek + 'static>),
    Stream(Box<dyn SyncWrite + 'static>),
}

impl SyncWriter {
    pub fn from_write<T: Into<Box<dyn SyncWrite>>>(write: T) -> Self {
        SyncWriter::Stream(write.into())
    }

    pub fn into_writer<T: 'static>(self) -> Box<T> {
        match self {
            SyncWriter::Seekable(writer) => writer.downcast().expect("Wrong type"),
            SyncWriter::Stream(writer) => writer.downcast().expect("Wrong type"),
        }
    }
}

impl Seek for SyncWriter {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SyncWriter::Seekable(writer) => writer.seek(pos),
            SyncWriter::Stream(_) => Err(io::Error::new(io::ErrorKind::Other, "woops")),
        }
    }
}
impl Write for SyncWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            SyncWriter::Seekable(writer) => writer.write(bytes),
            SyncWriter::Stream(writer) => writer.write(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            SyncWriter::Seekable(writer) => writer.flush(),
            SyncWriter::Stream(writer) => writer.flush(),
        }
    }
}
