use crate::{
    format::{
        mkv::{EBML_DOC_TYPE, EBML_DOC_TYPE_VERSION},
        Movie, Muxer2, MuxerError, ScratchMemory,
    },
    muxer, Packet, Span,
};

use super::*;

muxer!("mkv", MatroskaMuxer::create);

#[derive(Default)]
pub struct MatroskaMuxer {}

impl MatroskaMuxer {}

impl Muxer2 for MatroskaMuxer {
    fn start(&mut self, scratch: &mut ScratchMemory, movie: Movie) -> Result<Span, MuxerError> {
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
    fn write(&mut self, scratch: &mut ScratchMemory, packet: Packet) -> Result<Span, MuxerError> {
        todo!()
    }
    fn stop(&mut self) -> Result<Span, MuxerError> {
        todo!()
    }
}
