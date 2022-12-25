use async_trait::async_trait;

use crate::{Track, io::Io, format::Muxer, muxer, Packet};

muxer!("mkv", MatroskaMuxer::create);

pub struct MatroskaMuxer {
    video: Option<Track>,
    audio: Option<Track>,
    io: Io,
}

impl MatroskaMuxer {
    pub fn new(io: Io) -> Self {
        MatroskaMuxer {
            video: None,
            audio: None,
            io,
        }
    }

    fn create(io: Io) -> Box<dyn Muxer> {
        Box::new(Self::new(io))
    }
}

#[async_trait]
impl Muxer for MatroskaMuxer {
    async fn start(&mut self, streams: Vec<Track>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn write(&mut self, packet: Packet) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    
    fn into_io(self) -> Io {
        self.io
    }
}
