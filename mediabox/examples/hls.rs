use mediabox::format::hls::*;
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

    eprintln!("{movie:#?}");

    let mut hls_mux = HlsMuxer::new("playlist.m3u8").await.unwrap();

    let mut muxer = hls_mux.new_stream(&movie).await.unwrap();
    muxer.start(movie.tracks).await.unwrap();

    // println!("pts,dts,keyframe,stream,length");
    loop {
        let pkt = demuxer.read().await.unwrap();

        // println!("{:?}", pkt.time);
        /*if pkt.track.id == subtitle_id {
            println!(
                "{}",
                String::from_utf8(pkt.buffer.to_slice().into_owned()).unwrap()
            );
        }*/
    }
}
