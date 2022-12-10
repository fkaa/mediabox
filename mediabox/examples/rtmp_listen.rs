use mediabox::format::mp4::*;
use mediabox::format::rtmp::*;
use mediabox::format::*;
use mediabox::io::*;

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut listener = RtmpListener::bind("127.0.0.1:1935")
        .await
        .expect("Failed to bind RTMP listener");

    loop {
        let request = listener
            .accept()
            .await
            .expect("Failed to accept RTMP request");

        tokio::spawn(async {
            eprintln!("Got RTMP request from {}", request.addr());

            let mut session = request
                .authenticate()
                .await
                .expect("Failed to authenticate RTMP session");

            let streams = session.streams().await.expect("Failed to get streams");
            for stream in &streams {
                eprintln!("{}: {:?}", stream.id, stream.info);
            }

            let file = Io::create_file("file.mp4")
                .await
                .expect("Failed to create file");
            let mut writer = FragmentedMp4Muxer::new(file);

            writer.start(streams).await.expect("Failed to start muxer");

            loop {
                let pkt = session.read_frame().await.expect("Failed to read packet");
                eprintln!("{:?}", pkt);

                writer
                    .write(pkt)
                    .await
                    .expect("Failed to write packet to muxer");
            }
        });
    }
}
