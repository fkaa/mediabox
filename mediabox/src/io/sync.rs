use std::io::{self, Read, Seek, SeekFrom, Write};

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

pub trait SyncWriteSeek: Write + Seek {}
impl<T> SyncWriteSeek for T where T: Write + Seek {}

pub enum SyncWriter {
    Seekable(Box<dyn SyncWriteSeek>),
    Stream(Box<dyn Write>),
}
impl Seek for SyncWriter {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SyncWriter::Seekable(writer) => writer.seek(pos),
            SyncWriter::Stream(_) => Err(io::Error::new(io::ErrorKind::Other, "woops")),
        }
    }
}
impl ::std::io::Write for SyncWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
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
