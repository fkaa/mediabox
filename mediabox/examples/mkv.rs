use mediabox::format::*;
use mediabox::io::*;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let _profiler = dhat::Profiler::new_heap();

    env_logger::init();

    let mut demuxer = DemuxerContext::open("./tests/files/testsrc-h264.mkv").unwrap();
    // let mut muxer = MuxerContext::open("").unwrap();
    // let mut muxer = Mp4Muxer::new(Io::create_file("test.mp4").await.unwrap());

    let movie = demuxer.read_headers().unwrap();

    for track in &movie.tracks {
        eprintln!(">  {}: {:?}", track.id, track.info);
    }

    // muxer.start(movie.tracks).await.unwrap();

    while let Some(pkt) = demuxer.read_packet().unwrap() {
        // println!("{:?}", pkt.time);

        // muxer.write(pkt).await.unwrap();
    }

    eprintln!("EOS!");

    // muxer.stop().await.unwrap();
}
