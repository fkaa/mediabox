use crate::{
    format::{
        mkv::{EBML_DOC_TYPE, EBML_DOC_TYPE_VERSION},
        Movie, Muxer2, MuxerError, ScratchMemory,
    },
    muxer, Packet, Span, CodecId,
};

use super::*;

muxer!("mkv", MatroskaMuxer::create);

#[derive(Default)]
pub struct MatroskaMuxer {}

impl MatroskaMuxer {}

impl Muxer2 for MatroskaMuxer {
    fn start(&mut self, scratch: &mut ScratchMemory, movie: &Movie) -> Result<Span, MuxerError> {
        let children = [
            EbmlElement(EBML_VERSION, EbmlValue::UInt(1)),
            EbmlElement(EBML_READ_VERSION, EbmlValue::UInt(1)),
            EbmlElement(EBML_DOC_MAX_ID_LENGTH, EbmlValue::UInt(4)),
            EbmlElement(EBML_DOC_MAX_SIZE_LENGTH, EbmlValue::UInt(8)),
            EbmlElement(EBML_DOC_TYPE, EbmlValue::String("matroska")),
            EbmlElement(EBML_DOC_TYPE_VERSION, EbmlValue::UInt(1)),
        ];
        let header = EbmlMasterElement(EBML_HEADER, &children);

        let segment = SEGMENT;
        let segment_len = EbmlLength::Unknown(8);

        let info = EbmlMasterElement(INFO, &[]);

        let children = [
            EbmlElement(EBML_VERSION, EbmlValue::UInt(1)),
            EbmlElement(EBML_READ_VERSION, EbmlValue::UInt(1)),
            EbmlElement(EBML_DOC_MAX_ID_LENGTH, EbmlValue::UInt(4)),
            EbmlElement(EBML_DOC_MAX_SIZE_LENGTH, EbmlValue::UInt(8)),
            EbmlElement(EBML_DOC_TYPE, EbmlValue::String("matroska".into())),
            EbmlElement(EBML_DOC_TYPE_VERSION, EbmlValue::UInt(1)),
        ];
        let tracks = EbmlMasterElement(TRACKS, &[]);

        let total_size = header.full_size() + segment.size() + segment_len.size();

        let span = scratch.write(total_size as usize, |mut buf| {
            header.write(&mut buf);
            segment.write(&mut buf);
            segment_len.write(&mut buf);
        })?;

        Ok(span)
    }
    fn write(&mut self, scratch: &mut ScratchMemory, packet: &Packet) -> Result<Span, MuxerError> {
        todo!()
    }
    fn stop(&mut self) -> Result<Span, MuxerError> {
        todo!()
    }
}

pub fn make_length_span(
    length: EbmlLength,
    scratch: &mut ScratchMemory,
) -> Result<Span<'static>, MuxerError> {
    scratch.write(length.size() as _, |mut buf| length.write(&mut buf))
}

pub fn make_element(
    id: EbmlId,
    scratch: &mut ScratchMemory,
    content: Span<'static>,
) -> Result<Span<'static>, MuxerError> {
    let length = make_length_span(EbmlLength::Known(content.len() as _), scratch)?;

    Ok([length, content].into_iter().collect())
}

fn to_mkv_codec_id(id: CodecId) -> &'static str {
    match id {
        CodecId::H264 => "V_MPEG4/ISO/AVC",
        CodecId::Aac => "A_AAC",
        CodecId::WebVtt => "S_TEXT/WEBVTT",
        CodecId::Unknown => "unknown"
    }
}

fn get_tracks<'a>(
    movie: &'a Movie,
    scratch: &'a mut ScratchMemory,
) -> Result<Span<'static>, MuxerError> {
    let mut tracks = Vec::new();

    for track in &movie.tracks {
        let codec_id = track.info.codec_id;

        if codec_id.is_video() {

        }

        let children = [
            EbmlElement(TRACK_NUMBER, EbmlValue::UInt(track.id as u64)),
            EbmlElement(TRACK_UID, EbmlValue::UInt(track.id as u64)),
            EbmlElement(CODEC_ID, EbmlValue::String(to_mkv_codec_id(track.info.codec_id))),
        ];
        let element = EbmlMasterElement(
            TRACK_ENTRY,
            &children,
        );

        let content = scratch.write(element.full_size() as _, |mut buf| element.write(&mut buf))?;

        tracks.push(make_element(TRACK_ENTRY, scratch, content)?);
    }

    make_element(TRACKS, scratch, tracks.into_iter().collect());

    todo!()
}
