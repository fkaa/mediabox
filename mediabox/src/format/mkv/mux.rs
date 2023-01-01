use std::fs;

use async_trait::async_trait;
use bytes::BytesMut;

use crate::{
    format::{
        mkv::{
            EBML_DOC_TYPE, EBML_DOC_TYPE_VERSION,
        },
        Muxer,
    },
    io::Io,
    muxer, Packet, Track,
};


use super::*;

muxer!("mkv", MatroskaMuxer::create);

pub struct MatroskaMuxer {
    video: Option<Track>,
    audio: Option<Track>,
    io: Io,
}

impl MatroskaMuxer {
    pub fn new(io: Io) -> Self {
        MatroskaMuxer {
            video: None,
            audio: None,
            io,
        }
    }

    fn create(io: Io) -> Box<dyn Muxer> {
        Box::new(Self::new(io))
    }
}

#[async_trait]
impl Muxer for MatroskaMuxer {
    async fn start(&mut self, streams: Vec<Track>) -> anyhow::Result<()> {
        let mut buf = BytesMut::new();

        let header = EbmlMasterElement(
            EbmlId(EBML_HEADER),
            vec![
                EbmlElement(EbmlId(EBML_VERSION), EbmlValue::UInt(1)),
                EbmlElement(EbmlId(EBML_READ_VERSION), EbmlValue::UInt(1)),
                EbmlElement(EbmlId(EBML_DOC_MAX_ID_LENGTH), EbmlValue::UInt(4)),
                EbmlElement(EbmlId(EBML_DOC_MAX_SIZE_LENGTH), EbmlValue::UInt(8)),
                EbmlElement(EbmlId(EBML_DOC_TYPE), EbmlValue::String("matroska".into())),
                EbmlElement(EbmlId(EBML_DOC_TYPE_VERSION), EbmlValue::UInt(1)),
            ],
        );

        header.write(&mut buf);

        EbmlId(SEGMENT).write(&mut buf);
        EbmlLength::Unknown(1).write(&mut buf);

        fs::write("test.mkv", &buf).unwrap();

        self.io.write(&buf).await?;

        Ok(())
    }

    async fn write(&mut self, packet: Packet) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn into_io(self) -> Io {
        self.io
    }
}
