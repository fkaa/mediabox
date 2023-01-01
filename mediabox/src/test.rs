use tokio::fs::File;

use crate::{
    format::{mkv::MatroskaDemuxer, Demuxer, Movie, Muxer},
    io::Io,
    Packet,
};

pub struct TestFile {
    pub path: &'static str,
}

impl TestFile {
    pub fn new(path: &'static str) -> Self {
        TestFile { path }
    }
}

#[macro_export]
macro_rules! test_files2 {
    ($func:item ; $($name:literal),+) => {
        $(#[test_case(TestFile::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/files/", $name)) ; $name)])*
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

pub async fn read_mkv_from_path(path: &str) -> (Movie, Vec<Packet>) {
    let file = File::open(path).await.unwrap();
    let mut demuxer = MatroskaDemuxer::new(Io::from_reader(Box::new(file)));

    read_movie_and_packets(&mut demuxer).await
}

pub async fn read_mkv_from_io(io: Io) -> (Movie, Vec<Packet>) {
    let mut demuxer = MatroskaDemuxer::new(io);

    read_movie_and_packets(&mut demuxer).await
}

pub async fn write_movie_and_packets(muxer: &mut dyn Muxer, movie: Movie, packets: &[Packet]) {
    muxer.start(movie.tracks).await.unwrap();
    for pkt in packets {
        muxer.write(pkt.clone()).await.unwrap();
    }
    muxer.stop().await.unwrap();
}

pub async fn read_movie_and_packets(demuxer: &mut dyn Demuxer) -> (Movie, Vec<Packet>) {
    let movie = demuxer.start().await.unwrap();
    let mut packets = Vec::new();

    while let Ok(pkt) = demuxer.read().await {
        packets.push(pkt);
    }

    demuxer.stop().await.unwrap();

    (movie, packets)
}
