use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use log::*;

use std::{collections::HashMap, time::Duration};

use crate::{
    codec::nal::{convert_bitstream, BitstreamFraming},
    format::Muxer,
    io::Io,
    muxer, H264Codec, MediaDuration, MediaKind, MediaTime, Packet, Span, Track, VideoCodec,
    VideoInfo,
};

use super::{write_audio_trak, write_video_trak, TrackBuilder};

muxer!("fmp4", FragmentedMp4Muxer::create);

pub struct FragmentedMp4Muxer {
    video: Option<Track>,
    audio: Option<Track>,
    start_times: HashMap<u32, MediaTime>,
    prev_times: HashMap<u32, MediaTime>,
    track_mapping: HashMap<u32, u32>,
    io: Io,
    seq: u64,
}

impl FragmentedMp4Muxer {
    pub fn with_streams(streams: &[Track]) -> Self {
        let mut muxer = FragmentedMp4Muxer {
            video: None,
            audio: None,
            start_times: HashMap::new(),
            prev_times: HashMap::new(),
            track_mapping: HashMap::new(),
            io: Io::null(),
            seq: 0,
        };

        muxer.assign_streams(streams);

        muxer
    }

    pub fn new(io: Io) -> Self {
        FragmentedMp4Muxer {
            video: None,
            audio: None,
            start_times: HashMap::new(),
            prev_times: HashMap::new(),
            track_mapping: HashMap::new(),
            io,
            seq: 0,
        }
    }

    fn create(io: Io) -> Box<dyn Muxer> {
        Box::new(Self::new(io))
    }

    pub fn initialization_segment(&self) -> anyhow::Result<Span> {
        let mut buf = BytesMut::new();

        write_box!(&mut buf, b"ftyp", {
            buf.extend_from_slice(b"isom\0\0\0\0isomiso5dash");
        });

        write_box!(&mut buf, b"moov", {
            write_box!(&mut buf, b"mvhd", {
                buf.put_u32(1 << 24); // version
                buf.put_u64(0); // creation_time
                buf.put_u64(0); // modification_time
                buf.put_u32(1_000); // timescale
                buf.put_u64(0);
                buf.put_u32(0x00010000); // rate
                buf.put_u16(0x0100); // volume
                buf.put_u16(0); // reserved
                buf.put_u64(0); // reserved
                for v in &[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
                    buf.put_u32(*v); // matrix
                }
                for _ in 0..6 {
                    buf.put_u32(0); // pre_defined
                }
                buf.put_u32(2); // next_track_id
            });
            write_box!(&mut buf, b"mvex", {
                write_box!(&mut buf, b"mehd", {
                    buf.put_u32(1 << 24); // version
                    buf.put_u64(0); // duration
                });
                if let Some(video) = &self.video {
                    write_box!(&mut buf, b"trex", {
                        buf.put_u32(0 << 24); // version
                        buf.put_u32(self.track_mapping[&video.id]); // track_id
                        buf.put_u32(1); // sample_description
                        buf.put_u32(0); // default_duration,
                        buf.put_u32(0); // default_size,
                        buf.put_u32(0); // default_flags,
                    });
                }
                if let Some(audio) = &self.audio {
                    write_box!(&mut buf, b"trex", {
                        buf.put_u32(0 << 24); // version
                        buf.put_u32(self.track_mapping[&audio.id]); // track_id
                        buf.put_u32(1); // sample_description
                        buf.put_u32(0); // default_duration,
                        buf.put_u32(0); // default_size,
                        buf.put_u32(0); // default_flags,
                    });
                }
            });

            if let Some(video) = &self.video {
                let builder = TrackBuilder::new(video.clone(), self.track_mapping[&video.id]);
                write_video_trak(&mut buf, builder)?;
            }
            if let Some(audio) = &self.audio {
                let builder = TrackBuilder::new(audio.clone(), self.track_mapping[&audio.id]);
                write_audio_trak(&mut buf, builder)?;
            }
        });

        Ok(buf.freeze().into())
    }

    pub fn write_media_segment(&mut self, packet: Packet) -> anyhow::Result<Span> {
        let prev_time = self
            .prev_times
            .entry(packet.track.id)
            .or_insert_with(|| packet.time.clone());
        let start_time = self
            .start_times
            .entry(packet.track.id)
            .or_insert_with(|| packet.time.clone());

        let media_duration = packet.time.clone() - prev_time.clone();
        let base_offset = prev_time.clone() - start_time.clone();

        let track_id = self.track_mapping[&packet.track.id];

        let duration = if media_duration.duration == 0 {
            packet.guess_duration().unwrap_or_else(|| {
                MediaDuration::from_duration(Duration::from_millis(16), packet.track.timebase)
            })
        } else {
            media_duration
        };

        // let duration = duration.in_base(Fraction::new(1, 90_000));

        let duration = duration.duration;

        let mut buf = BytesMut::new();
        let data_offset_pos;

        write_box!(&mut buf, b"moof", {
            write_box!(&mut buf, b"mfhd", {
                buf.put_u32(0 << 24); // version
                buf.put_u64(self.seq); // creation_time
            });

            write_box!(&mut buf, b"traf", {
                write_box!(&mut buf, b"tfhd", {
                    let flags = 0x0200_00; // base_is_moof
                    buf.put_u32(flags); // version, flags
                    buf.put_u32(track_id); // track_id
                });
                write_box!(&mut buf, b"trun", {
                    let flags = 0x0000_01 | // offset_present
                        0x0000_04 | // first_flags_present
                        0x0001_00 | // duration_present
                        0x0002_00; // size_present
                    buf.put_u32(flags); // version, flags
                    buf.put_u32(1); // sample_len

                    data_offset_pos = buf.len();
                    buf.put_u32(0); // data_offset
                    buf.put_u32(if packet.key { 0x10000 } else { 0 }); // first_sample_flags
                    buf.put_u32(duration as u32);
                    buf.put_u32(packet.buffer.len() as _);
                });
                write_box!(&mut buf, b"tfdt", {
                    buf.put_u32(1 << 24); // version
                    buf.put_u64(base_offset.duration as u64); // decode_time
                });
            });
        });

        let len = (buf.len() as u32 + 8).to_be_bytes();
        buf[data_offset_pos..(data_offset_pos + 4)].copy_from_slice(&len);

        let moof = buf.freeze();

        let mut mdat_header = BytesMut::new();
        mdat_header.put_u32(packet.buffer.len() as u32 + 8);
        mdat_header.extend_from_slice(b"mdat");
        let mdat_header = mdat_header.freeze();

        let sample_data = super::get_packet_sample_data(&packet);

        let segment = [moof.into(), mdat_header.into(), sample_data]
            .into_iter()
            .collect::<Span>();

        self.seq += 1;
        self.prev_times.insert(packet.track.id, packet.time);

        Ok(segment)
    }

    fn get_packet_time(&mut self, packet: &Packet) -> (MediaDuration, MediaDuration) {
        let prev_time = self
            .prev_times
            .entry(packet.track.id)
            .or_insert_with(|| packet.time.clone());
        let start_time = self
            .start_times
            .entry(packet.track.id)
            .or_insert_with(|| packet.time.clone());

        let media_duration = packet.time.clone() - prev_time.clone();
        let base_offset = prev_time.clone() - start_time.clone();

        let duration = if media_duration.duration == 0 {
            packet.guess_duration().unwrap_or_else(|| {
                MediaDuration::from_duration(Duration::from_millis(16), packet.track.timebase)
            })
        } else {
            media_duration
        };

        self.prev_times.insert(packet.track.id, packet.time.clone());

        (base_offset, duration)
    }

    pub fn write_many_media_segments(&mut self, packets: &[Packet]) -> anyhow::Result<Span> {
        // TODO: audio?
        let track_id = self.track_mapping[&packets[0].track.id];

        let mut buf = BytesMut::new();
        let data_offset_pos;

        write_box!(&mut buf, b"moof", {
            write_box!(&mut buf, b"mfhd", {
                buf.put_u32(0 << 24); // version
                buf.put_u32(self.seq as u32); // sequence_id
            });

            write_box!(&mut buf, b"traf", {
                write_box!(&mut buf, b"tfhd", {
                    let flags = 0x0200_00; // base_is_moof
                    buf.put_u32(flags); // version, flags
                    buf.put_u32(track_id); // track_id
                });
                write_box!(&mut buf, b"trun", {
                    let flags = 0x0000_01 | // offset_present
                        0x0001_00 | // duration_present
                        0x0002_00 | // size_present
                        0x0004_00; // sample_flags_prsent
                    buf.put_u32(flags); // version, flags
                    buf.put_u32(packets.len() as u32); // sample_len

                    data_offset_pos = buf.len();
                    buf.put_u32(0); // data_offset
                    for pkt in packets {
                        let (_base_offset, duration) = self.get_packet_time(&pkt);
                        let track_id = self.track_mapping[&pkt.track.id];
                        let duration = duration.duration;

                        buf.put_u32(duration as u32);
                        buf.put_u32(pkt.buffer.len() as _);
                        buf.put_u32(if pkt.key { 0x10000 } else { 0 }); // first_sample_flags
                    }
                });
                write_box!(&mut buf, b"tfdt", {
                    buf.put_u32(1 << 24); // version
                    buf.put_u64(0); // decode_time
                });
            });
        });

        let len = (buf.len() as u32 + 8).to_be_bytes();
        buf[data_offset_pos..(data_offset_pos + 4)].copy_from_slice(&len);

        let moof = buf.freeze();

        let mut mdat_header = BytesMut::new();
        mdat_header.put_u32(packets.iter().map(|p| p.buffer.len()).sum::<usize>() as u32 + 8);
        mdat_header.extend_from_slice(b"mdat");
        let mdat_header = mdat_header.freeze();

        let sample_data = packets.iter().map(|packet| match packet.track.info.kind {
            MediaKind::Video(VideoInfo {
                codec:
                    VideoCodec::H264(H264Codec {
                        bitstream_format, ..
                    }),
                ..
            }) => convert_bitstream(
                packet.buffer.clone(),
                bitstream_format,
                BitstreamFraming::FourByteLength,
            ),
            _ => packet.buffer.clone(),
        });

        let segment = [moof.into(), mdat_header.into()]
            .into_iter()
            .chain(sample_data)
            .collect::<Span>();

        Ok(segment)
    }

    fn assign_streams(&mut self, streams: &[Track]) {
        use crate::media::MediaTrackExt;

        let mut track_number = 1;
        if let Some(video) = streams.video() {
            self.track_mapping.insert(video.id, track_number);
            track_number += 1;

            self.video = Some(video.clone());
        }

        if let Some(audio) = streams.audio() {
            self.track_mapping.insert(audio.id, track_number);

            self.audio = Some(audio.clone());
        }

        debug!("Track mappings: {:?}", self.track_mapping);
    }
}

#[async_trait]
impl Muxer for FragmentedMp4Muxer {
    async fn start(&mut self, streams: Vec<Track>) -> anyhow::Result<()> {
        self.assign_streams(&streams);
        let init_segment = self.initialization_segment()?;

        self.io.write_span(init_segment).await?;

        Ok(())
    }

    async fn write(&mut self, packet: Packet) -> anyhow::Result<()> {
        if !self.track_mapping.contains_key(&packet.track.id) {
            return Ok(());
        }

        let media_segment = self.write_media_segment(packet)?;

        self.io.write_span(media_segment).await?;

        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    
    fn into_io(self) -> Io {
        self.io
    }
}
