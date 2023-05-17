use crate::{
    format::{DemuxerContext, Movie, SyncMuxerContext},
    memory::{MemoryPool, MemoryPoolConfig},
    Packet,
};

pub struct TestFile {
    pub path: &'static str,
    pub file_name: &'static str,
}

impl TestFile {
    pub fn new(path: &'static str, file_name: &'static str) -> Self {
        TestFile { path, file_name }
    }
}

#[macro_export]
macro_rules! test_files2 {
    ($func:item ; $($name:literal),+) => {
        $(#[test_case(TestFile::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/files/", $name), $name) ; $name)])*
        $func
    };
}

#[macro_export]
macro_rules! test_files {
    ($func:item) => {
        $crate::test_files2! {
            $func ;
            "testsrc-h264.mkv",
            "testsrc-h264-no-bframes.mkv"
        }
    };
}

pub fn read_mkv_from_path(path: &str) -> (Movie, Vec<Packet>) {
    let config = MemoryPoolConfig {
        max_capacity: None,
        default_memory_capacity: 1024,
    };
    let pool = MemoryPool::new(config);

    let mut demuxer = DemuxerContext::open_with_pool(path, pool.clone()).unwrap();

    read_movie_and_packets(&mut demuxer)
}

pub fn read_movie_and_packets(demuxer: &mut DemuxerContext) -> (Movie, Vec<Packet<'static>>) {
    let mut packets = Vec::new();

    let movie = demuxer.read_headers().unwrap();

    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    (movie, packets)
}

pub fn write_movie_and_packets(
    muxer: &mut SyncMuxerContext,
    movie: &Movie,
    packets: &[Packet<'static>],
) {
    muxer.start(movie).unwrap();
    for pkt in packets {
        muxer.write(&pkt).unwrap();
    }
    // muxer.stop().unwrap();
}

/*pub async fn read_mkv_from_io(io: Io) -> (Movie, Vec<Packet>) {
    let mut demuxer = MatroskaDemuxer::new(io);

    read_movie_and_packets(&mut demuxer).await
}
*/
