use std::{io::{SeekFrom, Cursor}};

use async_trait::async_trait;

use crate::{io::Io, OwnedPacket, Packet, Span, Track};

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

pub struct SyncMuxerContext {}

impl SyncMuxerContext {}




pub struct ScratchMemory<'a> {
    buf: Cursor<&'a mut [u8]>,
}

pub trait MuxerContext {
    fn write(&mut self, span: Span);
}

pub trait Muxer2 {
    fn start(&mut self, movie: Movie) -> Result<Span, MuxerError>;
    fn write(&mut self, packet: Packet) -> Result<Span, MuxerError>;
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
    NeedMore,

    #[error("Requesting seek")]
    Seek(SeekFrom),

    #[error("End of stream")]
    EndOfStream,

    #[error("{0}")]
    Misc(#[from] anyhow::Error),
}

/// A trait for exposing functionality related to muxing together multiple streams into a container
/// format.
#[async_trait]
pub trait Muxer: Send {
    /// Starts the muxer with the given tracks.
    async fn start(&mut self, tracks: Vec<Track>) -> anyhow::Result<()>;

    /// Writes a packet to the muxer.
    ///
    /// Note that this does not ensure something will be written to the output, as it may buffer
    /// packets internally in order to write its output correctly.
    async fn write(&mut self, packet: OwnedPacket) -> anyhow::Result<()>;

    /// Stops the muxer. This will flush any buffered packets and finalize the output if
    /// appropriate.
    async fn stop(&mut self) -> anyhow::Result<()>;

    fn into_io(self) -> Io;
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

/// A muxer that can handle splitting up the output into multiple segments.
pub struct SegmentMuxer {
    muxer: Box<dyn Muxer>,
}
