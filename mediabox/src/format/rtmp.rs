use bytes::Bytes;
use futures::{
    channel::mpsc::{channel, Receiver, Sender},
    SinkExt,
};
use h264_reader::{
    annexb::AnnexBReader,
    nal::{
        pps::{PicParameterSet, PpsError},
        sps::{SeqParameterSet, SpsError},
        NalHandler, NalHeader, NalSwitch, UnitType,
    },
    rbsp::decode_nal,
    Context,
};
use rml_rtmp::{
    chunk_io::Packet,
    handshake::{Handshake, HandshakeProcessResult, PeerType},
    sessions::{
        ServerSession, ServerSessionConfig, ServerSessionEvent, ServerSessionResult, StreamMetadata,
    },
    time::RtmpTimestamp,
};

use bytes::{BufMut, BytesMut};
use log::*;
use tokio::net::{tcp, TcpListener, TcpStream, ToSocketAddrs};

use std::{cell::RefCell, collections::VecDeque, net::SocketAddr, sync::Arc};

use crate::{codec::nal::BitstreamFraming, media, Fraction, Track};

const RTMP_TIMEBASE: Fraction = Fraction::new(1, 1000);
const RTMP_AAC_TIMEBASE: Fraction = Fraction::new(1, 48000);

pub struct RtmpListener {
    listener: TcpListener,
}

impl RtmpListener {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> anyhow::Result<RtmpListener> {
        Ok(RtmpListener {
            listener: TcpListener::bind(addr).await?,
        })
    }

    pub async fn accept(&mut self) -> anyhow::Result<RtmpRequest> {
        let (socket, addr) = self.listener.accept().await?;

        RtmpRequest::from_socket(socket, addr).await
    }
}

pub struct RtmpRequest {
    write: tcp::OwnedWriteHalf,
    read: tcp::OwnedReadHalf,
    addr: SocketAddr,
    request_id: u32,
    app: String,
    key: String,
    results: VecDeque<ServerSessionResult>,
    server_session: ServerSession,
}

impl RtmpRequest {
    pub async fn from_socket(socket: TcpStream, addr: SocketAddr) -> anyhow::Result<Self> {
        socket.set_nodelay(true)?;

        let (mut read, mut write) = socket.into_split();
        let (server_session, results, request_id, app, key) =
            process(&mut read, &mut write).await?;

        let request = RtmpRequest {
            write,
            read,
            addr,
            request_id,
            app,
            key,
            results,
            server_session,
        };

        Ok(request)
    }

    pub async fn authenticate(mut self) -> anyhow::Result<RtmpSession> {
        let results = self.server_session.accept_request(self.request_id)?;

        self.results.extend(results);

        let (mut rtmp_tx, rtmp_rx) = channel(500);

        tokio::spawn(async move {
            match rtmp_write_task(self.write, rtmp_rx).await {
                Ok(()) => {
                    trace!("RTMP write task finished without errors");
                }
                Err(e) => {
                    warn!("RTMP write task finished with error: {}", e);
                }
            }
        });

        let mut new_results = Vec::new();
        for result in self.results.into_iter() {
            match result {
                ServerSessionResult::OutboundResponse(pkt) => rtmp_tx.send(pkt).await?,
                _ => new_results.push(result),
            }
        }

        let meta =
            wait_for_metadata(&mut self.server_session, &mut self.read, &mut rtmp_tx).await?;

        Ok(RtmpSession::new(
            meta,
            self.read,
            self.server_session,
            rtmp_tx,
            new_results.into(),
        ))
    }

    pub fn app(&self) -> &str {
        &self.app
    }

    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

async fn wait_for_metadata(
    rtmp_server_session: &mut ServerSession,
    read: &mut tcp::OwnedReadHalf,
    rtmp_tx: &mut Sender<Packet>,
) -> anyhow::Result<StreamMetadata> {
    use tokio::io::AsyncReadExt;

    debug!("Waiting for metadata");

    let mut buf = [0u8; 1024];
    loop {
        let n = read.read(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("EOS");
        }

        for res in rtmp_server_session.handle_input(&buf[..n]).map_err(|e| e)? {
            match res {
                ServerSessionResult::OutboundResponse(pkt) => rtmp_tx.send(pkt).await?,
                ServerSessionResult::RaisedEvent(ServerSessionEvent::StreamMetadataChanged {
                    app_name: _,
                    stream_key: _,
                    metadata,
                }) => return Ok(metadata),
                _ => {}
            }
        }
    }
}

async fn rtmp_write_task(
    mut write_filter: tcp::OwnedWriteHalf,
    mut rtmp_rx: Receiver<Packet>,
) -> anyhow::Result<()> {
    use futures::stream::StreamExt;
    use tokio::io::AsyncWriteExt;

    trace!("Starting RTMP write task");

    while let Some(pkt) = rtmp_rx.next().await {
        write_filter.write(&pkt.bytes).await?;
    }

    Ok(())
}

/// Process the initial handshake
async fn process(
    read: &mut tcp::OwnedReadHalf,
    write: &mut tcp::OwnedWriteHalf,
) -> anyhow::Result<(
    ServerSession,
    VecDeque<ServerSessionResult>,
    u32,
    String,
    String,
)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut handshake = Handshake::new(PeerType::Server);

    let mut buf = [0u8; 1024];
    // Do initial RTMP handshake
    let (response, remaining) = loop {
        let n = read.read(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("EOS");
        }

        let response = match handshake.process_bytes(&buf[..n])? {
            HandshakeProcessResult::InProgress { response_bytes } => response_bytes,
            HandshakeProcessResult::Completed {
                response_bytes,
                remaining_bytes,
            } => break (response_bytes, remaining_bytes),
        };

        write.write(&response).await?;
    };

    write.write(&response).await?;

    // Create the RTMP session
    let config = ServerSessionConfig::new();
    let (mut session, initial_results) = ServerSession::new(config)?;

    let results = session.handle_input(&remaining)?;

    let mut r = VecDeque::new();
    let mut stream_info = None;

    r.extend(results.into_iter().chain(initial_results.into_iter()));

    // TODO: add a timeout to the handshake process
    // Loop until we get a publish request
    loop {
        while let Some(res) = r.pop_front() {
            match res {
                ServerSessionResult::OutboundResponse(packet) => {
                    write.write(&packet.bytes).await?;
                }
                ServerSessionResult::RaisedEvent(evt) => match evt {
                    ServerSessionEvent::ConnectionRequested {
                        request_id,
                        app_name: _,
                    } => {
                        r.extend(session.accept_request(request_id)?);

                        debug!("Accepted connection request");
                    }
                    ServerSessionEvent::PublishStreamRequested {
                        request_id,
                        app_name,
                        stream_key,
                        mode: _,
                    } => {
                        stream_info = Some((request_id, app_name, stream_key));
                    }
                    _ => {}
                },
                ServerSessionResult::UnhandleableMessageReceived(_payload) => {}
            }
        }

        // Return the partial session (unauthenticated) when we
        // receive a publish request
        if let Some((request_id, app, key)) = stream_info.take() {
            return Ok((session, r, request_id, app, key));
        }

        // debug!("reading from endpoint!");
        let n = read.read(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("EOS");
        }
        let results = session.handle_input(&buf[..n])?;
        r.extend(results);
    }
}

pub struct RtmpSession {
    meta: StreamMetadata,
    read: tcp::OwnedReadHalf,
    server_session: ServerSession,
    rtmp_tx: Sender<Packet>,

    video_stream: Option<media::Track>,
    video_time: u64,
    prev_video_time: Option<RtmpTimestamp>,

    audio_stream: Option<media::Track>,
    audio_time: u64,
    prev_audio_time: Option<RtmpTimestamp>,

    results: VecDeque<ServerSessionResult>,
    frames: VecDeque<media::Packet>,
}

impl RtmpSession {
    pub fn new(
        meta: StreamMetadata,
        read: tcp::OwnedReadHalf,
        server_session: ServerSession,
        rtmp_tx: Sender<Packet>,
        results: VecDeque<ServerSessionResult>,
    ) -> Self {
        RtmpSession {
            meta,
            read,
            server_session,
            rtmp_tx,

            video_stream: None,
            video_time: 0,
            prev_video_time: None,

            audio_stream: None,
            audio_time: 0,
            prev_audio_time: None,

            results,
            frames: VecDeque::new(),
        }
    }

    fn assign_audio_stream(&mut self, tag: flvparse::AudioTag) -> anyhow::Result<()> {
        let codec_info = get_audio_codec_info(&tag)?;

        self.audio_stream = Some(media::Track {
            id: 1,
            info: Arc::new(codec_info),
            timebase: RTMP_AAC_TIMEBASE,
        });

        Ok(())
    }

    fn assign_video_stream(
        &mut self,
        _tag: flvparse::VideoTag,
        packet: flvparse::AvcVideoPacket,
    ) -> anyhow::Result<()> {
        let codec_info = match packet.packet_type {
            flvparse::AvcPacketType::SequenceHeader => get_codec_from_mp4(&packet)?,
            flvparse::AvcPacketType::NALU => get_codec_from_nalu(&packet)?,
            _ => anyhow::bail!("Unsupported AVC packet type: {:?}", packet.packet_type),
        };

        self.video_stream = Some(media::Track {
            id: 0,
            info: Arc::new(codec_info),
            timebase: RTMP_TIMEBASE,
        });

        Ok(())
    }

    fn add_video_frame(&mut self, data: Bytes, timestamp: RtmpTimestamp) -> anyhow::Result<()> {
        let (video_tag, video_packet) = parse_video_tag(&data)?;

        if self.video_stream.is_none() {
            self.assign_video_stream(video_tag, video_packet)?;
            return Ok(());
        }

        if self.prev_video_time.is_none() {
            self.prev_video_time = Some(timestamp);
        }

        let diff = timestamp
            - self
                .prev_video_time
                .unwrap_or_else(|| RtmpTimestamp::new(0));

        self.video_time += diff.value as u64;

        let time = media::MediaTime {
            pts: self.video_time,
            dts: None,
            duration: None,
            timebase: RTMP_TIMEBASE,
        };

        let pkt = media::Packet {
            time,
            track: self.video_stream.clone().unwrap(),
            key: video_tag.header.frame_type == flvparse::FrameType::Key,
            buffer: video_packet.avc_data.to_vec().into(),
        };

        self.frames.push_back(pkt);

        self.prev_video_time = Some(timestamp);

        Ok(())
    }

    fn add_audio_frame(&mut self, data: Bytes, timestamp: RtmpTimestamp) -> anyhow::Result<()> {
        let audio_tag = parse_audio_tag(&data)?;

        if self.audio_stream.is_none() {
            self.assign_audio_stream(audio_tag)?;
            return Ok(());
        }

        if self.prev_audio_time.is_none() {
            self.prev_audio_time = Some(timestamp);
        }

        let diff = timestamp
            - self
                .prev_audio_time
                .unwrap_or_else(|| RtmpTimestamp::new(0));

        self.audio_time += diff.value as u64;

        let time = media::MediaTime {
            pts: self.audio_time,
            dts: None,
            duration: None,
            timebase: RTMP_TIMEBASE,
        };

        let time = time.in_base(RTMP_AAC_TIMEBASE);

        let frame = media::Packet {
            time,
            key: true,
            buffer: Bytes::from(audio_tag.body.data[1..].to_vec()).into(),
            track: self.audio_stream.clone().unwrap(),
        };

        self.frames.push_back(frame);

        self.prev_audio_time = Some(timestamp);

        Ok(())
    }

    async fn process_event(&mut self, event: ServerSessionEvent) -> anyhow::Result<()> {
        match event {
            ServerSessionEvent::AudioDataReceived {
                app_name: _,
                stream_key: _,
                data,
                timestamp,
            } => {
                self.add_audio_frame(data, timestamp)?;
            }
            ServerSessionEvent::VideoDataReceived {
                app_name: _,
                stream_key: _,
                data,
                timestamp,
            } => {
                self.add_video_frame(data, timestamp)?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn process_results<I: IntoIterator<Item = ServerSessionResult>>(
        &mut self,
        results: I,
    ) -> anyhow::Result<()> {
        for result in results.into_iter() {
            match result {
                ServerSessionResult::OutboundResponse(pkt) => self.rtmp_tx.send(pkt).await?,
                ServerSessionResult::RaisedEvent(evt) => self.process_event(evt).await?,
                ServerSessionResult::UnhandleableMessageReceived(_payload) => {}
            }
        }

        Ok(())
    }

    async fn fetch(&mut self) -> anyhow::Result<()> {
        use tokio::io::AsyncReadExt;

        let mut buf = [0u8; 1024];
        let n = self.read.read(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("EOS");
        }

        let results = self.server_session.handle_input(&buf[..n])?;

        self.process_results(results).await?;

        Ok(())
    }

    pub async fn streams(&mut self) -> anyhow::Result<Vec<Track>> {
        let expecting_video = self.meta.video_width.is_some();
        let expecting_audio = self.meta.audio_sample_rate.is_some();

        while (expecting_video && self.video_stream.is_none())
            || (expecting_audio && self.audio_stream.is_none())
        {
            self.fetch().await?;
        }

        if let Some(ref video) = self.video_stream {
            debug!("Video: {:?}", video.info);
        }
        if let Some(ref audio) = self.audio_stream {
            debug!("Audio: {:?}", audio.info);
        }

        let streams = [self.video_stream.clone(), self.audio_stream.clone()];

        Ok(streams.into_iter().flatten().collect())
    }

    pub async fn read_frame(&mut self) -> anyhow::Result<media::Packet> {
        loop {
            if let Some(frame) = self.frames.pop_front() {
                return Ok(frame);
            }

            self.fetch().await?;
        }
    }
}

fn parse_video_tag(data: &[u8]) -> anyhow::Result<(flvparse::VideoTag, flvparse::AvcVideoPacket)> {
    let tag = flvparse::VideoTag::parse(data, data.len())
        .map(|(_, t)| t)
        .map_err(|e| anyhow::anyhow!("Failed to parse video tag: {}", e))?;

    let packet = flvparse::avc_video_packet(tag.body.data, tag.body.data.len())
        .map(|(_, p)| p)
        .map_err(|e| anyhow::anyhow!("Failed to parse AVC packet: {}", e))?;

    Ok((tag, packet))
}

fn parse_audio_tag(data: &[u8]) -> anyhow::Result<flvparse::AudioTag> {
    let tag = flvparse::AudioTag::parse(data, data.len())
        .map(|(_, t)| t)
        .map_err(|e| anyhow::anyhow!("Failed to parse audio tag: {}", e))?;

    Ok(tag)
}

fn get_codec_from_nalu(packet: &flvparse::AvcVideoPacket) -> anyhow::Result<media::MediaInfo> {
    let parameter_sets = find_parameter_sets(packet.avc_data);

    dbg!(&parameter_sets);
    let codec_info = get_video_codec_info(parameter_sets)?;

    Ok(codec_info)
}

fn get_codec_from_mp4(packet: &flvparse::AvcVideoPacket) -> anyhow::Result<media::MediaInfo> {
    use h264_reader::avcc::AvcDecoderConfigurationRecord;

    let avc_record: AvcDecoderConfigurationRecord = packet
        .avc_data
        .try_into()
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;

    let sps_bytes_no_header = avc_record
        .sequence_parameter_sets()
        .next()
        .ok_or(anyhow::anyhow!("No SPS found"))
        .unwrap()
        .unwrap();
    let pps_bytes_no_header = avc_record
        .picture_parameter_sets()
        .next()
        .ok_or(anyhow::anyhow!("No PPS found"))
        .unwrap()
        .unwrap();

    let mut sps_bytes = BytesMut::new();
    sps_bytes.put_u8(UnitType::SeqParameterSet.id());
    sps_bytes.extend_from_slice(&sps_bytes_no_header);
    let sps_bytes = sps_bytes.freeze().into();

    let mut pps_bytes = BytesMut::new();
    pps_bytes.put_u8(UnitType::PicParameterSet.id());
    pps_bytes.extend_from_slice(&pps_bytes_no_header);
    let pps_bytes = pps_bytes.freeze().into();

    let sps = SeqParameterSet::from_bytes(&decode_nal(&sps_bytes_no_header[..]))
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;
    let (width, height) = sps.pixel_dimensions().unwrap();

    let codec = media::H264Codec {
        bitstream_format: BitstreamFraming::FourByteLength,
        profile_indication: avc_record.avc_profile_indication().into(),
        profile_compatibility: avc_record.profile_compatibility().into(),
        level_indication: avc_record.avc_level_indication().level_idc(),
        // FIXME Always uses first set
        sps: sps_bytes,
        pps: pps_bytes,
    };

    Ok(media::MediaInfo {
        name: "h264",
        kind: media::MediaKind::Video(media::VideoInfo {
            width,
            height,
            codec: media::VideoCodec::H264(codec),
        }),
    })
}

fn find_parameter_sets(bytes: &[u8]) -> ParameterSetContext {
    let mut s = NalSwitch::default();
    s.put_handler(
        UnitType::SeqParameterSet,
        Box::new(RefCell::new(SpsHandler)),
    );
    s.put_handler(
        UnitType::PicParameterSet,
        Box::new(RefCell::new(PpsHandler)),
    );

    let mut ctx = Context::new(ParameterSetContext::default());

    let mut reader = AnnexBReader::new(s);
    reader.start(&mut ctx);
    reader.push(&mut ctx, bytes);
    reader.end_units(&mut ctx);

    ctx.user_context
}

fn get_video_codec_info(parameter_sets: ParameterSetContext) -> anyhow::Result<media::MediaInfo> {
    let (sps_bytes_no_header, sps) = parameter_sets.sps.unwrap();
    let (pps_bytes_no_header, _pps) = parameter_sets.pps.unwrap();

    let mut sps_bytes = BytesMut::new();
    sps_bytes.put_u8(UnitType::SeqParameterSet.id());
    sps_bytes.extend_from_slice(&sps_bytes_no_header);
    let sps_bytes = sps_bytes.freeze().into();

    let mut pps_bytes = BytesMut::new();
    pps_bytes.put_u8(UnitType::PicParameterSet.id());
    pps_bytes.extend_from_slice(&pps_bytes_no_header);
    let pps_bytes = pps_bytes.freeze().into();

    let sps = sps.unwrap();

    let (width, height) = sps.pixel_dimensions().unwrap();

    let profile_indication = sps.profile_idc.into();
    let profile_compatibility = sps.constraint_flags.into();
    let level_indication = sps.level_idc;

    let codec = media::H264Codec {
        bitstream_format: BitstreamFraming::FourByteLength,
        profile_indication,
        profile_compatibility,
        level_indication,
        sps: sps_bytes,
        pps: pps_bytes,
    };

    Ok(media::MediaInfo {
        name: "h264",
        kind: media::MediaKind::Video(media::VideoInfo {
            width,
            height,
            codec: media::VideoCodec::H264(codec),
        }),
    })
}

fn get_audio_codec_info(tag: &flvparse::AudioTag) -> anyhow::Result<media::MediaInfo> {
    let name = match tag.header.sound_format {
        flvparse::SoundFormat::AAC => "aac",
        _ => anyhow::bail!("Unsupported audio codec {:?}", tag.header.sound_format),
    };

    let codec = media::AacCodec {
        extra: match tag.body.data[0] {
            // TODO Maybe this doesn't have to be owned
            0 => tag.body.data[1..].to_owned(), // AudioSpecificConfig
            1 => unimplemented!("Raw AAC frame data"),
            _ => panic!("Unknown AACPacketType"),
        },
    };

    Ok(media::MediaInfo {
        name,
        kind: media::MediaKind::Audio(media::AudioInfo {
            sample_rate: match tag.header.sound_rate {
                flvparse::SoundRate::_5_5KHZ => 5500,
                flvparse::SoundRate::_11KHZ => 11000,
                flvparse::SoundRate::_22KHZ => 22000,
                flvparse::SoundRate::_44KHZ => 44000,
            },
            sample_bpp: match tag.header.sound_size {
                flvparse::SoundSize::_8Bit => 8,
                flvparse::SoundSize::_16Bit => 16,
            },
            sound_type: match tag.header.sound_type {
                flvparse::SoundType::Mono => media::SoundType::Mono,
                flvparse::SoundType::Stereo => media::SoundType::Stereo,
            },
            codec: media::AudioCodec::Aac(codec),
        }),
    })
}

#[derive(Debug, Default)]
pub struct ParameterSetContext {
    pub sps: Option<(Vec<u8>, Result<SeqParameterSet, SpsError>)>,
    pub pps: Option<(Vec<u8>, Result<PicParameterSet, PpsError>)>,
}

pub struct SpsHandler;
pub struct PpsHandler;

impl NalHandler for SpsHandler {
    type Ctx = ParameterSetContext;

    fn start(&mut self, _ctx: &mut Context<Self::Ctx>, _header: NalHeader) {}

    fn push(&mut self, ctx: &mut Context<Self::Ctx>, buf: &[u8]) {
        // error!("handle SPS: {}", base64::encode(&buf[1..]));
        let sps = SeqParameterSet::from_bytes(&decode_nal(&buf[1..]));
        if let Ok(sps) = &sps {
            ctx.put_seq_param_set(sps.clone());
        }
    }

    fn end(&mut self, _ctx: &mut Context<Self::Ctx>) {}
}

impl NalHandler for PpsHandler {
    type Ctx = ParameterSetContext;

    fn start(&mut self, _ctx: &mut Context<Self::Ctx>, _header: NalHeader) {}

    fn push(&mut self, ctx: &mut Context<Self::Ctx>, buf: &[u8]) {
        // error!("handle PPS: {}", base64::encode(&buf[1..]));
        ctx.user_context.pps = Some((
            buf.to_vec(),
            PicParameterSet::from_bytes(ctx, &decode_nal(&buf[1..])),
        ));
    }

    fn end(&mut self, _ctx: &mut Context<Self::Ctx>) {}
}
