use mediabox::format::*;
use mediabox::io::*;
use mediabox::memory::*;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let _profiler = dhat::Profiler::new_heap();

    env_logger::init();

    let config = MemoryPoolConfig {
        max_capacity: None,
        default_memory_capacity: 1024,
    };
    let mut pool = MemoryPool::new(config);

    let mut demuxer =
        DemuxerContext::open_with_pool("./tests/files/testsrc-h264.mkv", pool.clone()).unwrap();
    let mut muxer = SyncMuxerContext::open_with_pool("./target/test.mkv", pool.clone()).unwrap();

    let movie = demuxer.read_headers().unwrap();

    for track in &movie.tracks {
        eprintln!(">  {}: {:?}", track.id, track.info);
    }

    muxer.start(&movie).unwrap();

    while let Some(pkt) = demuxer.read_packet().unwrap() {
        muxer.write(&pkt).unwrap();
    }

    eprintln!("EOS!");

    // muxer.stop().await.unwrap();
}
