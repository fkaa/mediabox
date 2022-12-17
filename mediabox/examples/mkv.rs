use mediabox::format::mkv::*;
use mediabox::format::*;
use mediabox::io::*;

use tokio::fs::File;

#[tokio::main]
async fn main() {
    env_logger::init();

    let file = File::open("test.mkv").await.unwrap();
    let io = Io::from_reader(Box::new(file));
    let mut demuxer = MatroskaDemuxer::new(io);

    let movie = demuxer.start().await.unwrap();

    for track in &movie.tracks {
        eprintln!("#{}: {:?}", track.id, track.info);
    }

    loop {
        let pkt = demuxer.read().await.unwrap();

        println!("{:?}", pkt.time);
    }
}
