use aho_corasick::AhoCorasick;
use anyhow::Context;
use async_trait::async_trait;

use log::*;
use nom::{combinator::opt, bytes::streaming::take, number::streaming::{be_i16, u8}, IResult};

use std::io::SeekFrom;

use crate::{
    format::{Demuxer2, DemuxerError},
    io::Buffered, MediaTime,
};

use super::ebml::*;
use super::*;

use crate::{
    demuxer,
    format::{Demuxer, Movie, ProbeResult},
    io::Io,
    Fraction, MediaInfo, Packet, Track,
};

demuxer!("mkv", MatroskaDemuxer::create, MatroskaDemuxer::probe);

pub struct MatroskaDemuxer {
    movie: Movie,
    timebase: Fraction,
    current_cluster_ts: u64,
    state: State,
}

#[derive(Eq, PartialEq)]
enum State {
    LookingForEbmlHeader,
    LookingForSegment,
    ParseUntilFirstCluster { tracks: bool, info: bool },
    ParseClusters,
}

macro_rules! element {
    ($dst: expr, $ebml: expr, $input: expr) => {
        if $dst.is_none() {
            *$dst = opt($ebml)($input)?.1;
        }
    };
}

#[derive(Clone, Debug, Default)]
struct MkvSimpleBlock<'a> {
    track_number: u64,
    timestamp: i16,
    flags: u8,
    buffer: &'a [u8],
}

fn read_simple_block<'a>(input: &'a [u8]) -> IResult<&'a [u8], MkvSimpleBlock<'a>, EbmlError> {
    let (input, track_number) = ebml_vint(input)?;
    let (input, timestamp) = be_i16(input)?;
    let (input, flags) = u8(input)?;

    let buffer = input;

    Ok((input, MkvSimpleBlock {
        track_number,
        timestamp,
        flags,
        buffer,
    }))
}

fn read_ebml_header(input: &[u8]) -> Result<&[u8], DemuxerError> {
    #[derive(Clone, Debug, Default)]
    struct EbmlHeader<'a> {
        version: Option<u64>,
        read_version: Option<u64>,
        max_id_length: Option<u64>,
        max_size_length: Option<u64>,
        doc_type: Option<&'a str>,
        doc_type_version: Option<u64>,
        doc_type_read_version: Option<u64>,
    }

    dbg!(input.len());
    let header_result =
        ebml_master_element_fold(EBML_HEADER, EbmlHeader::default(), |acc, input| {
            element!(&mut acc.version, ebml_uint(EBML_VERSION), input);
            element!(&mut acc.read_version, ebml_uint(EBML_READ_VERSION), input);
            element!(
                &mut acc.max_id_length,
                ebml_uint(EBML_DOC_MAX_ID_LENGTH),
                input
            );
            element!(
                &mut acc.max_size_length,
                ebml_uint(EBML_DOC_MAX_SIZE_LENGTH),
                input
            );
            element!(&mut acc.doc_type, ebml_str(EBML_DOC_TYPE), input);
            element!(
                &mut acc.doc_type_version,
                ebml_uint(EBML_DOC_TYPE_VERSION),
                input
            );
            element!(
                &mut acc.doc_type_read_version,
                ebml_uint(EBML_DOC_TYPE_READ_VERSION),
                input
            );

            Ok(())
        })(input);

    match header_result {
        Ok((remaining, header)) => {
            dbg!(header);

            Ok(remaining)
        }
        Err(nom::Err::Error(EbmlError::UnexpectedElement(expected, id, len))) => Err(
            DemuxerError::Misc(anyhow::anyhow!("Expected EBML header, found {id:?}")),
        ),
        Err(e) => Err(e.into()),
    }
}

fn read_until_segment(input: &[u8]) -> Result<&[u8], DemuxerError> {
    let (remaining, (id, len)) = ebml_element_header()(input)?;

    let len = len
        .require()
        .context("Found element with unknown size before segment")?;

    if id != SEGMENT {
        let header_len = slice_dist(input, remaining);

        return Err(DemuxerError::Seek(SeekFrom::Current(
            (header_len + len) as i64,
        )));
    }

    Ok(remaining)
}

const TRACK_TYPE_VIDEO: u64 = 1;
const TRACK_TYPE_AUDIO: u64 = 2;
const TRACK_TYPE_SUBTITLE: u64 = 17;

fn convert_track(track: &MkvTrack) -> anyhow::Result<(u64, MediaInfo)> {
    let number = mand(track.number, TRACK_NUMBER)?;
    let ty = mand(track.ty, TRACK_TYPE)?;
    let codec_id = mand(track.number, CODEC_ID)?;

    let mut info = MediaInfo::default();

    match ty {
        self::TRACK_TYPE_VIDEO => fill_video_info(&mut info, track)?,
        self::TRACK_TYPE_AUDIO => fill_audio_info(&mut info, track)?,
        self::TRACK_TYPE_SUBTITLE => fill_subtitle_info(&mut info, track)?,
        _ => anyhow::bail!("Unsupported track type {ty}"),
    }

    Ok((number, info))
}

fn fill_video_info(info: &mut MediaInfo, track: &MkvTrack) -> anyhow::Result<()> {
    Ok(())
}

fn fill_audio_info(info: &mut MediaInfo, track: &MkvTrack) -> anyhow::Result<()> {
    Ok(())
}

fn fill_subtitle_info(info: &mut MediaInfo, track: &MkvTrack) -> anyhow::Result<()> {
    Ok(())
}

enum MkvTrackType {
    Video,
    Audio,
    Subtitle,
}

#[derive(Clone, Debug, Default)]
struct MkvTrack<'a> {
    number: Option<u64>,
    uid: Option<u64>,
    ty: Option<u64>,
    codec_id: Option<&'a str>,
    codec_private: Option<&'a [u8]>,
    audio: Option<MkvAudio>,
}

#[derive(Clone, Debug, Default)]
struct MkvInfo {
    scale: Option<u64>,
}

fn parse_info(input: &[u8]) -> Result<(&[u8], MkvInfo), DemuxerError> {
    Ok(ebml_master_element_fold(
        INFO,
        MkvInfo::default(),
        |acc, input| {
            element!(&mut acc.scale, ebml_uint(TIMESTAMP_SCALE), input);

            Ok(())
        },
    )(input)?)
}

fn parse_tracks(input: &[u8]) -> Result<(&[u8], Vec<MkvTrack>), DemuxerError> {
    Ok(ebml_master_element_fold(
        TRACKS,
        Vec::new(),
        |acc, input| {
            if let Ok(track) = parse_track(input) {
                acc.push(track);
            }

            Ok(())
        },
    )(input)?)
}

#[derive(Clone, Debug, Default)]
struct MkvAudio {
    sampling_frequency: Option<f64>,
    channels: Option<u64>,
    bit_depth: Option<u64>,
}

fn parse_track(input: &[u8]) -> Result<MkvTrack, DemuxerError> {
    Ok(
        ebml_master_element_fold(TRACK_ENTRY, MkvTrack::default(), |acc, input| {
            element!(&mut acc.number, ebml_uint(TRACK_NUMBER), input);
            element!(&mut acc.uid, ebml_uint(TRACK_UID), input);
            element!(&mut acc.ty, ebml_uint(TRACK_TYPE), input);
            element!(&mut acc.codec_id, ebml_str(CODEC_ID), input);
            element!(&mut acc.codec_private, ebml_bin(CODEC_PRIVATE), input);
            element!(
                &mut acc.audio,
                ebml_master_element_fold(AUDIO, MkvAudio::default(), |acc, input| {
                    element!(
                        &mut acc.sampling_frequency,
                        ebml_float(SAMPLING_FREQUENCY),
                        input
                    );
                    element!(&mut acc.channels, ebml_uint(CHANNELS), input);
                    element!(&mut acc.bit_depth, ebml_uint(BIT_DEPTH), input);
                    Ok(())
                }),
                input
            );

            Ok(())
        })(input)?
        .1,
    )
}

impl MatroskaDemuxer {
    fn read_packet_internal<'a>(&mut self, input: &'a [u8]) -> Result<(&'a [u8], Option<Packet>), DemuxerError> {
        let (remaining, (id, len)) = ebml_element_header()(input)?;

        match id {
            self::CLUSTER => {
                Ok((remaining, None))
            }
            self::CUES => {
                Err(DemuxerError::EndOfStream)
            }
            self::TIMESTAMP => {
                let (remaining, time) = ebml_uint(TIMESTAMP)(input)?;

                self.current_cluster_ts = time;

                Ok((remaining, None))
            },
            self::SIMPLE_BLOCK => {
                let len = len.require().context("Expected simple block to have known length")?;

                let (remaining, bytes) = take(len)(remaining)?;
                let (_, blk) = read_simple_block(bytes)?;

                let packet = self.convert_block_to_packet(blk);

                Ok((remaining, packet))
            }
            _ => {
                // eprintln!("{id:?}");
                // TODO: error instead?
                // if we dont recognize the element, skip
                let len = len
                    .require()
                    .context("Found element with unknown size while parsing clusters")?;

                let header_len = slice_dist(input, remaining);

                Err(DemuxerError::Seek(SeekFrom::Current(
                    (header_len + len) as i64,
                )))
            }
        }
    }

    fn read_headers_internal<'a>(&mut self, input: &'a [u8]) -> Result<&'a [u8], DemuxerError> {
        match self.state {
            State::LookingForEbmlHeader => {
                let remaining = read_ebml_header(input)?;

                self.state = State::LookingForSegment;

                Ok(remaining)
            }
            State::LookingForSegment => {
                let remaining = read_until_segment(input)?;

                self.state = State::ParseUntilFirstCluster { tracks: false, info: false };

                Ok(remaining)
            }
            State::ParseUntilFirstCluster { tracks, info } => {

                let (remaining, id) = self.read_segment_elements(input)?;

                let tracks = tracks || id == TRACKS;
                let info = info || id == INFO;

                if tracks && info {
                    self.state = State::ParseClusters;
                } else {
                    self.state = State::ParseUntilFirstCluster { tracks, info };
                }

                Ok(remaining)
            }
            State::ParseClusters => {
                todo!("shouldnt come here")
            }
        }
    }

    fn read_segment_elements<'a, 'b>(
        &'b mut self,
        input: &'a [u8],
    ) -> Result<(&'a [u8], EbmlId), DemuxerError> {
        let (remaining, (id, len)) = ebml_element_header()(input)?;

        let len = len
            .require()
            .context("Found element with unknown size before info")?;

        match id {
            self::INFO => {
                let (remaining, info) = parse_info(input)?;

                let scale = info.scale.unwrap_or(1000000);

                self.timebase = Fraction::new(1, (scale / 1000) as u32);

                Ok((remaining, INFO))
            }
            self::TRACKS => {
                let (remaining, tracks) = parse_tracks(input)?;

                convert_tracks(self.timebase, &mut self.movie, &tracks)?;

                Ok((remaining, TRACKS))
            }

            _ => {
                let header_len = slice_dist(input, remaining);

                Err(DemuxerError::Seek(SeekFrom::Current(
                    (header_len + len) as i64,
                )))
            }
        }
    }

    fn convert_block_to_packet(&self, blk: MkvSimpleBlock) -> Option<Packet> {
        let track = self.movie.tracks.iter().find(|t| t.id == blk.track_number as u32).cloned()?;

        let time = MediaTime {
            pts: self.current_cluster_ts.checked_add_signed(blk.timestamp as i64).unwrap_or(0),
            dts: None,
            duration: None,
            timebase: track.timebase,
        };

        let key = (blk.flags & 0b1000_0000) != 0;

        let buffer = blk.buffer.to_vec().into();

        Some(Packet { time, key, track, buffer })
    }
}

fn convert_tracks(
    timebase: Fraction,
    movie: &mut Movie,
    mkv_tracks: &[MkvTrack],
) -> anyhow::Result<()> {
    for track in mkv_tracks {
        match convert_track(track) {
            Ok((id, info)) => movie.tracks.push(Track {
                id: id as u32,
                info: info.into(),
                timebase: timebase.clone(),
            }),
            Err(e) => {
                warn!("Ignoring track: {e}");
            }
        }
    }

    Ok(())
}

impl Demuxer2 for MatroskaDemuxer {
    fn read_headers(&mut self, buf: &mut dyn Buffered) -> Result<Movie, DemuxerError> {
        loop {
            let input = buf.data();

            let remaining = self.read_headers_internal(input)?;
            buf.consume(slice_dist(input, remaining) as usize);

            if self.state == State::ParseClusters {
                return Ok(self.movie.clone());
            }
        }
    }

    fn read_packet(&mut self, buf: &mut dyn crate::io::Buffered) -> Result<Option<Packet>, DemuxerError> {
        loop {
            let input = buf.data();

            let (remaining, packet) = self.read_packet_internal(input)?;
            buf.consume(slice_dist(input, remaining) as usize);

            if let Some(packet) = packet {
                return Ok(Some(packet));
            }
        } 
    }
}

impl MatroskaDemuxer {
    pub fn new2() -> Self {
        MatroskaDemuxer {
            movie: Movie::default(),
            timebase: Fraction::new(1, 1),
            current_cluster_ts: 0,
            state: State::LookingForEbmlHeader,
        }
    }
    pub fn new(io: Io) -> Self {
        todo!()
    }

    /*
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
    }*/
}

struct Audio {
    sampling_frequency: f64,
    channels: u64,
    bit_depth: Option<u64>,
}

#[async_trait(?Send)]
impl Demuxer for MatroskaDemuxer {
    async fn start(&mut self) -> anyhow::Result<Movie> {
        /*self.parse_ebml_header()
            .await
            .context("Parsing EBML header")?;
        self.find_tracks().await.context("Finding tracks")?;

        Ok(Movie {
            tracks: self.streams.clone(),
            attachments: Vec::new(),
        })*/
        todo!()
    }

    async fn read(&mut self) -> anyhow::Result<Packet> {
        /*loop {
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
        }*/
        todo!()
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn create(io: Io) -> Box<dyn Demuxer> {
        Box::new(Self::new(io))
    }

    fn probe(data: &[u8]) -> ProbeResult {
        let patterns = &[
            &EBML_HEADER.0.to_be_bytes()[..],
            b"matroska",
            &SEGMENT.0.to_be_bytes()[..],
            &CLUSTER.0.to_be_bytes()[..],
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

fn mand<T>(value: Option<T>, id: EbmlId) -> Result<T, MkvError> {
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

fn slice_dist(a: &[u8], b: &[u8]) -> u64 {
    let a = a.as_ptr() as u64;
    let b = b.as_ptr() as u64;

    if a > b {
        a - b
    } else {
        b - a
    }
}
