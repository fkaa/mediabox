use h264_reader::{
    annexb::AnnexBReader,
    avcc::AvcDecoderConfigurationRecord,
    nal::{sps::SeqParameterSet, NalHandler, NalHeader, NalSwitch, UnitType},
    rbsp::decode_nal,
    Context,
};

use bytes::{BufMut, Bytes, BytesMut};

use std::cell::RefCell;

use crate::{H264Codec, MediaInfo, MediaKind, Span, VideoCodec, VideoInfo};

/// Describes how H.264 and H.265 NAL units are framed.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BitstreamFraming {
    /// NAL units are prefixed with a 4 byte length integer. Used by
    /// the `AVC1` and `HVC1` fourcc, mainly for storage in MP4 files.
    FourByteLength,

    /// NAL units are prefixed with a 2 byte length integer.
    TwoByteLength,

    // /// NAL units are prefixed with a 3 byte start code '00 00 01'.
    // ThreeByteStartCode,
    /// NAL units are prefixed with a 4 byte start code `00 00 00 01`.
    FourByteStartCode,
}

impl BitstreamFraming {
    pub fn is_start_code(&self) -> bool {
        matches!(
            self,
            //BitstreamFraming::ThreeByteStartCode |
            BitstreamFraming::FourByteStartCode
        )
    }
}

const THREE_BYTE_STARTCODE: [u8; 3] = [0, 0, 1];
const FOUR_BYTE_STARTCODE: [u8; 4] = [0, 0, 0, 1];

/// Parses a H.26x bitstream framed in AVC format (length prefix) into NAL units.
fn parse_bitstream_length_field<const N: usize, F: Fn([u8; N]) -> usize>(
    bitstream: Span,
    read: F,
) -> Vec<Span> {
    let mut nal_units = Vec::new();

    let mut i = 0;
    let len = bitstream.len();
    while i < len - N {
        let len_bytes = bitstream.slice(i..(i + N));
        let len_bytes = len_bytes.to_slice();
        let len_bytes = <[u8; N]>::try_from(&len_bytes[..]).unwrap();
        let nal_unit_len = read(len_bytes); // BigEndian::read_u32(&nal_units[i..]) as usize;

        dbg!(len_bytes, nal_unit_len);

        i += N;

        let nal_unit = bitstream.slice(i..(i + nal_unit_len));
        nal_units.push(nal_unit);

        i += nal_unit_len;
    }

    nal_units.into_iter().collect()
}

/// Parses a H.26x bitstream framed in Annex B format (start codes) into NAL units.
fn parse_bitstream_start_codes(bitstream: Span) -> Vec<Span> {
    let mut s = NalSwitch::default(); // new(NalFramer::new());
    s.put_handler(
        UnitType::SliceLayerWithoutPartitioningNonIdr,
        Box::new(RefCell::new(GenericNalHandler::with_capacity(1024 * 32))),
    );
    s.put_handler(
        UnitType::SliceLayerWithoutPartitioningIdr,
        Box::new(RefCell::new(GenericNalHandler::with_capacity(1024 * 512))),
    );
    s.put_handler(
        UnitType::SEI,
        Box::new(RefCell::new(GenericNalHandler::with_capacity(1024))),
    );
    s.put_handler(
        UnitType::SeqParameterSet,
        Box::new(RefCell::new(GenericNalHandler::with_capacity(32))),
    );
    s.put_handler(
        UnitType::PicParameterSet,
        Box::new(RefCell::new(GenericNalHandler::with_capacity(32))),
    );

    let framing_context = NalFramerContext::new();

    let mut ctx = Context::new(framing_context);
    let mut reader = AnnexBReader::new(s);
    reader.start(&mut ctx);
    for span in bitstream.spans() {
        reader.push(&mut ctx, &span);
    }
    reader.end_units(&mut ctx);

    ctx.user_context.nal_units.into_iter().collect()
}

/// Parses a H.26x bitstream in a given [BitstreamFraming] into NAL units.
pub fn parse_bitstream(bitstream: Span, source: BitstreamFraming) -> Vec<Span> {
    match source {
        BitstreamFraming::TwoByteLength => {
            parse_bitstream_length_field::<2, _>(bitstream, |b| u16::from_be_bytes(b) as usize)
        }
        BitstreamFraming::FourByteLength => {
            parse_bitstream_length_field::<4, _>(bitstream, |b| u32::from_be_bytes(b) as usize)
        }
        BitstreamFraming::FourByteStartCode => parse_bitstream_start_codes(bitstream),
    }
}

/// Frames NAL units with a given start code before each NAL.
fn frame_nal_units_with_start_codes(nal_units: &[Span], codes: &'static [u8]) -> Span {
    let mut spans = Vec::new();

    for nal in nal_units {
        spans.push(Span::from(Bytes::from_static(codes)));
        spans.push(nal.clone());
    }

    spans.into_iter().collect()
}

/// Frames NAL units with a length prefix before each NAL.
fn frame_nal_units_with_length<const N: usize, F: Fn(usize) -> [u8; N]>(
    nal_units: &[Span],
    func: F,
) -> Span {
    let mut spans = Vec::new();

    for nal in nal_units {
        let len_bytes = func(nal.len());
        spans.push(Span::from(len_bytes.to_vec()));
        spans.push(nal.clone());
    }

    spans.into_iter().collect()
}

/// Frame the given NAL units with the specified [BitstreamFraming].
///
/// The NAL units are assumed to have no prefix.
pub fn frame_nal_units(nal_units: &[Span], target: BitstreamFraming) -> Span {
    match target {
        BitstreamFraming::TwoByteLength => {
            frame_nal_units_with_length(nal_units, |len| (len as u16).to_be_bytes())
        }
        BitstreamFraming::FourByteLength => {
            frame_nal_units_with_length(nal_units, |len| (len as u32).to_be_bytes())
        }
        BitstreamFraming::FourByteStartCode => {
            frame_nal_units_with_start_codes(nal_units, &FOUR_BYTE_STARTCODE[..])
        }
    }
}

/// Converts a H.26x bitstream from a source [BitstreamFraming] to a
/// target [BitstreamFraming].
pub fn convert_bitstream(
    bitstream: Span,
    source: BitstreamFraming,
    target: BitstreamFraming,
) -> Span {
    if source == target {
        return bitstream;
    }

    let nal_units = parse_bitstream(bitstream, source);
    dbg!(&nal_units);

    frame_nal_units(&nal_units[..], target)
}

pub fn is_video_nal_unit(nal: &Bytes) -> bool {
    matches!(
        nut_header(nal),
        Some(UnitType::SeqParameterSet)
            | Some(UnitType::PicParameterSet)
            | Some(UnitType::SliceLayerWithoutPartitioningNonIdr)
            | Some(UnitType::SliceLayerWithoutPartitioningIdr)
    )
}

pub fn nut_header(nal: &Bytes) -> Option<UnitType> {
    NalHeader::new(nal[0]).map(|h| h.nal_unit_type()).ok()
}

pub fn get_codec_from_mp4(
    decoder_config: &AvcDecoderConfigurationRecord,
) -> anyhow::Result<MediaInfo> {
    let sps_bytes_no_header = decoder_config
        .sequence_parameter_sets()
        .next()
        .ok_or(anyhow::anyhow!("No SPS found"))
        .unwrap()
        .unwrap();
    let pps_bytes_no_header = decoder_config
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

    let codec = H264Codec {
        bitstream_format: BitstreamFraming::FourByteLength,
        profile_indication: decoder_config.avc_profile_indication().into(),
        profile_compatibility: decoder_config.profile_compatibility().into(),
        level_indication: decoder_config.avc_level_indication().level_idc(),
        sps: sps_bytes,
        pps: pps_bytes,
    };

    Ok(MediaInfo {
        name: "h264",
        kind: MediaKind::Video(VideoInfo {
            width,
            height,
            codec: VideoCodec::H264(codec),
        }),
    })
}

struct NalFramerContext {
    nal_units: Vec<Span>,
    current_nal_unit: Vec<u8>,
}

impl NalFramerContext {
    pub fn new() -> Self {
        Self {
            nal_units: Vec::new(),
            current_nal_unit: Vec::new(),
        }
    }
}

struct GenericNalHandler {
    capacity: usize,
    nut: Option<Vec<u8>>,
}

impl GenericNalHandler {
    fn with_capacity(size: usize) -> Self {
        GenericNalHandler {
            capacity: size,
            nut: Some(Vec::with_capacity(size)),
        }
    }
}

impl NalHandler for GenericNalHandler {
    type Ctx = NalFramerContext;

    fn start(&mut self, ctx: &mut Context<Self::Ctx>, header: NalHeader) {
        let mut nal_unit = Vec::with_capacity(self.capacity);
        nal_unit.push(header.into());
        self.nut = Some(nal_unit);
    }

    fn push(&mut self, ctx: &mut Context<Self::Ctx>, buf: &[u8]) {
        if let Some(ref mut nut) = &mut self.nut {
            nut.extend(buf);
        }
    }

    fn end(&mut self, ctx: &mut Context<Self::Ctx>) {
        if let Some(nut) = self.nut.take() {
            ctx.user_context.nal_units.push(nut.into());
        }
    }
}

struct NalFramer {
    nal_start: usize,
}

impl NalFramer {
    pub fn new() -> Self {
        Self { nal_start: 0 }
    }
}

impl NalHandler for NalFramer {
    type Ctx = NalFramerContext;

    fn start(&mut self, _ctx: &mut Context<Self::Ctx>, _header: NalHeader) {}

    fn push(&mut self, ctx: &mut Context<Self::Ctx>, buf: &[u8]) {
        ctx.user_context.current_nal_unit.extend(buf);
    }

    fn end(&mut self, ctx: &mut Context<Self::Ctx>) {
        let mut nal_unit = Vec::new();

        std::mem::swap(&mut nal_unit, &mut ctx.user_context.current_nal_unit);
        ctx.user_context.nal_units.push(nal_unit.into());
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use test_case::test_case;

    use BitstreamFraming::*;
    use FOUR_BYTE_STARTCODE as FS;

    fn len(l: u32) -> [u8; 4] {
        l.to_be_bytes()
    }

    #[test_case(&[b"a"], FourByteStartCode, &[&FS, b"a"])]
    #[test_case(&[b"a", b"b"], FourByteStartCode, &[&FS, b"a", &FS, b"b"])]
    #[test_case(&[b"a"], FourByteLength, &[&len(1), b"a"])]
    #[test_case(&[b"abc"], FourByteLength, &[&len(3), b"abc"])]
    #[test_case(&[b"a", b"b"], FourByteLength, &[&len(1), b"a", &len(1), b"b"])]
    fn frame_nal_units(nal_units: &[&'static [u8]], framing: BitstreamFraming, expected: &[&[u8]]) {
        let nal_units = nal_units.iter().map(|&n| Span::from(n)).collect::<Vec<_>>();
        let framed = super::frame_nal_units(&nal_units, framing);

        let expected = expected
            .iter()
            .flat_map(|&s| s)
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(expected, framed.to_bytes());
    }

    #[test_case(&[&FS, &[5], b"a", &FS, &[1], b"b"], FourByteStartCode, FourByteLength, &[&len(2), &[5], b"a", &len(2), &[1], b"b"])]
    #[test_case(
        &[&len(2), &[5], b"a", &len(2), &[1], b"b"],
        FourByteLength,
        FourByteStartCode,
        &[&FS, &[5], b"a", &FS, &[1], b"b"]
    )]
    fn convert_bitstream(
        bitstream: &[&[u8]],
        source: BitstreamFraming,
        target: BitstreamFraming,
        expected: &[&[u8]],
    ) {
        let bitstream = bitstream
            .iter()
            .map(|&n| Span::from(n.to_vec()))
            .collect::<Span>();
        let converted_bitstream = super::convert_bitstream(bitstream, source, target);

        let expected = expected
            .iter()
            .flat_map(|&s| s)
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(expected, converted_bitstream.to_bytes());
    }
}
