mod demux;
mod mux;

pub use demux::*;
pub use mux::*;

const EBML_HEADER: u32 = 0x1a45dfa3;
const EBML_DOC_TYPE: u32 = 0x4282;
const EBML_DOC_TYPE_VERSION: u32 = 0x4287;
const EBML_DOC_TYPE_READ_VERSION: u32 = 0x4285;
const SEGMENT: u32 = 0x18538067;
const SEEK_HEAD: u32 = 0x114d9b74;
const SEEK: u32 = 0x4dbb;
const SEEK_ID: u32 = 0x53ab;
const SEEK_POSITION: u32 = 0x53ac;
const INFO: u32 = 0x1549a966;
const TIMESTAMP_SCALE: u32 = 0x2ad7b1;
const DURATION: u32 = 0x4489;
const DATE_UTC: u32 = 0x4461;
const TRACKS: u32 = 0x1654ae6b;
const TRACK_ENTRY: u32 = 0xae;
const TRACK_NUMBER: u32 = 0xd7;
const TRACK_UID: u32 = 0x73c5;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63a2;
const VIDEO: u32 = 0xe0;
const AUDIO: u32 = 0xe1;
const SAMPLING_FREQUENCY: u32 = 0xb5;
const CHANNELS: u32 = 0x9f;
const BIT_DEPTH: u32 = 0x6264;
const CLUSTER: u32 = 0x1f43b675;
const TIMESTAMP: u32 = 0xe7;
const SIMPLE_BLOCK: u32 = 0xa3;
const BLOCK_GROUP: u32 = 0xa0;
const BLOCK: u32 = 0xa1;
const BLOCK_DURATION: u32 = 0x9b;


#[cfg(test)]
mod test {
    use std::io::Cursor;

    use test_case::test_case;
    use tokio::io::BufReader;

    use crate::{format::{Muxer, Demuxer}, test_files, test::{TestFile, self}, io::Io};

    use super::{MatroskaDemuxer, MatroskaMuxer};

    test_files!{
        #[tokio::test]
        async fn write_read_packets_are_equal(test_file: TestFile) {
            let (movie, packets) = test::read_mkv_from_path(test_file.path).await;
            
            let io = Io::from_stream(Box::new(Vec::<u8>::new()));
            let mut muxer = MatroskaMuxer::new(io);

            test::write_movie_and_packets(&mut muxer, movie, &packets).await;

            let buffer: Box<Vec<u8>> = muxer.into_io().into_writer().unwrap();
            let buffer = Box::new(Cursor::new(*buffer));
            let mut demuxer  = MatroskaDemuxer::new(Io::from_reader(buffer));

            let (new_movie, new_packets) = test::read_movie_and_packets(&mut demuxer).await;


        }
    }
}
