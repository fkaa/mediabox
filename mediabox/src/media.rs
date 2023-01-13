use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::{
    codec::{nal::BitstreamFraming, SubtitleInfo},
    Fraction, Span,
};

/// Information about video media
#[derive(Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub codec_private: Span<'static>,
    pub bitstream_format: BitstreamFraming,
    // pub codec: VideoCodec,
}

impl VideoInfo {
    pub fn parameter_sets(&self) -> Option<Vec<u8>> {
        None
    }
}

impl fmt::Debug for VideoInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        todo!()
        /*match &self.codec {
            VideoCodec::H264(H264Codec { sps, .. }) => {
                use h264_reader::{
                    nal::sps::SeqParameterSet,
                    rbsp::{decode_nal, BitReader},
                };

                let sps_slice = sps.to_slice();
                let nal = decode_nal(&sps_slice[1..]).unwrap();

                let reader = BitReader::new(nal.as_ref());
                let sps = SeqParameterSet::from_bits(reader).unwrap();

                let aspect_ratio = sps
                    .vui_parameters
                    .as_ref()
                    .and_then(|vui| vui.aspect_ratio_info.as_ref().and_then(|a| a.get()));

                let frame_rate = sps.vui_parameters.as_ref().and_then(|vui| {
                    vui.timing_info
                        .as_ref()
                        .map(|t| Fraction::new(t.time_scale / 2, t.num_units_in_tick))
                });

                write!(
                    f,
                    "H264 ({:?}) {:?} {}x{}",
                    sps.profile(),
                    sps.chroma_info.chroma_format,
                    self.width,
                    self.height
                )?;

                let dar = Fraction::new(self.width, self.height).simplify();

                if let Some((a, b)) = aspect_ratio {
                    write!(
                        f,
                        " [DAR {}:{} SAR {}:{}]",
                        dar.numerator, dar.denominator, a, b
                    )?;
                } else {
                    write!(f, " [DAR {}:{}]", dar.numerator, dar.denominator)?;
                }

                if let Some(fps) = frame_rate {
                    write!(
                        f,
                        " {:.3} fps",
                        fps.numerator as f32 / fps.denominator as f32
                    )?;
                }

                Ok(())
            }
        }*/
    }
}

#[derive(Debug, Clone)]
pub struct AacCodec {
    pub extra: Vec<u8>,
}

/// Information about specific audio codecs
#[derive(Debug, Clone)]
pub enum AudioCodec {
    Aac(AacCodec),
}

impl AudioCodec {
    pub fn decoder_specific_data(&self) -> Option<&[u8]> {
        match self {
            Self::Aac(AacCodec { extra }) => Some(&extra),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SoundType {
    Mono,
    Stereo,
}

impl Default for SoundType {
    fn default() -> Self {
        SoundType::Mono
    }
}

impl SoundType {
    pub fn channel_count(&self) -> u16 {
        match self {
            SoundType::Mono => 1,
            SoundType::Stereo => 2,
        }
    }
}

/// Information about a piece of audio media
#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub sample_bpp: u32,
    pub sound_type: SoundType,
    pub codec: AudioCodec,
}

/// The kind of media
#[derive(Clone)]
pub enum MediaKind {
    Video(VideoInfo),
    Audio(AudioInfo),
    Subtitle(SubtitleInfo),
}

impl fmt::Debug for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MediaKind::Video(video) => write!(f, "{:?}", video),
            MediaKind::Audio(audio) => write!(f, "{:?}", audio),
            MediaKind::Subtitle(subtitle) => write!(f, "{:?}", subtitle),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum CodecId {
    Unknown,
    H264,
    Aac,
    WebVtt,
}

impl CodecId {
    pub fn is_video(&self) -> bool {
        use CodecId::*;

        matches!(self, H264)
    }

    pub fn is_audio(&self) -> bool {
        use CodecId::*;

        matches!(self, Aac)
    }

    pub fn is_subtitle(&self) -> bool {
        use CodecId::*;

        matches!(self, WebVtt)
    }
}

impl Default for CodecId {
    fn default() -> Self {
        CodecId::Unknown
    }
}

/// Defines properties about a type of media (eg. a video or audio track)
#[derive(Default, Clone)]
pub struct MediaInfo {
    pub codec_id: CodecId,
    pub codec_private: Span<'static>,

    // video specific
    pub width: u32,
    pub height: u32,

    // audio specific
    pub sample_freq: u32,
    pub channels: u32,
    pub sound_type: SoundType,
    // pub kind: MediaKind,
}

impl fmt::Debug for MediaInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.codec_id)
    }
}

/// Description of a media track.
#[derive(Clone)]
pub struct Track {
    pub id: u32,
    pub info: Arc<MediaInfo>,
    pub timebase: Fraction,
}

impl fmt::Debug for Track {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{}: {:?}", self.id, self.info)
    }
}

impl Track {
    pub fn is_video(&self) -> bool {
        self.info.codec_id.is_video()
    }
}

/// A media packet.
///
/// A packet contains timestamped opaque data for a given track.
#[derive(Clone)]
pub struct Packet<'a> {
    pub time: MediaTime,
    pub key: bool,
    pub track: Track,
    pub buffer: Span<'a>,
}

pub type OwnedPacket = Packet<'static>;

impl<'a> Packet<'a> {
    pub fn guess_duration(&self) -> Option<MediaDuration> {
        /*match &self.track.info.kind {
            MediaKind::Video(VideoInfo {
                codec: VideoCodec::H264(H264Codec { sps, .. }),
                ..
            }) => {
                use h264_reader::{
                    nal::sps::SeqParameterSet,
                    rbsp::{decode_nal, BitReader},
                };

                let sps_slice = sps.to_slice();
                let nal = decode_nal(&sps_slice[1..]).unwrap();

                let reader = BitReader::new(nal.as_ref());
                let sps = SeqParameterSet::from_bits(reader).unwrap();

                let frame_rate = sps.vui_parameters.as_ref().and_then(|vui| {
                    vui.timing_info
                        .as_ref()
                        .map(|t| Fraction::new(t.time_scale / 2, t.num_units_in_tick))
                });

                frame_rate.map(|fps| {
                    let fps = fps.denominator as f64 / fps.numerator as f64;
                    let duration = Duration::from_nanos((1_000_000_000f64 * fps) as u64);

                    MediaDuration::from_duration(duration, self.track.timebase)
                })
            }
            _ => None,
        }*/
        None
    }
}

impl<'a> fmt::Debug for Packet<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Packet")
            .field("time", &self.time)
            .field("key", &self.key)
            .field(
                "track",
                &format_args!("{} ({:?})", self.track.id, self.track.info.codec_id),
            )
            .field("buffer", &format_args!("[{}]", self.buffer.len()))
            .finish()
    }
}

/// A media duration.
#[derive(Clone)]
pub struct MediaDuration {
    pub duration: i64,
    pub timebase: Fraction,
}

impl MediaDuration {
    pub fn from_duration(duration: Duration, timebase: Fraction) -> Self {
        MediaDuration {
            duration: convert_timebase(
                (duration.as_secs_f64() * 1_000_000_000f64) as u64,
                Fraction::new(1, 1_000_000_000),
                timebase,
            ) as i64,
            timebase,
        }
    }

    pub fn in_base(&self, timebase: Fraction) -> Self {
        MediaDuration {
            duration: convert_timebase(self.duration as u64, self.timebase, timebase) as i64,
            timebase,
        }
    }
}

impl fmt::Debug for MediaDuration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", Duration::from(self.clone()))?;

        Ok(())
    }
}

impl From<MediaDuration> for Duration {
    fn from(t: MediaDuration) -> std::time::Duration {
        std::time::Duration::from_nanos(
            (1_000_000_000f64 * (t.duration as f64 / t.timebase.denominator as f64)) as u64,
        )
    }
}

/// A media time.
///
/// Any piece of media needs to have a time. Some media can be delivered "out-of-order" (eg.
/// B-frames) which requires a media time to have two timestamps; a presentation time (pts) and a
/// decode time (dts).
#[derive(Clone)]
pub struct MediaTime {
    pub pts: u64,
    pub dts: Option<u64>,
    pub duration: Option<u64>,
    pub timebase: Fraction,
}

impl std::ops::Sub for &MediaTime {
    type Output = MediaDuration;

    fn sub(self, rhs: &MediaTime) -> Self::Output {
        MediaDuration {
            duration: self.pts as i64 - rhs.pts as i64,
            timebase: self.timebase,
        }
    }
}

impl std::ops::Sub for MediaTime {
    type Output = MediaDuration;

    fn sub(self, rhs: MediaTime) -> Self::Output {
        MediaDuration {
            duration: self.pts as i64 - rhs.pts as i64,
            timebase: self.timebase,
        }
    }
}

impl fmt::Debug for MediaTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let pts = self.pts as f32 / self.timebase.denominator as f32;
        write!(f, "{pts:.3}s")?;

        if let Some(duration) = self.duration {
            write!(
                f,
                "-{:.3}s ",
                pts + duration as f32 / self.timebase.denominator as f32
            )?;
        }

        if let Some(dts) = self.dts {
            write!(
                f,
                "{:.3}s (decode) ",
                dts as f32 / self.timebase.denominator as f32
            )?
        }

        Ok(())
    }
}

impl MediaTime {
    pub fn since(&self, rhs: &MediaTime) -> MediaDuration {
        MediaDuration {
            duration: self.pts as i64 - rhs.pts as i64,
            timebase: self.timebase,
        }
    }

    pub fn in_base(&self, new_timebase: Fraction) -> MediaTime {
        let pts = convert_timebase(self.pts, self.timebase, new_timebase);
        let dts = self
            .dts
            .map(|ts| convert_timebase(ts, self.timebase, new_timebase));
        let duration = self
            .duration
            .map(|ts| convert_timebase(ts, self.timebase, new_timebase));

        MediaTime {
            pts,
            dts,
            duration,
            timebase: new_timebase,
        }
    }
}

fn convert_timebase(time: u64, original: Fraction, new: Fraction) -> u64 {
    time * new.denominator as u64 / original.denominator as u64
}

#[test]
fn con_test() {
    assert_eq!(
        1000,
        convert_timebase(500, Fraction::new(1, 500), Fraction::new(1, 1000))
    );
}
