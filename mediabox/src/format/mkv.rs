mod ebml;
use ebml::*;

mod demux;
mod mux;

pub use demux::*;
pub use mux::*;

const EBML_HEADER: u64 = 0x1a45dfa3;
const EBML_DOC_TYPE: u64 = 0x4282;
const EBML_DOC_MAX_ID_LENGTH: u64 = 0x42F2;
const EBML_DOC_MAX_SIZE_LENGTH: u64 = 0x42F3;
const EBML_READ_VERSION: u64 = 0x42F7;
const EBML_VERSION: u64 = 0x4286;
const EBML_DOC_TYPE_VERSION: u64 = 0x4287;
const EBML_DOC_TYPE_READ_VERSION: u64 = 0x4285;
const SEGMENT: u64 = 0x18538067;
const SEEK_HEAD: u64 = 0x114d9b74;
const SEEK: u64 = 0x4dbb;
const SEEK_ID: u64 = 0x53ab;
const SEEK_POSITION: u64 = 0x53ac;
const INFO: u64 = 0x1549a966;
const TIMESTAMP_SCALE: u64 = 0x2ad7b1;
const DURATION: u64 = 0x4489;
const DATE_UTC: u64 = 0x4461;
const TRACKS: u64 = 0x1654ae6b;
const TRACK_ENTRY: u64 = 0xae;
const TRACK_NUMBER: u64 = 0xd7;
const TRACK_UID: u64 = 0x73c5;
const TRACK_TYPE: u64 = 0x83;
const CODEC_ID: u64 = 0x86;
const CODEC_PRIVATE: u64 = 0x63a2;
const VIDEO: u64 = 0xe0;
const AUDIO: u64 = 0xe1;
const SAMPLING_FREQUENCY: u64 = 0xb5;
const CHANNELS: u64 = 0x9f;
const BIT_DEPTH: u64 = 0x6264;
const CLUSTER: u64 = 0x1f43b675;
const TIMESTAMP: u64 = 0xe7;
const SIMPLE_BLOCK: u64 = 0xa3;
const BLOCK_GROUP: u64 = 0xa0;
const BLOCK: u64 = 0xa1;
const BLOCK_DURATION: u64 = 0x9b;

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

    #[error("No element 0x{0:08x} was found")]
    MissingElement(u64),

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
        format::{Muxer},
        io::Io,
        test::{self, TestFile},
        test_files,
    };

    use super::{MatroskaDemuxer, MatroskaMuxer};

    test_files! {
        #[tokio::test]
        async fn write_read_packets_are_equal(test_file: TestFile) {
            let (movie, packets) = test::read_mkv_from_path(test_file.path).await;
            println!("read movie");

            let io = Io::from_stream(Box::new(Vec::<u8>::new()));
            let mut muxer = MatroskaMuxer::new(io);

            test::write_movie_and_packets(&mut muxer, movie, &packets).await;
            println!("wrote movie");

            let buffer: Box<Vec<u8>> = muxer.into_io().into_writer().unwrap();
            let buffer = Box::new(Cursor::new(*buffer));
            let mut demuxer  = MatroskaDemuxer::new(Io::from_reader(buffer));

            let (new_movie, new_packets) = test::read_movie_and_packets(&mut demuxer).await;

            // TODO: assert
        }
    }
}
