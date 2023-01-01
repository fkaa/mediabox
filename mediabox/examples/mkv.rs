use mediabox::format::mkv::*;
use mediabox::format::mp4::*;
use mediabox::format::*;
use mediabox::io::*;

use tokio::fs::File;

#[tokio::main]
async fn main() {
    env_logger::init();

    let file = File::open("test.mkv").await.unwrap();
    let _out = File::create("test.mp4").await.unwrap();
    let mut demuxer = MatroskaDemuxer::new(Io::from_reader(Box::new(file)));
    let mut muxer = Mp4Muxer::new(Io::create_file("test.mp4").await.unwrap());

    let movie = demuxer.start().await.unwrap();

    for track in &movie.tracks {
        eprintln!("#{}: {:?}", track.id, track.info);
    }

    muxer.start(movie.tracks).await.unwrap();

    loop {
        let pkt = demuxer.read().await.unwrap();

        println!("{:?}", pkt.time);

        muxer.write(pkt).await.unwrap();
    }

    muxer.stop().await.unwrap();
}
