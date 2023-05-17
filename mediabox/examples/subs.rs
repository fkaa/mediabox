/*fn transcode_subtitles(cxt: &MediaContext, track: &Track) -> Option<(u32, Transcode)> {
    (track.info.name != "webvtt").then(|| {
        (
            track.id,
            Transcode::Subtitles {
                decoder: cxt.find_decoder_for_track(track).unwrap(),
                encoder: cxt.find_encoder_with_params("webvtt", &track.info).unwrap(),
            },
        )
    })
}*/

#[tokio::main]
async fn main() {
    env_logger::init();

    /*let mut cxt = MediaContext::default();
    cxt.register_all();

    let path = env::args().nth(1).expect("Provide a file");
    debug!("Opening {path}");

    let io = Io::from_reader(Box::new(File::open(path).await.unwrap()));
    let mut demuxer = MatroskaDemuxer::new(io);

    let movie = demuxer.start().await.unwrap();
    for track in &movie.tracks {
        eprintln!("#{}: {:?}", track.id, track.info);
    }

    let transcode_mapping = movie
        .subtitles()
        .filter_map(|t| transcode_subtitles(&cxt, t))
        .collect();

    let mut transcoder = PacketTranscoder::new(transcode_mapping);

    loop {
        let pkt = demuxer.read().await.unwrap();

        transcoder
            .process(pkt, |pkt| {
                if pkt.track.info.name == "webvtt" {
                    eprintln!(
                        "{}",
                        str::from_utf8(&pkt.buffer.to_slice()).expect("Failed to read string")
                    );
                }
            })
            .await
            .unwrap();
    }*/
}
