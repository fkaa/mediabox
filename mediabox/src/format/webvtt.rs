use async_trait::async_trait;

use crate::{io::Io, Packet, Track};

use super::Muxer;

#[derive(Debug, thiserror::Error)]
pub enum WebVttError {
    #[error("Only a single WebVTT track is allowed.")]
    InvalidTracks,
}

pub struct WebVttMuxer {
    track: Option<Track>,
    io: Io,
}

#[async_trait]
impl Muxer for WebVttMuxer {
    async fn start(&mut self, mut tracks: Vec<Track>) -> anyhow::Result<()> {
        if tracks.len() != 1 {
            Err(WebVttError::InvalidTracks)?;
        }

        let track = tracks.swap_remove(0);

        if track.info.name != "webvtt" {
            Err(WebVttError::InvalidTracks)?;
        }

        self.track = Some(track);

        self.io.write(b"WebVTT\n\n").await?;

        Ok(())
    }

    async fn write(&mut self, packet: Packet) -> anyhow::Result<()> {
        self.io.write_span(packet.buffer).await?;

        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
