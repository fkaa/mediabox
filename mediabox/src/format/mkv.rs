use aho_corasick::AhoCorasick;
use anyhow::Context;
use async_trait::async_trait;
use h264_reader::avcc::AvcDecoderConfigurationRecord;
use log::*;

use std::sync::Arc;

use crate::{
    codec::{nal::get_codec_from_mp4, AssCodec, SubtitleCodec, SubtitleInfo},
    demuxer,
    format::{Demuxer, Movie},
    io::Io,
    AacCodec, AudioCodec, AudioInfo, Fraction, MediaInfo, MediaKind, MediaTime, Packet, SoundType,
    Track,
};

use super::{ProbeResult};

macro_rules! ebml {
    ($io:expr, $size:expr, $( $pat:pat_param => $blk:block ),* ) => {
        let mut i = 0;
        while i < $size {
            let (len, id) = vid($io).await?;
            i += len as u64;
            let (len, size) = vint($io).await?;
            i += len as u64;

            match (id, size) {
                $( $pat => $blk, )*
                _ => {
                    trace!("Ignoring element: 0x{id:08x} ({size} B)");

                    $io.skip(size).await?;
                }
            }

            i += size;
        }
    }
}

demuxer!("mkv", MatroskaDemuxer::create, MatroskaDemuxer::probe);

pub struct MatroskaDemuxer {
    io: Io,
    streams: Vec<Track>,
    timebase: Fraction,
    current_cluster_ts: u64,
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

        let track = if let Some(track) = self.streams.iter().find(|s| s.id == track_number as u32) {
            track.clone()
        } else {
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
        self.parse_ebml_header().await.context("Parsing EBML header")?;
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

fn mand<T>(value: Option<T>, id: u32) -> Result<T, MkvError> {
    value.ok_or(MkvError::MissingElement(id))
}

#[derive(thiserror::Error, Debug)]
pub enum MkvError {
    #[error("Not enough data")]
    NotEnoughData,

    #[error("Unsupported variable integer size: {0}")]
    UnsupportedVint(u64),

    #[error("Unsupported variable integer ID: {0}")]
    UnsupportedVid(u8),

    #[error("Invalid float size: {0}")]
    InvalidFloatSize(u64),

    #[error("Expected 0x{0:08x} but found 0x{1:08x}")]
    UnexpectedId(u32, u32),

    #[error("No element 0x{0:08x} was found")]
    MissingElement(u32),

    #[error("Invalid UTF-8: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("{0}")]
    Io(#[from] crate::io::IoError),

    #[error("{0}")]
    StdIo(#[from] std::io::Error),

    // lazy
    #[error("{0}")]
    Misc(#[from] anyhow::Error),
}

const EBML_HEADER: u32 = 0x1a45dfa3;
const EBML_DOC_TYPE: u32 = 0x4282;
const EBML_DOC_TYPE_VERSION: u32 = 0x4287;
const EBML_DOC_TYPE_READ_VERSION: u32 = 0x4285;
const SEGMENT: u32 = 0x18538067;
const SEEK_HEAD: u32 = 0x114d9b74;
const SEEK: u32 = 0x4dbb;
const SEEK_ID: u32 = 0x53ab;
const SEEK_POSITION: u32 = 0x53ac;
const INFO: u32 = 0x1549a966;
const TIMESTAMP_SCALE: u32 = 0x2ad7b1;
const DURATION: u32 = 0x4489;
const DATE_UTC: u32 = 0x4461;
const TRACKS: u32 = 0x1654ae6b;
const TRACK_ENTRY: u32 = 0xae;
const TRACK_NUMBER: u32 = 0xd7;
const TRACK_UID: u32 = 0x73c5;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63a2;
const VIDEO: u32 = 0xe0;
const AUDIO: u32 = 0xe1;
const SAMPLING_FREQUENCY: u32 = 0xb5;
const CHANNELS: u32 = 0x9f;
const BIT_DEPTH: u32 = 0x6264;
const CLUSTER: u32 = 0x1f43b675;
const TIMESTAMP: u32 = 0xe7;
const SIMPLE_BLOCK: u32 = 0xa3;
const BLOCK_GROUP: u32 = 0xa0;
const BLOCK: u32 = 0xa1;
const BLOCK_DURATION: u32 = 0x9b;

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

async fn vstr(io: &mut Io, size: u64) -> Result<String, MkvError> {
    let mut data = vec![0u8; size as usize];

    io.read_exact(&mut data).await?;

    Ok(String::from_utf8(data)?)
}

async fn vfloat(io: &mut Io, size: u64) -> Result<f64, MkvError> {
    let mut data = [0u8; 8];

    let value = match size {
        0 => 0.0,
        4 => {
            io.read_exact(&mut data[..4]).await?;

            f32::from_be_bytes(data[..4].try_into().unwrap()) as f64
        }
        8 => {
            io.read_exact(&mut data[..8]).await?;

            f64::from_be_bytes(data)
        }
        _ => return Err(MkvError::InvalidFloatSize(size)),
    };

    Ok(value)
}

async fn vu(io: &mut Io, size: u64) -> Result<u64, MkvError> {
    if size > 8 {
        return Err(MkvError::UnsupportedVint(size));
    }

    let mut data = [0u8; 8];
    io.read_exact(&mut data[..size as usize]).await?;

    let mut value = 0u64;
    for i in 0..size {
        value <<= 8;
        value |= data[i as usize] as u64;
    }

    Ok(value)
}

async fn vint(io: &mut Io) -> Result<(u8, u64), MkvError> {
    use tokio::io::AsyncReadExt;

    let reader = io.reader()?;

    let byte = reader.read_u8().await?;
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 7 {
        return Err(MkvError::UnsupportedVint(extra_bytes as u64));
    }

    let mut bytes = [0u8; 7];
    if extra_bytes > 0 {
        reader
            .read_exact(&mut bytes[..extra_bytes as usize])
            .await?;
    }

    let mut value = byte as u64 & ((1 << (8 - len)) - 1) as u64;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= bytes[i as usize] as u64;
    }

    Ok((len as u8, value))
}

async fn vid(io: &mut Io) -> Result<(u8, u32), MkvError> {
    use tokio::io::AsyncReadExt;

    let reader = io.reader()?;

    let byte = reader.read_u8().await?;
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 3 {
        return Err(MkvError::UnsupportedVid(extra_bytes));
    }

    let mut bytes = [0u8; 3];
    if extra_bytes > 0 {
        reader
            .read_exact(&mut bytes[..extra_bytes as usize])
            .await?;
    }

    let mut value = byte as u32;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= bytes[i as usize] as u32;
    }

    Ok((len as u8, value))
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;
    use std::io::Cursor;
    use test_case::test_case;

    #[test_case(&[0b1000_0010], 2)]
    #[test_case(&[0b0100_0000, 0b0000_0010], 2)]
    #[test_case(&[0b0010_0000, 0b0000_0000, 0b0000_0010], 2)]
    #[test_case(&[0b0001_0000, 0b0000_0000, 0b0000_0000, 0b0000_0010], 2)]
    #[tokio::test]
    async fn vint(bytes: &[u8], expected: u64) {
        let cursor = Cursor::new(bytes.to_vec());
        let mut io = Io::from_reader(Box::new(cursor));

        let value = super::vint(&mut io).await;

        assert_matches!(value, Ok(expected));
    }
}
