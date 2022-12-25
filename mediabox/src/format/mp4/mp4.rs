use anyhow::Context;
use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use log::*;

use std::{collections::HashMap, io::SeekFrom, time::Duration};

use crate::{
    codec::nal::{convert_bitstream, BitstreamFraming},
    format::Muxer,
    io::Io,
    muxer, H264Codec, MediaDuration, MediaKind, MediaTime, Packet, Span, Track, VideoCodec,
    VideoInfo,
};

use super::{write_audio_trak, write_video_trak, SampleEntry, TrackBuilder};

muxer!("fmp4", Mp4Muxer::create);

pub struct Mp4Muxer {
    video: Option<Track>,
    audio: Option<Track>,
    track_builders: HashMap<u32, TrackBuilder>,
    io: Io,
    mdat_start: u64,
}

impl Mp4Muxer {
    pub fn new(io: Io) -> Self {
        Mp4Muxer {
            video: None,
            audio: None,
            track_builders: HashMap::new(),
            io,
            mdat_start: 0,
        }
    }

    fn create(io: Io) -> Box<dyn Muxer> {
        Box::new(Self::new(io))
    }

    async fn write_moov_box(&mut self) -> anyhow::Result<()> {
        let mut buf = BytesMut::new();

        write_box!(&mut buf, b"moov", {
            super::write_mvhd(&mut buf);

            write_box!(&mut buf, b"mvex", {
                write_box!(&mut buf, b"mehd", {
                    buf.put_u32(1 << 24); // version
                    buf.put_u64(0); // duration
                });
            });

            for builder in self.track_builders.values() {
                super::write_trak(&mut buf, builder.clone())?;
            }
        });

        Ok(())
    }
}

#[async_trait]
impl Muxer for Mp4Muxer {
    async fn start(&mut self, streams: Vec<Track>) -> anyhow::Result<()> {
        let mut buf = BytesMut::new();

        write_box!(&mut buf, b"ftyp", {
            buf.extend_from_slice(b"isom\0\0\0\0isomiso5dash");
        });

        self.mdat_start = self
            .io
            .seek(SeekFrom::Current(0))
            .await
            .context("Failed to get mdat position")?;
        // 8 byte length and 'mdat'
        buf.extend_from_slice(b"\0\0\0\0\0\0\0\0mdat");

        Ok(())
    }

    async fn write(&mut self, packet: Packet) -> anyhow::Result<()> {
        let builders = &mut self.track_builders;
        let Some(builder) = builders.get_mut(&packet.track.id) else {
            return Ok(());
        };

        let sample_data = super::get_packet_sample_data(&packet);

        let sample_entry = SampleEntry {
            is_sync: packet.key,
            time: packet.time.clone(),
            size: sample_data.len() as u64,
        };

        builder.add_sample(sample_entry);

        self.io.write_span(sample_data).await?;

        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        let current = self.io.seek(SeekFrom::Current(0)).await?;

        let mdat_len = (current - self.mdat_start) as u64;

        self.io.seek(SeekFrom::Start(self.mdat_start)).await?;
        self.io.write(&mdat_len.to_be_bytes()[..]).await?;
        self.io.seek(SeekFrom::Start(current)).await?;

        self.write_moov_box().await?;
        Ok(())
    }
    
    fn into_io(self) -> Io {
        self.io
    }
}

struct DecodeTimeBuffer {
    buffer: Vec<Packet>,
}
