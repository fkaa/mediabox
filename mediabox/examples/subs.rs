use mediabox::format::mkv::*;
use mediabox::format::*;
use mediabox::io::*;
use mediabox::*;

use log::*;
use tokio::fs::File;

use std::{env, str};

fn transcode_subtitles(track: &Track) -> Option<(u32, Transcode)> {
    (track.info.name != "webvtt").then(|| {
        (
            track.id,
            Transcode::Subtitles {
                decoder: mediabox::find_decoder_for_track(track).unwrap(),
                encoder: mediabox::find_encoder_with_params("webvtt", &track.info).unwrap(),
            },
        )
    })
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let path = env::args().nth(1).expect("Provide a file");
    debug!("Opening {path}");

    let io = Io::from_reader(Box::new(File::open(path).await.unwrap()));
    let mut demuxer = MatroskaDemuxer::new(io);

    let movie = demuxer.start().await.unwrap();
    for track in &movie.tracks {
        eprintln!("#{}: {:?}", track.id, track.info);
    }

    let transcode_mapping = movie.subtitles().filter_map(transcode_subtitles).collect();

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
    }
}
