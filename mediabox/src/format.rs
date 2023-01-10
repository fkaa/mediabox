use std::{
    cmp::Ordering,
    fmt::Debug,
    fs::File,
    io::{Seek, SeekFrom},
};

use async_trait::async_trait;

use crate::{
    io::{Buffered, GrowableBufferedReader, Io, SyncReader},
    Packet, Span, Track,
};

use std::fmt::Write;

use self::mkv::MatroskaDemuxer;

// pub mod hls;
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

pub struct DemuxerContext {
    demuxer: Box<dyn Demuxer2>,
    buf: GrowableBufferedReader,
}

impl DemuxerContext {
    pub fn open(url: &str) -> anyhow::Result<Self> {
        let demuxer = Box::new(MatroskaDemuxer::new2());

        let reader = SyncReader::Seekable(Box::new(File::open(url)?));
        let buf = GrowableBufferedReader::new(reader);

        Ok(DemuxerContext { demuxer, buf })
    }

    pub fn read_headers(&mut self) -> anyhow::Result<Movie> {
        loop {
            match self.demuxer.read_headers(&mut self.buf) {
                Ok(movie) => return Ok(movie),
                Err(DemuxerError::NeedMore(more)) => {
                    eprintln!("growing: {more}");
                    self.buf.ensure_additional(more);
                    self.buf.fill_buf()?;
                }
                Err(DemuxerError::Seek(seek)) => {
                    eprintln!("seeking: {seek:?}");

                    self.buf.seek(seek)?;
                }
                Err(DemuxerError::Misc(err)) => return Err(err),
            }
        }
    }

    pub fn read_packet(&mut self) -> anyhow::Result<Packet> {
        loop {
            match self.demuxer.read_packet(&mut self.buf) {
                Ok(pkt) => return Ok(pkt),
                Err(DemuxerError::NeedMore(more)) => {
                    self.buf.ensure_additional(more);
                    self.buf.fill_buf()?;
                }
                Err(DemuxerError::Seek(seek)) => {
                    eprintln!("seeking: {seek:?}");

                    self.buf.seek(seek)?;
                }
                Err(DemuxerError::Misc(err)) => return Err(err),
            }
        }
    }
}

pub trait Demuxer2 {
    fn read_headers(&mut self, buf: &mut dyn Buffered) -> Result<Movie, DemuxerError>;
    fn read_packet(&mut self, buf: &mut dyn Buffered) -> Result<Packet, DemuxerError>;
}

#[derive(Debug)]
pub enum DemuxerResponse {
    Movie(Movie),
    Packet(Packet),
}

#[derive(Debug, thiserror::Error)]
pub enum DemuxerError {
    #[error("")]
    NeedMore(usize),
    #[error("")]
    Seek(SeekFrom),

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
    async fn write(&mut self, packet: Packet) -> anyhow::Result<()>;

    /// Stops the muxer. This will flush any buffered packets and finalize the output if
    /// appropriate.
    async fn stop(&mut self) -> anyhow::Result<()>;

    fn into_io(self) -> Io;
}

#[derive(Clone)]
pub struct DemuxerMetadata {
    pub name: &'static str,
    create: fn(Io) -> Box<dyn Demuxer>,
    probe: fn(&[u8]) -> ProbeResult,
}

impl DemuxerMetadata {
    pub fn create(&self, io: Io) -> Box<dyn Demuxer> {
        (self.create)(io)
    }

    pub fn probe(&self, data: &[u8]) -> ProbeResult {
        (self.probe)(data)
    }
}

#[derive(Clone)]
pub struct MuxerMetadata {
    name: &'static str,
    create: fn(Io) -> Box<dyn Muxer>,
}

impl MuxerMetadata {
    pub fn create(&self, io: Io) -> Box<dyn Muxer> {
        (self.create)(io)
    }
}

/// A muxer that can handle splitting up the output into multiple segments.
pub struct SegmentMuxer {
    muxer: Box<dyn Muxer>,
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
    pub data: Span,
}

impl Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({}) {} B", self.name, self.mime, self.data.len())
    }
}
