use aho_corasick::AhoCorasick;
use anyhow::Context;
use async_trait::async_trait;
use h264_reader::avcc::AvcDecoderConfigurationRecord;
use log::*;

use std::{sync::Arc, io::SeekFrom};

use crate::{ebml, format::{Demuxer2, DemuxerResponse}, io::Buffered};

use super::ebml::*;
use super::*;

use crate::{
    codec::{nal::get_codec_from_mp4, AssCodec, SubtitleCodec, SubtitleInfo},
    demuxer,
    format::{Demuxer, Movie, ProbeResult},
    io::Io,
    AacCodec, AudioCodec, AudioInfo, Fraction, MediaInfo, MediaKind, MediaTime, Packet, SoundType,
    Track,
};

demuxer!("mkv", MatroskaDemuxer::create, MatroskaDemuxer::probe);

pub struct MatroskaDemuxer {
    io: Io,
    streams: Vec<Track>,
    timebase: Fraction,
    current_cluster_ts: u64,
}

enum State {
    LookingFor(EbmlId),
}

impl Demuxer2 for MatroskaDemuxer {
    fn read_headers(&mut self, buf: &mut dyn Buffered) -> anyhow::Result<DemuxerResponse> {
        let input = buf.data();
        let (input, id) = ebml_vid(input)?;
        let (input, len) = ebml_len(input)?;

        if id == EbmlId(EBML_HEADER) {

        } else {
            match len {
                EbmlLength::Known(len) => {
                    return Ok(DemuxerResponse::Seek(SeekFrom::Current(len as i64)));
                }
                EbmlLength::Unknown(_) => {
                    anyhow::bail!("Unknown length when looking for movie");
                }
            }
        }

        todo!()
    }

    fn read_packet(&mut self, buf: &mut dyn crate::io::Buffered) -> anyhow::Result<crate::format::DemuxerResponse> {
        todo!()
    }
}

impl MatroskaDemuxer {
    pub fn new(io: Io) -> Self {
        MatroskaDemuxer {
            io,
            streams: Vec::new(),
            timebase: Fraction::new(1, 1),
            current_cluster_ts: 0,
        }
    }

    async fn parse_ebml_header(&mut self) -> Result<(), MkvError> {
        let (_, id) = vid(&mut self.io).await?;
        let (_, size) = vint(&mut self.io).await?;

        if id != EBML_HEADER {
            return Err(MkvError::UnexpectedId(EBML_HEADER, id));
        }

        ebml!(&mut self.io, size,
            (self::EBML_DOC_TYPE, size) => {
                let doc_type = vstr(&mut self.io, size).await?;

                debug!("DocType: {doc_type}");
            },
            (self::EBML_DOC_TYPE_VERSION, size) => {
                let version = vu(&mut self.io, size).await?;

                debug!("DocTypeVersion: {version}");
            },
            (self::EBML_DOC_TYPE_READ_VERSION, size) => {
                let version = vu(&mut self.io, size).await?;

                debug!("DocTypeReadVersion: {version}");
            }
        );

        Ok(())
    }

    async fn find_tracks(&mut self) -> Result<(), MkvError> {
        let (_, id) = vid(&mut self.io).await?;
        let (_, size) = vint(&mut self.io).await?;

        if id != SEGMENT {
            return Err(MkvError::UnexpectedId(SEGMENT, id));
        }

        ebml!(&mut self.io, size,
            (self::INFO, size) => {
                self.parse_segment_info(size).await?;
            },
            (self::TRACKS, size) => {
                self.parse_track_entries(size).await?;
                break;
            }
        );

        Ok(())
    }

    async fn parse_segment_info(&mut self, size: u64) -> Result<(), MkvError> {
        ebml!(&mut self.io, size,
            (self::TIMESTAMP_SCALE, size) => {
                let scale = vu(&mut self.io, size).await?;

                self.timebase = Fraction::new(1, scale as u32 / 1000);
            }
        );

        Ok(())
    }

    async fn parse_track_entries(&mut self, size: u64) -> Result<(), MkvError> {
        ebml!(&mut self.io, size,
            (self::TRACK_ENTRY, size) => {
                self.parse_track_entry(size).await?;
            }
        );

        Ok(())
    }

    async fn parse_track_entry(&mut self, size: u64) -> Result<(), MkvError> {
        let mut track_number = None;
        // let mut track_type = None;
        let mut codec_id = None;
        let mut codec_private = None;
        let mut audio = None;

        ebml!(&mut self.io, size,
            (self::TRACK_NUMBER, size) => {
                track_number = Some(vu(&mut self.io, size).await?);
            },
            /*(self::TRACK_UID, size) => {
                let uid = vu(&mut self.io, size).await?;

                debug!("TrackUID: {uid:016x}");
            },*/
            /*(self::TRACK_TYPE, size) => {
                track_type = Some(vu(&mut self.io, size).await?);
            },*/
            (self::CODEC_ID, size) => {
                codec_id = Some(vstr(&mut self.io, size).await?);
            },
            (self::CODEC_PRIVATE, size) => {
                codec_private = Some(vbin(&mut self.io, size).await?);
            },
            (self::AUDIO, size) => {
                audio = Some(self.parse_audio(size).await?);
            }
        );

        let track_number = mand(track_number, TRACK_NUMBER)?;
        let codec_id = mand(codec_id, CODEC_ID)?;

        let info = match codec_id.as_str() {
            "S_TEXT/ASS" => {
                let codec_private = mand(codec_private, CODEC_PRIVATE)?;
                let header = String::from_utf8(codec_private)?;

                debug!("{header}");

                MediaInfo {
                    name: "ass",
                    kind: MediaKind::Subtitle(SubtitleInfo {
                        codec: SubtitleCodec::Ass(AssCodec { header }),
                    }),
                }
            }
            "V_MPEG4/ISO/AVC" => {
                let codec_private = mand(codec_private, CODEC_PRIVATE)?;

                let avc_record: AvcDecoderConfigurationRecord = codec_private
                    .as_slice()
                    .try_into()
                    .map_err(|e| anyhow::anyhow!("{:?}", e))?;

                get_codec_from_mp4(&avc_record).unwrap()
            }
            "A_AAC" => {
                let audio = mand(audio, AUDIO)?;
                let codec_private = mand(codec_private, CODEC_PRIVATE)?;

                MediaInfo {
                    name: "aac",
                    kind: MediaKind::Audio(AudioInfo {
                        sample_rate: audio.sampling_frequency as u32,
                        sample_bpp: audio.bit_depth.unwrap_or(8) as u32,
                        sound_type: if audio.channels > 1 {
                            SoundType::Stereo
                        } else {
                            SoundType::Mono
                        },
                        codec: AudioCodec::Aac(AacCodec {
                            extra: codec_private,
                        }),
                    }),
                }
            }
            _ => {
                warn!("Unsupported codec {codec_id:?}");
                return Ok(());
            }
        };

        let stream = Track {
            id: track_number as u32,
            info: Arc::new(info),
            timebase: self.timebase,
        };

        self.streams.push(stream);

        Ok(())
    }

    async fn parse_audio(&mut self, size: u64) -> Result<Audio, MkvError> {
        let mut sampling_frequency = None;
        let mut channels = None;
        let mut bit_depth = None;

        ebml!(&mut self.io, size,
            (self::SAMPLING_FREQUENCY, size) => {
                sampling_frequency = Some(vfloat(&mut self.io, size).await?);
            },
            (self::CHANNELS, size) => {
                channels = Some(vu(&mut self.io, size).await?);
            },
            (self::BIT_DEPTH, size) => {
                bit_depth = Some(vu(&mut self.io, size).await?);
            }
        );

        let sampling_frequency =
            sampling_frequency.ok_or(MkvError::MissingElement(SAMPLING_FREQUENCY))?;
        let channels = channels.ok_or(MkvError::MissingElement(CHANNELS))?;

        Ok(Audio {
            sampling_frequency,
            channels,
            bit_depth,
        })
    }

    async fn parse_video(&mut self, size: u64) -> Result<(), MkvError> {
        ebml!(&mut self.io, size,
            (self::TRACK_NUMBER, size) => {
            }
        );

        Ok(())
    }

    async fn read_block(&mut self, size: u64) -> Result<Option<Packet>, MkvError> {
        use tokio::io::AsyncReadExt;

        let (len, track_number) = vint(&mut self.io).await?;

        let Some(track) = self.streams.iter().find(|s| s.id == track_number as u32).cloned() else {
            self.io.skip(size - len as u64).await?;

            return Ok(None);
        };

        let reader = self.io.reader()?;
        let timestamp = reader.read_u16().await?;
        let flags = reader.read_u8().await?;

        let key = (flags & 0b1000_0000) != 0;

        let mut buffer = vec![0u8; size as usize - len as usize - 3];
        reader.read_exact(&mut buffer).await?;

        let time = MediaTime {
            pts: self.current_cluster_ts + timestamp as u64,
            dts: None,
            duration: None,
            timebase: self.timebase,
        };

        Ok(Some(Packet {
            time,
            track,
            key,
            buffer: buffer.into(),
        }))
    }
}

struct Audio {
    sampling_frequency: f64,
    channels: u64,
    bit_depth: Option<u64>,
}

#[async_trait(?Send)]
impl Demuxer for MatroskaDemuxer {
    async fn start(&mut self) -> anyhow::Result<Movie> {
        self.parse_ebml_header()
            .await
            .context("Parsing EBML header")?;
        self.find_tracks().await.context("Finding tracks")?;

        Ok(Movie {
            tracks: self.streams.clone(),
            attachments: Vec::new(),
        })
    }

    async fn read(&mut self) -> anyhow::Result<Packet> {
        loop {
            let (_, id) = vid(&mut self.io).await?;
            let (_, size) = vint(&mut self.io).await?;

            match id {
                self::CLUSTER => {
                    continue;
                }
                self::TIMESTAMP => {
                    self.current_cluster_ts = vu(&mut self.io, size).await?;
                    trace!("cluster_ts: {}", self.current_cluster_ts);
                }
                self::BLOCK_GROUP => {
                    let mut pkt = None;
                    let mut block_duration = None;

                    ebml!(&mut self.io, size,
                        (BLOCK, size) => {
                            pkt = self.read_block(size).await?;
                        },
                        (BLOCK_DURATION, size) => {
                            block_duration = Some(vu(&mut self.io, size).await?);
                        }
                    );

                    if let Some(mut pkt) = pkt {
                        pkt.time.duration = block_duration;

                        return Ok(pkt);
                    }
                }
                self::SIMPLE_BLOCK => {
                    if let Some(pkt) = self.read_block(size).await? {
                        return Ok(pkt);
                    }
                }
                _ => {
                    trace!("Ignoring element 0x{id:08x} ({size} B)");
                    self.io.skip(size).await?;
                }
            }
        }
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn create(io: Io) -> Box<dyn Demuxer> {
        Box::new(Self::new(io))
    }

    fn probe(data: &[u8]) -> ProbeResult {
        let patterns = &[
            &EBML_HEADER.to_be_bytes()[..],
            b"matroska",
            &SEGMENT.to_be_bytes()[..],
            &CLUSTER.to_be_bytes()[..],
        ];
        let ac = AhoCorasick::new(patterns);

        let mut score = 0f32;
        for mat in ac.find_iter(data) {
            score += 0.25;
        }

        if score >= 1.0 {
            ProbeResult::Yup
        } else {
            ProbeResult::Maybe(score)
        }
    }
}

fn mand<T>(value: Option<T>, id: u64) -> Result<T, MkvError> {
    value.ok_or(MkvError::MissingElement(id))
}

async fn vbin(io: &mut Io, size: u64) -> Result<Vec<u8>, MkvError> {
    let mut data = vec![0u8; size as usize];

    io.read_exact(&mut data).await?;

    Ok(data)
}

async fn be16(io: &mut Io) -> Result<i16, MkvError> {
    let mut data = [0u8; 2];

    io.read_exact(&mut data).await?;

    Ok(i16::from_be_bytes(data))
}
