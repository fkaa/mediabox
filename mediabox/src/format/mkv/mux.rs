use std::mem::size_of;

use bytes::BufMut;

use crate::{
    format::{
        mkv::{EBML_DOC_TYPE, EBML_DOC_TYPE_VERSION},
        Movie, Muxer2, MuxerError, ScratchMemory,
    },
    memory::{Memory, MemoryPool, MemoryPoolConfig},
    muxer, CodecId, Packet, Span,
};

use super::*;

muxer!("mkv", MatroskaMuxer::create);

pub struct MatroskaMuxer {
    current_cluster: Vec<Span<'static>>,
    current_cluster_pts: u64,
    pool: MemoryPool,
    cluster_scratch_memory: Option<Memory>,
}

impl Default for MatroskaMuxer {
    fn default() -> Self {
        MatroskaMuxer {
            current_cluster: Vec::new(),
            current_cluster_pts: 0,
            pool: MemoryPool::new(MemoryPoolConfig {
                max_capacity: None,
                default_memory_capacity: 4096,
            }),
            cluster_scratch_memory: None,
        }
    }
}

impl Muxer2 for MatroskaMuxer {
    fn start(&mut self, scratch: &mut ScratchMemory, movie: &Movie) -> Result<Span, MuxerError> {
        let ebml_header = EbmlMasterElement(
            EBML_HEADER,
            &[
                EbmlElement(EBML_VERSION, EbmlValue::UInt(1)),
                EbmlElement(EBML_READ_VERSION, EbmlValue::UInt(1)),
                EbmlElement(EBML_DOC_MAX_ID_LENGTH, EbmlValue::UInt(4)),
                EbmlElement(EBML_DOC_MAX_SIZE_LENGTH, EbmlValue::UInt(8)),
                EbmlElement(EBML_DOC_TYPE, EbmlValue::String("matroska")),
                EbmlElement(EBML_DOC_TYPE_VERSION, EbmlValue::UInt(1)),
            ],
        );

        let segment = SEGMENT;
        let segment_len = EbmlLength::Unknown(8);

        let info = EbmlMasterElement(
            INFO,
            &[
                EbmlElement(WRITING_APP, EbmlValue::String(crate::NAME)),
                EbmlElement(MUXING_APP, EbmlValue::String(crate::NAME)),
            ],
        );

        let total_size =
            ebml_header.full_size() + segment.size() + segment_len.size() + info.full_size();

        let span = scratch.write(total_size as usize, |mut buf| {
            ebml_header.write(&mut buf);
            segment.write(&mut buf);
            segment_len.write(&mut buf);
            info.write(&mut buf);
        })?;

        let tracks = get_tracks(movie, scratch)?;

        Ok([span, tracks].into_iter().collect())
    }
    fn write(
        &mut self,
        scratch: &mut ScratchMemory,
        packet: &Packet<'static>,
    ) -> Result<Span, MuxerError> {
        let block = get_simple_block(packet, self.current_cluster_pts, scratch)?;

        self.current_cluster.push(block);

        todo!()
    }
    fn stop(&mut self) -> Result<Span, MuxerError> {
        todo!()
    }
}

pub fn make_id_span(id: EbmlId, scratch: &mut ScratchMemory) -> Result<Span<'static>, MuxerError> {
    scratch.write(id.size() as _, |mut buf| id.write(&mut buf))
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
    let id = make_id_span(id, scratch)?;
    let length = make_length_span(EbmlLength::Known(content.len() as _), scratch)?;

    Ok([id, length, content].into_iter().collect())
}

fn to_mkv_codec_id(id: CodecId) -> &'static str {
    match id {
        CodecId::H264 => "V_MPEG4/ISO/AVC",
        CodecId::Aac => "A_AAC",
        CodecId::WebVtt => "S_TEXT/WEBVTT",
        CodecId::Ass => "S_TEXT/ASS",
        CodecId::Unknown => "unknown",
    }
}

fn get_simple_block<'a>(
    packet: &'a Packet<'static>,
    current_cluster_pts: u64,
    scratch: &'a mut ScratchMemory,
) -> Result<Span<'static>, MuxerError> {
    let track_number = packet.track.id;
    let time = (current_cluster_pts - packet.time.pts) as i16;

    let size_required =
        vint_bytes_required(track_number as _) as usize + size_of::<i16>() + size_of::<u8>();

    let element_header = scratch.write(size_required, |mut buf| {
        write_vint(&mut buf, track_number as _);
        buf.put_i16(time);
        buf.put_u8(0);
    })?;

    let data = packet.buffer.clone();

    make_element(
        SIMPLE_BLOCK,
        scratch,
        [element_header, data].into_iter().collect(),
    )
}

fn get_tracks<'a>(
    movie: &'a Movie,
    scratch: &'a mut ScratchMemory,
) -> Result<Span<'static>, MuxerError> {
    let mut tracks = Vec::new();

    for track in &movie.tracks {
        let codec_id = track.info.codec_id;

        let mut children = vec![
            EbmlElement(TRACK_NUMBER, EbmlValue::UInt(track.id as u64)),
            EbmlElement(TRACK_UID, EbmlValue::UInt(track.id as u64)),
            EbmlElement(
                CODEC_ID,
                EbmlValue::String(to_mkv_codec_id(track.info.codec_id)),
            ),
            EbmlElement(
                CODEC_PRIVATE,
                EbmlValue::Binary(track.info.codec_private.clone()),
            ),
        ];

        let video_children = [
            EbmlElement(PIXEL_WIDTH, EbmlValue::UInt(track.info.width as u64)),
            EbmlElement(PIXEL_HEIGHT, EbmlValue::UInt(track.info.height as u64)),
            EbmlElement(FLAG_INTERLACED, EbmlValue::UInt(2)),
        ];

        if codec_id.is_video() {
            children.push(EbmlElement(VIDEO, EbmlValue::Children(&video_children)));
        }
        let element = EbmlMasterElement(TRACK_ENTRY, &children);

        let content = scratch.write(element.full_size() as _, |mut buf| element.write(&mut buf))?;

        tracks.push(content);
    }

    make_element(TRACKS, scratch, tracks.into_iter().collect())
}
