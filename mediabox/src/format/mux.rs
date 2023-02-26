use std::{
    fs::File,
    io::{SeekFrom, Write},
};

use crate::format::mkv::MatroskaMuxer;
use crate::{io::SyncWriter, memory::MemoryPool, Packet, Span};

use super::Movie;

/// Registers a muxer with mediabox
#[macro_export]
macro_rules! muxer {
    ($name:literal, $create:expr) => {
        pub const MUXER_META: $crate::format::MuxerMetadata = $crate::format::MuxerMetadata {
            name: $name,
            create: $create,
        };
    };
}

pub struct SyncMuxerContext {
    muxer: Box<dyn Muxer2>,
    pool: MemoryPool,
    write: SyncWriter,
    scratch_size: usize,
}

impl SyncMuxerContext {
    pub fn open_with_pool(uri: &str, pool: MemoryPool) -> anyhow::Result<Self> {
        let muxer = MatroskaMuxer::create();
        let write = SyncWriter::Seekable(Box::new(File::create(uri)?));

        Ok(SyncMuxerContext {
            muxer,
            pool,
            write,
            scratch_size: 4096,
        })
    }

    pub fn start(&mut self, movie: &Movie) -> anyhow::Result<()> {
        loop {
            let mut memory = self.pool.alloc(self.scratch_size);
            let mut scratch = ScratchMemory::new(&mut memory);

            match self.muxer.start(&mut scratch, movie) {
                Ok(mut span) => {
                    span.realize_with_memory(memory);
                    let mut slices = span.to_io_slice();
                    self.write.write_all_vectored(&mut slices)?;

                    return Ok(());
                }
                Err(MuxerError::NeedMore(more)) => {
                    self.scratch_size += more;
                }
                Err(MuxerError::Seek(seek)) => {
                    // self.write.seek(seek)?;
                    todo!()
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }

    pub fn write(&mut self, packet: &Packet) -> anyhow::Result<()> {
        loop {
            let mut memory = self.pool.alloc(self.scratch_size);
            let mut scratch = ScratchMemory::new(&mut memory);

            match self.muxer.write(&mut scratch, packet) {
                Ok(span) => {
                    todo!()
                }
                Err(MuxerError::NeedMore(more)) => {
                    self.scratch_size += more;
                }
                Err(MuxerError::Seek(seek)) => {
                    todo!()
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }
}

pub struct ScratchMemory<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> ScratchMemory<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        ScratchMemory { buf, pos: 0 }
    }

    pub fn write<F: FnOnce(&mut [u8])>(
        &mut self,
        len: usize,
        func: F,
    ) -> Result<Span<'static>, MuxerError> {
        let end = self.pos + len;

        if end > self.buf.len() {
            return Err(MuxerError::NeedMore(end - self.buf.len()));
        }

        func(&mut self.buf[self.pos..end]);

        let span = Span::NonRealizedMemory {
            start: self.pos,
            end,
        };

        self.pos = end;

        Ok(span)
    }
}

pub trait Muxer2 {
    fn start(&mut self, scratch: &mut ScratchMemory, movie: &Movie) -> Result<Span, MuxerError>;
    fn write(&mut self, scratch: &mut ScratchMemory, packet: &Packet) -> Result<Span, MuxerError>;
    fn stop(&mut self) -> Result<Span, MuxerError>;

    fn create() -> Box<dyn Muxer2>
    where
        Self: Default + 'static,
    {
        Box::<Self>::default()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MuxerError {
    #[error("Need more data")]
    NeedMore(usize),

    #[error("Requesting seek")]
    Seek(SeekFrom),

    #[error("End of stream")]
    EndOfStream,

    #[error("{0}")]
    Misc(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct MuxerMetadata {
    pub name: &'static str,
    pub create: fn() -> Box<dyn Muxer2>,
}

impl MuxerMetadata {
    pub fn create(&self) -> Box<dyn Muxer2> {
        (self.create)()
    }
}
