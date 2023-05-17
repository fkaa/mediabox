use std::io::{self, IoSlice, Read, Seek, SeekFrom, Write};

use downcast::{downcast, Any};

pub trait SyncReadSeek: Any + Read + Seek {}
impl<T> SyncReadSeek for T where T: Any + Read + Seek {}

pub trait SyncRead: Any + Read {}
impl<T> SyncRead for T where T: Any + Read {}

downcast!(dyn SyncReadSeek);
downcast!(dyn SyncRead);

pub enum SyncReader {
    Seekable(Box<dyn SyncReadSeek>),
    Stream(Box<dyn SyncRead>),
}
impl SyncReader {
    pub fn from_read<T: Into<Box<R>>, R: SyncRead>(read: T) -> Self {
        SyncReader::Stream(read.into())
    }

    pub fn into_reader<T: 'static>(self) -> Box<T> {
        match self {
            SyncReader::Seekable(reader) => reader.downcast().expect("Wrong type"),
            SyncReader::Stream(reader) => reader.downcast().expect("Wrong type"),
        }
    }
}
impl Seek for SyncReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SyncReader::Seekable(reader) => reader.seek(pos),
            SyncReader::Stream(_) => Err(io::Error::new(io::ErrorKind::Other, "woops")),
        }
    }
}

pub trait SyncWriteSeek: Any + Write + Seek {}
impl<T> SyncWriteSeek for T where T: Any + Write + Seek {}

pub trait SyncWrite: Any + Write {}
impl<T> SyncWrite for T where T: Any + Write {}

downcast!(dyn SyncWriteSeek);
downcast!(dyn SyncWrite);

pub enum SyncWriter {
    Seekable(Box<dyn SyncWriteSeek>),
    Stream(Box<dyn SyncWrite>),
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

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        match self {
            SyncWriter::Seekable(writer) => writer.write_vectored(bufs),
            SyncWriter::Stream(writer) => writer.write_vectored(bufs),
        }
    }

    /*fn is_write_vectored(&self) -> bool {
        match self {
            SyncWriter::Seekable(writer) => writer.is_write_vectored(),
            SyncWriter::Stream(writer) => writer.is_write_vectored(),
        }
    }*/

    fn flush(&mut self) -> io::Result<()> {
        match self {
            SyncWriter::Seekable(writer) => writer.flush(),
            SyncWriter::Stream(writer) => writer.flush(),
        }
    }
}
