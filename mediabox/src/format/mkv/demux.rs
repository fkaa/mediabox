use aho_corasick::AhoCorasick;
use anyhow::Context;

use log::*;
use nom::{
    bytes::streaming::take,
    combinator::opt,
    number::streaming::{be_i16, u8},
    IResult,
};

use std::io::SeekFrom;

use crate::{
    buffer::Buffered,
    format::{Demuxer2, DemuxerError},
    CodecId, MediaTime, SoundType, Span,
};

use super::ebml::*;
use super::*;

use crate::{
    demuxer,
    format::{Movie, ProbeResult},
    Fraction, MediaInfo, Packet, Track,
};

demuxer!("mkv", MatroskaDemuxer::create, MatroskaDemuxer::probe);

pub struct MatroskaDemuxer {
    movie: Movie,
    timebase: Fraction,
    current_cluster_ts: u64,
    state: State,
}

impl Default for MatroskaDemuxer {
    fn default() -> Self {
        MatroskaDemuxer {
            movie: Movie::default(),
            timebase: Fraction::new(1, 1),
            current_cluster_ts: 0,
            state: State::LookingForEbmlHeader,
        }
    }
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

impl Demuxer2 for MatroskaDemuxer {
    fn read_headers(
        &mut self,
        mut input: &[u8],
        buf: &mut dyn Buffered,
    ) -> Result<Movie, DemuxerError> {
        loop {
            let remaining = self.read_headers_internal(input)?;
            buf.consume(slice_dist(input, remaining) as usize);

            input = remaining;

            if self.state == State::ParseClusters {
                return Ok(self.movie.clone());
            }
        }
    }

    fn read_packet<'a>(
        &mut self,
        mut input: &'a [u8],
        buf: &mut dyn Buffered,
    ) -> Result<Option<Packet<'a>>, DemuxerError> {
        loop {
            let (remaining, packet) = self.read_packet_internal(input)?;
            let dist = slice_dist(input, remaining) as usize;
            buf.consume(dist);

            input = remaining;

            if let Some(packet) = packet {
                return Ok(Some(packet));
            }
        }
    }

    fn probe(data: &[u8]) -> ProbeResult {
        let patterns = &[
            &EBML_HEADER.0.to_be_bytes()[..],
            b"matroska",
            &SEGMENT.0.to_be_bytes()[..],
            &CLUSTER.0.to_be_bytes()[..],
        ];
        let ac = AhoCorasick::new(patterns).unwrap();

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

impl MatroskaDemuxer {
    fn read_packet_internal<'a>(
        &mut self,
        input: &'a [u8],
    ) -> Result<(&'a [u8], Option<Packet<'a>>), DemuxerError> {
        let (remaining, (id, len)) = ebml_element_header()(input)?;

        match id {
            self::CLUSTER => Ok((remaining, None)),
            self::CUES => Err(DemuxerError::EndOfStream),
            self::TIMESTAMP => {
                let (remaining, time) = ebml_uint(TIMESTAMP)(input)?;

                self.current_cluster_ts = time;

                Ok((remaining, None))
            }
            self::BLOCK_GROUP => {
                let (remaining, packet) = self.parse_block_group(input)?;

                Ok((remaining, packet))
            }
            self::SIMPLE_BLOCK => {
                let len = len
                    .require()
                    .context("Expected simple block to have known length")?;
                let header_len = slice_dist(input, remaining);

                let old = remaining;
                let (remaining, header) = read_simple_block_header(remaining)?;
                let read = slice_dist(old, remaining);

                /*if header.track_number != 6 {
                    return Err(DemuxerError::Seek(SeekFrom::Current(
                        (header_len + len) as i64,
                    )));
                }*/

                let (remaining, buffer_bytes) = take(len - read)(remaining)?;

                let buffer = buffer_bytes;

                let packet = self.convert_block_to_packet(header, Span::Slice(buffer), None);

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

                self.state = State::ParseUntilFirstCluster {
                    tracks: false,
                    info: false,
                };

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
                for track in &mut self.movie.tracks {
                    track.timebase = self.timebase.clone();
                }

                Ok((remaining, INFO))
            }
            self::TRACKS => {
                let remaining = parse_tracks(input, self.timebase, &mut self.movie)?;

                // convert_tracks(self.timebase, &mut self.movie, &tracks)?;

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

    fn parse_block_group<'a>(
        &self,
        input: &'a [u8],
    ) -> IResult<&'a [u8], Option<Packet<'a>>, EbmlError> {
        let (remaining, block_group) =
            ebml_master_element_fold(BLOCK_GROUP, MkvBlockGroup::default(), |acc, input| {
                element!(&mut acc.block, ebml_match(BLOCK), input);
                element!(&mut acc.duration, ebml_uint(BLOCK_DURATION), input);

                Ok(())
            })(input)?;

        let block = block_group.block.unwrap();

        let (block_remaining, header) = read_simple_block_header(block)?;
        let buffer = block_remaining;

        let packet =
            self.convert_block_to_packet(header, Span::Slice(buffer), block_group.duration);

        Ok((remaining, packet))
    }

    fn convert_block_to_packet<'a>(
        &self,
        blk: MkvSimpleBlockHeader,
        buffer: Span<'a>,
        duration: Option<u64>,
    ) -> Option<Packet<'a>> {
        let track = self
            .movie
            .tracks
            .iter()
            .find(|t| t.id == blk.track_number as u32)
            .cloned()?;

        let time = MediaTime {
            pts: self
                .current_cluster_ts
                .checked_add_signed(blk.timestamp as i64)
                .unwrap_or(0),
            dts: None,
            duration: duration,
            timebase: track.timebase,
        };

        let key = (blk.flags & 0b1000_0000) != 0;

        Some(Packet {
            time,
            key,
            track,
            buffer,
        })
    }
}

#[derive(Clone, Debug, Default)]
struct MkvSimpleBlockHeader {
    track_number: u64,
    timestamp: i16,
    flags: u8,
}

#[derive(Clone, Default)]
struct MkvBlockGroup<'a> {
    block: Option<&'a [u8]>,
    duration: Option<u64>,
}

fn read_simple_block_header<'a>(
    input: &'a [u8],
) -> IResult<&'a [u8], MkvSimpleBlockHeader, EbmlError> {
    let (input, track_number) = ebml_vint(input)?;
    let (input, timestamp) = be_i16(input)?;
    let (input, flags) = u8(input)?;

    Ok((
        input,
        MkvSimpleBlockHeader {
            track_number,
            timestamp,
            flags,
        },
    ))
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
        Ok((remaining, header)) => Ok(remaining),
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

fn convert_track(track: MkvTrack) -> anyhow::Result<(u64, MediaInfo)> {
    let number = mand(track.number, TRACK_NUMBER)?;
    let ty = mand(track.ty, TRACK_TYPE)?;
    let codec_id = mand(track.codec_id, CODEC_ID)?;

    let mut info = MediaInfo::default();

    info.codec_id = convert_codec_id(codec_id);

    match ty {
        self::TRACK_TYPE_VIDEO => fill_video_info(&mut info, track)?,
        self::TRACK_TYPE_AUDIO => fill_audio_info(&mut info, mand(track.audio, AUDIO)?)?,
        self::TRACK_TYPE_SUBTITLE => fill_subtitle_info(&mut info, track)?,
        _ => anyhow::bail!("Unsupported track type {ty}"),
    }

    Ok((number, info))
}

fn convert_codec_id(name: &str) -> CodecId {
    match name {
        "V_MPEG4/ISO/AVC" => CodecId::H264,
        "A_AAC" => CodecId::Aac,
        "S_TEXT/WEBVTT" => CodecId::WebVtt,
        "S_TEXT/ASS" => CodecId::Ass,
        _ => {
            debug!("Unrecognized codec {name:?}");

            CodecId::Unknown
        }
    }
}

fn fill_video_info(info: &mut MediaInfo, track: MkvTrack) -> anyhow::Result<()> {
    let video = mand(track.video, VIDEO)?;

    info.width = mand(video.width, PIXEL_WIDTH)? as u32;
    info.height = mand(video.height, PIXEL_HEIGHT)? as u32;

    match info.codec_id {
        CodecId::H264 => {
            info.codec_private = Span::from(mand(track.codec_private, CODEC_PRIVATE)?.to_vec());
        }
        _ => {}
    }

    Ok(())
}

fn fill_audio_info(info: &mut MediaInfo, audio: MkvAudio) -> anyhow::Result<()> {
    info.sample_freq = mand(audio.sampling_frequency, SAMPLING_FREQUENCY)? as u32;
    info.sound_type = match mand(audio.channels, SAMPLING_FREQUENCY)? {
        1 => SoundType::Mono,
        2 => SoundType::Stereo,
        _ => SoundType::Unknown,
    };

    Ok(())
}

fn fill_subtitle_info(info: &mut MediaInfo, track: MkvTrack) -> anyhow::Result<()> {
    info.codec_private = Span::from(mand(track.codec_private, CODEC_PRIVATE)?.to_vec());
    Ok(())
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

fn parse_tracks<'a, 'b>(
    input: &'a [u8],
    timebase: Fraction,
    movie: &'b mut Movie,
) -> Result<&'a [u8], DemuxerError> {
    Ok(ebml_master_element_fold(TRACKS, (), |_, input| {
        if let Ok(track) = parse_track(input) {
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
    })(input)?
    .0)
}

#[derive(Clone, Debug, Default)]
struct MkvTrack<'a> {
    number: Option<u64>,
    uid: Option<u64>,
    ty: Option<u64>,
    codec_id: Option<&'a str>,
    codec_private: Option<&'a [u8]>,
    audio: Option<MkvAudio>,
    video: Option<MkvVideo>,
}

#[derive(Clone, Debug, Default)]
struct MkvAudio {
    sampling_frequency: Option<f64>,
    channels: Option<u64>,
    bit_depth: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct MkvVideo {
    width: Option<u64>,
    height: Option<u64>,
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
                &mut acc.video,
                ebml_master_element_fold(VIDEO, MkvVideo::default(), |acc, input| {
                    element!(&mut acc.width, ebml_uint(PIXEL_WIDTH), input);
                    element!(&mut acc.height, ebml_uint(PIXEL_HEIGHT), input);

                    Ok(())
                }),
                input
            );
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

fn mand<T>(value: Option<T>, id: EbmlId) -> Result<T, MkvError> {
    value.ok_or(MkvError::MissingElement(id))
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
