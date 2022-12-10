use std::{collections::VecDeque, io::Write, sync::Arc};

use crate::{encoder, Fraction, MediaInfo, MediaKind, Track};

use super::*;

const WEBVTT_TIMEBASE: Fraction = Fraction::new(1, 1000);

encoder!("webvtt", WebVttEncoder::create);

pub struct WebVttEncoder {
    track: Option<Track>,
    queue: VecDeque<Packet>,
    cue_index: usize,
}

impl WebVttEncoder {
    pub fn new() -> Self {
        WebVttEncoder {
            track: None,
            queue: VecDeque::new(),
            cue_index: 0,
        }
    }

    fn create() -> Box<dyn Encoder> {
        Box::new(Self::new())
    }
}

impl Default for WebVttEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder for WebVttEncoder {
    fn start(&mut self, desc: CodecDescription) -> anyhow::Result<Track> {
        let info = SubtitleInfo {
            codec: SubtitleCodec::WebVtt(WebVttCodec { header: "".into() }),
        };

        let track = Track {
            id: 0,
            info: Arc::new(MediaInfo {
                name: "webvtt",
                kind: MediaKind::Subtitle(info),
            }),
            timebase: WEBVTT_TIMEBASE,
        };

        self.track = Some(track.clone());

        Ok(track)
    }

    fn feed(&mut self, raw: Decoded) -> anyhow::Result<()> {
        let cue = raw
            .into_subtitle()
            .ok_or_else(|| anyhow::anyhow!("Expected text cue"))?;

        let time = cue.time;
        let timebase = time.timebase;
        let begin_seconds = time.pts as f32 / timebase.denominator as f32;
        let duration_seconds = time
            .duration
            .ok_or_else(|| anyhow::anyhow!("Expected duration for subtitle"))?
            as f32
            / timebase.denominator as f32;
        let end_seconds = begin_seconds + duration_seconds;

        let begin = WebVttTime::from(begin_seconds);
        let end = WebVttTime::from(end_seconds);

        let mut text = Vec::new();

        writeln!(&mut text, "{}", self.cue_index)?;
        writeln!(&mut text, "{begin} --> {end}")?;
        for part in cue.text {
            match part {
                TextPart::Text(txt) => {
                    for b in txt.into_bytes() {
                        match b {
                            // TODO: probably need &nbsp; as well...
                            b'&' => text.extend(b"&amp;"),
                            b'<' => text.extend(b"&lt;"),
                            b'>' => text.extend(b"&gt;"),
                            _ => {
                                text.push(b);
                            }
                        }
                    }
                }
                TextPart::SmartBreak => {
                    text.push(b'\n');
                }
                // TODO: add styling
                _ => {}
            }
        }
        writeln!(&mut text)?;

        let pkt = Packet {
            time,
            key: true,
            track: self.track.clone().expect("Encoder not started"),
            buffer: text.into(),
        };

        self.cue_index += 1;
        self.queue.push_back(pkt);

        Ok(())
    }

    fn receive(&mut self) -> Option<Packet> {
        self.queue.pop_front()
    }
}

struct WebVttTime(u32, u8, u8, u16);

impl From<f32> for WebVttTime {
    fn from(val: f32) -> Self {
        let h = (val / 3600.0).floor();
        let m = (val / 60.0).floor() % 60.0;
        let s = (val % 60.0).floor() % 60.0;
        let ms = (val.fract() * 1000.0).round();

        WebVttTime(h as u32, m as u8, s as u8, ms as u16)
    }
}

impl fmt::Display for WebVttTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let WebVttTime(h, m, s, ms) = self;

        write!(f, "{h:02}:{m:02}:{s:02}.{ms:03}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use test_case::test_case;

    #[test_case(0.001, "00:00:00.001")]
    #[test_case(0.01, "00:00:00.010")]
    #[test_case(0.1, "00:00:00.100")]
    #[test_case(1.0, "00:00:01.000")]
    #[test_case(1.5, "00:00:01.500")]
    #[test_case(59.5, "00:00:59.500")]
    #[test_case(60.0, "00:01:00.000")]
    #[test_case(60.5, "00:01:00.500")]
    #[test_case(3600.0, "01:00:00.000")]
    #[test_case(3600.5, "01:00:00.500")]
    #[test_case(3600.0 * 9.0, "09:00:00.000")]
    #[test_case(3600.0 * 11.0, "11:00:00.000")]
    #[test_case(3600.0 * 100.0, "100:00:00.000")]
    fn cue_time_format(seconds: f32, expected: &str) {
        let time: WebVttTime = seconds.into();

        assert_eq!(&format!("{time}"), expected);
    }
}
