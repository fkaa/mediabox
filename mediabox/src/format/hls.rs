use std::{time::Duration, path::Path, fmt, io::Write};

use async_trait::async_trait;

use crate::{io::Io, Packet, Track};

use super::{Movie, Muxer};

/// A muxer for the *HTTP Live Streaming* (HLS) format/protocol.
///
/// *Note* that HLS is not just one file, but consists of several playlist files and multiple
/// media segment files.
pub struct HlsMuxer {
    master_playlist: Io,
    movies: u32,
}

impl HlsMuxer {
    pub async fn new<P: AsRef<Path> + fmt::Debug>(path: P) -> anyhow::Result<Self> {
let mut master_playlist= Io::create_file(path).await?;

master_playlist.write(b"#EXTM3U\n").await?;
        Ok(HlsMuxer {
            master_playlist,
            movies: 0,
        })
    }

    async fn write_variant_entry(&mut self, movie: &Movie, path: &str) -> anyhow::Result<()> {
        let mut entry = Vec::new();
        write_hls_stream_info_for_movie(&mut entry, movie, 500);
        writeln!(&mut entry, "{}", path).unwrap();

        self.master_playlist.write(&entry).await?;

        Ok(())
    }
}

pub struct HlsPlaylist {
    
}

pub enum HlsMediaType {
    Video,
    Audio,
    Subtitle,
}

pub struct HlsMedia {
    media_type: HlsMediaType,
    group: String,
    name: String,
    default: Option<bool>,
}

impl HlsMuxer {
    pub async fn new_playlist(&mut self, group: name: &str) {

    }

    pub async fn new_stream(&mut self, movie: &Movie) -> anyhow::Result<HlsStreamMuxer> {
        self.movies += 1;

        let path = format!("movie_{}.m3u8", self.movies);

        self.write_variant_entry(movie, &path).await?;

        Ok(HlsStreamMuxer {
            playlist: Io::create_file(&path).await?,
            segment_idx: 0,
            segment_duration: Duration::from_secs(0)
        })
    }
}

pub struct HlsStreamMuxer {
    playlist: Io,
    segment_idx: u32,
    segment_duration: Duration,
}

#[async_trait]
impl Muxer for HlsStreamMuxer {
    async fn start(&mut self, streams: Vec<Track>) -> anyhow::Result<()> {
        self.write_preamble().await?;

        todo!()
    }

    async fn write(&mut self, packet: Packet) -> anyhow::Result<()> {
        todo!()
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }
}

impl HlsStreamMuxer {
    async fn write_preamble(&mut self) -> anyhow::Result<()> {
        let preamble = b"#EXTM3U
#EXT-X-PLAYLIST-TYPE:VOD
#EXT-X-TARGETDURATION:10
#EXT-X-VERSION:4
#EXT-X-MEDIA-SEQUENCE:0";

        self.playlist.write(preamble).await?;

        Ok(())
    }
}

fn write_hls_stream_info_for_movie(entry: &mut Vec<u8>, movie: &Movie, bandwidth: u64) {
    write!(entry, "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}").unwrap();

    if let Some(codec) = movie.codec_string() {
        write!(entry, ",CODECS=\"{codec}\"").unwrap();
    }

    writeln!(entry).unwrap();
}
