use std::fs;

use async_trait::async_trait;
use bytes::BytesMut;

use crate::{
    format::{
        mkv::{EBML_DOC_TYPE, EBML_DOC_TYPE_VERSION},
        Muxer, Muxer2, Movie, MuxerError,
    },
    Packet,
    Span,
    io::Io,
    muxer, OwnedPacket, Track,
};

use super::*;

muxer!("mkv", MatroskaMuxer::create);

#[derive(Default)]
pub struct MatroskaMuxer {
}

impl MatroskaMuxer {
}

impl Muxer2 for MatroskaMuxer {
    fn start(&mut self, movie: Movie) -> Result<Span, MuxerError> {
        let header = EbmlMasterElement(
            EBML_HEADER,
            &[
                EbmlElement(EBML_VERSION, EbmlValue::UInt(1)),
                EbmlElement(EBML_READ_VERSION, EbmlValue::UInt(1)),
                EbmlElement(EBML_DOC_MAX_ID_LENGTH, EbmlValue::UInt(4)),
                EbmlElement(EBML_DOC_MAX_SIZE_LENGTH, EbmlValue::UInt(8)),
                EbmlElement(EBML_DOC_TYPE, EbmlValue::String("matroska".into())),
                EbmlElement(EBML_DOC_TYPE_VERSION, EbmlValue::UInt(1)),
            ],
        );

        todo!()
    }
    fn write(&mut self, packet: Packet) -> Result<Span, MuxerError> {
        todo!()
    }
    fn stop(&mut self) -> Result<Span, MuxerError> {
        todo!()
    }
}
