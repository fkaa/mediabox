use std::{
    io::{self, Write},
    sync::Arc,
};

use aho_corasick::AhoCorasick;
use nom::{
    bytes::streaming::{is_not, take_until},
    character::streaming::line_ending,
};

use crate::{
    buffer::Buffered, codec::ass::parse_line, demuxer, muxer, CodecId, Fraction, MediaInfo,
    MediaTime, Packet, Span, Track,
};

use super::{Demuxer2, DemuxerError, Movie, Muxer2, MuxerError, ProbeResult, ScratchMemory};

muxer!("ass", AssMuxer::create);
demuxer!("ass", AssDemuxer::create, AssDemuxer::probe);

#[derive(Default)]
pub struct AssMuxer {}

impl Muxer2 for AssMuxer {
    fn start(&mut self, scratch: &mut ScratchMemory, movie: &Movie) -> Result<Span, MuxerError> {
        let track = movie
            .tracks
            .iter()
            .find(|t| t.info.codec_id == CodecId::Ass)
            .unwrap();

        Ok(track.info.codec_private.clone())
    }
    fn write(
        &mut self,
        scratch: &mut ScratchMemory,
        packet: &Packet<'static>,
    ) -> Result<Span, MuxerError> {
        let slice = packet.buffer.to_slice();

        let first_comma = slice.iter().position(|&c| c == b',').unwrap() + 1;
        let second_comma = first_comma
            + &slice[first_comma..]
                .iter()
                .position(|&c| c == b',')
                .unwrap();

        let first_part = &slice[first_comma..second_comma + 1];
        let second_part = &slice[second_comma..];

        let buffer_len = first_part.len()
            + second_part.len()
            + b"Dialogue: ".len()
            + b"0:00:00:00,0:00:00:00".len()
            + b"\r\n".len();
        //dbg!(buffer_len);
        let span = scratch.write(buffer_len, |mut buf| {
            //dbg!(buf.len());
            buf.write_all(b"Dialogue: ").unwrap();
            //dbg!(buf.len());
            buf.write_all(first_part).unwrap();
            //dbg!(buf.len());
            write_ass_time_range(&mut buf, packet.time.clone()).unwrap();
            //dbg!(b"0:00:00:00,0:00:00:00".len());

            //dbg!(buf.len());
            buf.write_all(second_part).unwrap();
            //dbg!(buf.len());
            buf.write_all(b"\r\n").unwrap();
            //dbg!(buf.len());
            buf
        })?;

        Ok(span)
    }
    fn stop(&mut self) -> Result<Span, MuxerError> {
        todo!()
    }
}

fn write_ass_time_range(writer: &mut dyn Write, time: MediaTime) -> io::Result<()> {
    // dbg!(&time);
    let start = time.pts_in_seconds();
    let end = start + time.duration_in_seconds().unwrap();

    write_ass_time(writer, start)?;
    write!(writer, ",")?;
    write_ass_time(writer, end)?;

    Ok(())
}

fn write_ass_time(writer: &mut dyn Write, seconds: f64) -> io::Result<()> {
    // dbg!(seconds);
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let seconds = seconds % 60.0;
    let hundreths = (seconds.fract() * 100.0) as u32;
    let seconds = seconds as u32;

    // println!("{hours}:{minutes:02}:{seconds:02}:{hundreths:02}");
    write!(writer, "{hours}:{minutes:02}:{seconds:02}:{hundreths:02}")
}

#[derive(Default)]
pub struct AssDemuxer {
    track: Option<Track>,
}

impl Demuxer2 for AssDemuxer {
    fn read_headers(
        &mut self,
        input: &[u8],
        buf: &mut dyn Buffered,
    ) -> Result<Movie, DemuxerError> {
        let (remaining, codec_private) = take_until(&b"[Events]"[..])(input)?;
        let (remaining, _) = take_until(&b"\r\n\r\n"[..])(remaining)?;
        buf.consume(slice_dist(input, &remaining[4..]) as usize);

        let codec_private = codec_private.to_vec().into();

        let track = Track {
            id: 1,
            info: Arc::new(MediaInfo {
                codec_id: CodecId::Ass,
                codec_private,
                ..Default::default()
            }),
            timebase: Fraction::new(1, 1000),
        };

        let movie = Movie {
            tracks: vec![track.clone()],
            attachments: Vec::new(),
        };
        self.track = Some(track);

        Ok(movie)
    }

    fn read_packet<'a>(
        &mut self,
        mut input: &'a [u8],
        buf: &mut dyn Buffered,
    ) -> Result<Option<Packet<'a>>, DemuxerError> {
        loop {
            // let line_string =
            // std::str::from_utf8(input).map_err(|e| DemuxerError::Misc(e.into()))?;
            // dbg!(line_string);

            let (remaining, line) = is_not("\r\n")(input)?;
            let (remaining, _) = line_ending(remaining)?;

            buf.consume(slice_dist(input, remaining) as usize);

            let line_string =
                std::str::from_utf8(line).map_err(|e| DemuxerError::Misc(e.into()))?;

            if let Ok((_, ass_line)) = parse_line(line_string) {
                let track = self.track.clone().unwrap();

                let pkt = Packet {
                    time: ass_line.time.unwrap(),
                    key: true,
                    track,
                    buffer: Span::Slice(line),
                };

                return Ok(Some(pkt));
            }

            input = remaining;
        }
    }

    fn probe(data: &[u8]) -> ProbeResult {
        let patterns = &[&b"[Script Info]"[..], &b"aegisub"[..]];
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

fn slice_dist(a: &[u8], b: &[u8]) -> u64 {
    let a = a.as_ptr() as u64;
    let b = b.as_ptr() as u64;

    if a > b {
        a - b
    } else {
        b - a
    }
}
