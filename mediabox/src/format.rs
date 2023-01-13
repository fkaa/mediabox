use std::{
    cmp::Ordering,
    fmt::Debug,
    fs::File,
    io::{Seek, SeekFrom},
};

use async_trait::async_trait;

use crate::{
    buffer::{Buffered, GrowableBufferedReader},
    io::{Io, SyncReader},
    Packet, Span, Track,
};

use std::fmt::Write;

use self::mkv::MatroskaDemuxer;

mod mux;

pub use mux::*;

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

pub struct DemuxerContext {
    demuxer: Box<dyn Demuxer2>,
    reader: GrowableBufferedReader,
    buf: Vec<u8>,
}

impl DemuxerContext {
    pub fn open(url: &str) -> anyhow::Result<Self> {
        let demuxer = MatroskaDemuxer::create();

        let reader = SyncReader::Seekable(Box::new(File::open(url)?));
        let (reader, buf) = GrowableBufferedReader::new(reader);

        Ok(DemuxerContext {
            demuxer,
            reader,
            buf,
        })
    }

    pub fn read_headers(&mut self) -> anyhow::Result<Movie> {
        loop {
            let data = self.reader.data(&self.buf);

            std::thread::sleep_ms(50);

            match self.demuxer.read_headers(data, &mut self.reader) {
                Ok(movie) => return Ok(movie),
                Err(DemuxerError::NeedMore(more)) => {
                    dbg!(self.buf.len());
                    self.reader.ensure_additional(&mut self.buf, more);
                    self.reader.fill_buf(&mut self.buf)?;
                }
                Err(DemuxerError::Seek(seek)) => {
                    eprintln!("seeking: {seek:?}");

                    self.reader.seek(seek)?;
                }
                Err(DemuxerError::Misc(err)) => return Err(err),
                Err(err @ DemuxerError::EndOfStream) => return Err(err.into()),
            }
        }
    }

    pub fn read_packet<'a>(&'a mut self) -> anyhow::Result<Option<Packet<'a>>> {
        loop {
            let err = {
                let buf = unsafe { std::mem::transmute::<&[u8], &[u8]>(&self.buf) };
                let data = self.reader.data(buf);

                let result = self.demuxer.read_packet(data, &mut self.reader);

                match result {
                    Ok(pkt) => return Ok(pkt),
                    Err(e) => e,
                }
            };

            match err {
                DemuxerError::EndOfStream => return Ok(None),
                DemuxerError::Misc(err) => return Err(err),

                DemuxerError::NeedMore(more) => {
                    self.reader.ensure_additional(&mut self.buf, more);
                    self.reader.fill_buf(&mut self.buf)?;
                }
                DemuxerError::Seek(seek) => {
                    eprintln!("seeking: {seek:?}");

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
