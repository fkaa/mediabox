use std::{
    cmp::Ordering,
    fmt::Debug,
    fs::File,
    io::{ErrorKind, Seek, SeekFrom},
    mem,
};

use async_trait::async_trait;
use tracing::debug;

use crate::{
    buffer::{Buffered, GrowableBufferedReader},
    io::{Io, SyncReader},
    memory::{Memory, MemoryPool},
    CodecId, Packet, Span, Track,
};

use self::{ass::AssDemuxer, mkv::MatroskaDemuxer};

mod mux;

pub use mux::*;

// pub mod hls;
pub mod ass;
pub mod mkv;
// pub mod mp4;

// #[cfg(feature = "rtmp")]
// pub mod rtmp;
// pub mod webvtt;

/// Registers a demuxer with mediabox
#[macro_export]
macro_rules! demuxer {
    ($name:literal, $create:expr, $probe:expr) => {
        pub const DEMUXER_META: $crate::format::DemuxerMetadata = $crate::format::DemuxerMetadata {
            name: $name,
            create: $create,
            probe: $probe,
        };
    };
}

pub struct DemuxerContext {
    demuxer: Box<dyn Demuxer2>,
    reader: GrowableBufferedReader,
    pool: MemoryPool,
    memory: Memory,
}

fn convert_packet<'a>(pool: &mut MemoryPool, memory: &Memory, pkt: Packet<'a>) -> Packet<'static> {
    let new_pkt = Packet {
        key: pkt.key,
        time: pkt.time,
        track: pkt.track,
        buffer: pkt.buffer.unrealize_from_memory(memory),
    };
    new_pkt
}

impl DemuxerContext {
    /*pub fn open(url: &str) -> anyhow::Result<Self> {
        Self::open_with_pool(url, MemoryPool::create
    }*/

    pub fn open_with_pool(url: &str, pool: MemoryPool) -> anyhow::Result<Self> {
        let demuxer = if url.ends_with(".mkv") {
            MatroskaDemuxer::create()
        } else {
            AssDemuxer::create()
        };

        let reader = SyncReader::Seekable(Box::new(File::open(url)?));
        let reader = GrowableBufferedReader::new(reader);

        let memory = pool.alloc(0);

        Ok(DemuxerContext {
            demuxer,
            reader,
            pool,
            memory,
        })
    }

    pub fn read_headers(&mut self) -> anyhow::Result<Movie> {
        loop {
            let data = self.reader.data(&self.memory);

            match self.demuxer.read_headers(data, &mut self.reader) {
                Ok(movie) => return Ok(movie),
                Err(DemuxerError::NeedMore(more)) => {
                    self.reader.ensure_additional(&mut self.memory, more);
                    self.reader.fill_buf(&mut self.memory)?;
                }
                Err(DemuxerError::Seek(seek)) => {
                    debug!("seeking: {seek:?}");

                    self.reader.seek(seek)?;
                }
                Err(DemuxerError::Misc(err)) => return Err(err),
                Err(err @ DemuxerError::EndOfStream) => return Err(err.into()),
            }
        }
    }

    pub fn read_packet(&mut self) -> anyhow::Result<Option<Packet<'static>>> {
        loop {
            let err = {
                let data = self.reader.data(&self.memory);

                let result = self.demuxer.read_packet(data, &mut self.reader);

                match result {
                    Ok(Some(pkt)) => {
                        let pkt_len = pkt.buffer.len();
                        let mut new_memory = self.pool.alloc(pkt_len);
                        new_memory[..pkt_len].copy_from_slice(&pkt.buffer.to_slice());

                        let pkt = Packet {
                            time: pkt.time,
                            key: pkt.key,
                            track: pkt.track,
                            buffer: Span::from_memory(new_memory, 0, pkt_len),
                        };

                        return Ok(Some(pkt));
                    }
                    Ok(None) => return Ok(None),
                    Err(e) => e,
                }
            };

            match err {
                DemuxerError::EndOfStream => return Ok(None),
                DemuxerError::Misc(err) => return Err(err),

                DemuxerError::NeedMore(more) => {
                    self.reader.ensure_additional(&mut self.memory, more);
                    if let Err(e) = self.reader.fill_buf(&mut self.memory) {
                        if e.kind() == ErrorKind::UnexpectedEof {
                            return Ok(None);
                        }

                        return Err(e.into());
                    }
                }
                DemuxerError::Seek(seek) => {
                    debug!("seeking: {seek:?}");

                    self.reader.seek(seek)?;
                }
            }
        }
    }
}

pub trait Demuxer2 {
    fn read_headers(&mut self, data: &[u8], buf: &mut dyn Buffered) -> Result<Movie, DemuxerError>;
    fn read_packet<'a>(
        &mut self,
        data: &'a [u8],
        buf: &mut dyn Buffered,
    ) -> Result<Option<Packet<'a>>, DemuxerError>;

    fn create() -> Box<dyn Demuxer2>
    where
        Self: Default + 'static,
    {
        Box::<Self>::default()
    }

    fn probe(data: &[u8]) -> ProbeResult
    where
        Self: Sized,
    {
        ProbeResult::Unsure
    }
}

#[derive(Debug)]
pub enum DemuxerResponse<'a> {
    Movie(Movie),
    Packet(Packet<'a>),
}

#[derive(Debug, thiserror::Error)]
pub enum DemuxerError {
    #[error("")]
    NeedMore(usize),
    #[error("")]
    Seek(SeekFrom),

    #[error("End of stream")]
    EndOfStream,

    // TODO: Skip(usize)
    #[error("{0}")]
    Misc(#[from] anyhow::Error),
}

#[async_trait(?Send)]
pub trait Demuxer {
    async fn start(&mut self) -> anyhow::Result<Movie>;
    async fn read(&mut self) -> anyhow::Result<Packet>;
    async fn stop(&mut self) -> anyhow::Result<()>;

    fn create(io: Io) -> Box<dyn Demuxer>
    where
        Self: Sized;

    fn probe(data: &[u8]) -> ProbeResult
    where
        Self: Sized,
    {
        ProbeResult::Unsure
    }
}

#[derive(Clone)]
pub struct DemuxerMetadata {
    pub name: &'static str,
    create: fn() -> Box<dyn Demuxer2>,
    probe: fn(&[u8]) -> ProbeResult,
}

impl DemuxerMetadata {
    pub fn create(&self) -> Box<dyn Demuxer2> {
        (self.create)()
    }

    pub fn probe(&self, data: &[u8]) -> ProbeResult {
        (self.probe)(data)
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum ProbeResult {
    Yup,
    Maybe(f32),
    Unsure,
}

impl PartialOrd for ProbeResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use ProbeResult::*;

        let ordering = match (self, other) {
            (Yup, Yup) => Ordering::Equal,
            (Yup, _) => Ordering::Greater,
            (_, Yup) => Ordering::Less,
            (Maybe(p1), Maybe(p2)) => p1.partial_cmp(p2)?,
            (Unsure, _) => Ordering::Less,
            (_, Unsure) => Ordering::Greater,
        };

        Some(ordering)
    }
}

#[derive(Default, Debug, Clone)]
pub struct Movie {
    pub tracks: Vec<Track>,
    pub attachments: Vec<Attachment>,
}

impl Movie {
    /*pub fn codec_string(&self) -> Option<String> {
        let video = self.tracks.video()?;
        let VideoCodec::H264(H264Codec {
            profile_indication,
            profile_compatibility,
            level_indication,
            ..
        }) = video.info.video()?.codec;

        let mut codec = format!(
            "avc1.{:02x}{:02x}{:02x}",
            profile_indication, profile_compatibility, level_indication
        );

        if let Some(audio) = self.tracks.audio() {
            let AudioCodec::Aac(AacCodec { ref extra }) = audio.info.audio()?.codec;

            write!(&mut codec, ",mp4a.40.{:02X}", extra[0] >> 3).ok()?;
        }

        Some(codec)
    }*/

    /*pub fn subtitles(&self) -> impl Iterator<Item = &Track> + '_ {
        self.tracks.iter().filter(|t| t.info.subtitle().is_some())
    }*/
}

#[derive(Clone)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: Span<'static>,
}

impl Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({}) {} B", self.name, self.mime, self.data.len())
    }
}
