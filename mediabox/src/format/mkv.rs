mod ebml;
use ebml::*;

mod demux;
mod mux;

pub use demux::*;
pub use mux::*;

const EBML_HEADER: EbmlId = EbmlId(0x1a45dfa3);
const EBML_DOC_TYPE: EbmlId = EbmlId(0x4282);
const EBML_DOC_MAX_ID_LENGTH: EbmlId = EbmlId(0x42F2);
const EBML_DOC_MAX_SIZE_LENGTH: EbmlId = EbmlId(0x42F3);
const EBML_READ_VERSION: EbmlId = EbmlId(0x42F7);
const EBML_VERSION: EbmlId = EbmlId(0x4286);
const EBML_DOC_TYPE_VERSION: EbmlId = EbmlId(0x4287);
const EBML_DOC_TYPE_READ_VERSION: EbmlId = EbmlId(0x4285);
const SEGMENT: EbmlId = EbmlId(0x18538067);
const SEEK_HEAD: EbmlId = EbmlId(0x114d9b74);
const SEEK: EbmlId = EbmlId(0x4dbb);
const SEEK_ID: EbmlId = EbmlId(0x53ab);
const SEEK_POSITION: EbmlId = EbmlId(0x53ac);
const INFO: EbmlId = EbmlId(0x1549a966);
const WRITING_APP: EbmlId = EbmlId(0x4d80);
const MUXING_APP: EbmlId = EbmlId(0x5741);
const TIMESTAMP_SCALE: EbmlId = EbmlId(0x2ad7b1);
const DURATION: EbmlId = EbmlId(0x4489);
const DATE_UTC: EbmlId = EbmlId(0x4461);
const TRACKS: EbmlId = EbmlId(0x1654ae6b);
const TRACK_ENTRY: EbmlId = EbmlId(0xae);
const TRACK_NUMBER: EbmlId = EbmlId(0xd7);
const TRACK_UID: EbmlId = EbmlId(0x73c5);
const TRACK_TYPE: EbmlId = EbmlId(0x83);
const CODEC_ID: EbmlId = EbmlId(0x86);
const CODEC_PRIVATE: EbmlId = EbmlId(0x63a2);
const VIDEO: EbmlId = EbmlId(0xe0);
const PIXEL_WIDTH: EbmlId = EbmlId(0xb0);
const PIXEL_HEIGHT: EbmlId = EbmlId(0xba);
const FLAG_INTERLACED: EbmlId = EbmlId(0x9a);
const AUDIO: EbmlId = EbmlId(0xe1);
const SAMPLING_FREQUENCY: EbmlId = EbmlId(0xb5);
const CHANNELS: EbmlId = EbmlId(0x9f);
const BIT_DEPTH: EbmlId = EbmlId(0x6264);
const CLUSTER: EbmlId = EbmlId(0x1f43b675);
const TIMESTAMP: EbmlId = EbmlId(0xe7);
const SIMPLE_BLOCK: EbmlId = EbmlId(0xa3);
const BLOCK_GROUP: EbmlId = EbmlId(0xa0);
const BLOCK: EbmlId = EbmlId(0xa1);
const BLOCK_DURATION: EbmlId = EbmlId(0x9b);
const CUES: EbmlId = EbmlId(0x1c53bb6b);

#[derive(thiserror::Error, Debug)]
pub enum MkvError {
    #[error("Not enough data")]
    NotEnoughData,

    #[error("Unsupported variable integer size: {0}")]
    UnsupportedVint(u64),

    #[error("Unsupported variable integer ID: {0}")]
    UnsupportedVid(u8),

    #[error("Invalid float size: {0}")]
    InvalidFloatSize(u64),

    #[error("Expected 0x{0:08x} but found 0x{1:08x}")]
    UnexpectedId(u64, u64),

    #[error("No element {0:?} was found")]
    MissingElement(EbmlId),

    #[error("Invalid UTF-8: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("{0}")]
    Io(#[from] crate::io::IoError),

    #[error("{0}")]
    StdIo(#[from] std::io::Error),

    // lazy
    #[error("{0}")]
    Misc(#[from] anyhow::Error),
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use test_case::test_case;

    use crate::{
        format::mux::Muxer2,
        format::SyncMuxerContext,
        io::Io,
        test::{self, TestFile},
    };

    use super::{MatroskaDemuxer, MatroskaMuxer};

    test_files! {
        #[test]
        fn write_read_packets_are_equal(test_file: TestFile) {
            let (movie, packets) = test::read_mkv_from_path(test_file.path);
            println!("read movie");

            let mut muxer = SyncMuxerContext::open_with_writer(MatroskaMuxer::create(), ).unwrap();
            /*let io = Io::from_stream(Box::new(Vec::<u8>::new()));
            let mut muxer = MatroskaMuxer::new(io);

            test::write_movie_and_packets(&mut muxer, movie, &packets).await;
            println!("wrote movie");

            let buffer: Box<Vec<u8>> = muxer.into_io().into_writer().unwrap();
            let buffer = Box::new(Cursor::new(*buffer));
            let mut demuxer  = MatroskaDemuxer::new(Io::from_reader(buffer));

            let (new_movie, new_packets) = test::read_movie_and_packets(&mut demuxer).await;*/

            // TODO: assert
        }
    }
}
