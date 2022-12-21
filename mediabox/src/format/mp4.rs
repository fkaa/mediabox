use bytes::{BufMut, BytesMut};

use crate::{
    codec::nal::{convert_bitstream, frame_nal_units, BitstreamFraming},
    AudioCodec, AudioInfo, H264Codec, MediaKind, MediaTime, Packet, Span, Track, VideoCodec,
    VideoInfo,
};

// Wonderful macro taken from https://github.com/scottlamb/retina/ examples
macro_rules! write_box {
    ($buf:expr, $fourcc:expr, $b:block) => {
        #[allow(clippy::unnecessary_mut_passed)]
        {
            let _: &mut bytes::BytesMut = $buf; // type-check.
            let pos_start = $buf.len();
            let fourcc: &[u8; 4] = $fourcc;
            $buf.extend_from_slice(&[0, 0, 0, 0, fourcc[0], fourcc[1], fourcc[2], fourcc[3]]);
            let r = {
                $b;
            };
            let pos_end = $buf.len();
            let len = pos_end.checked_sub(pos_start).unwrap();
            $buf[pos_start..pos_start + 4].copy_from_slice(&(len as u32).to_be_bytes()[..]);
            r
        }
    };
}

mod fmp4;
mod mp4;

pub use fmp4::*;
pub use mp4::*;

fn get_packet_sample_data(packet: &Packet) -> Span {
    match packet.track.info.kind {
        MediaKind::Video(VideoInfo {
            codec: VideoCodec::H264(H264Codec {
                bitstream_format, ..
            }),
            ..
        }) => convert_bitstream(
            packet.buffer.clone(),
            bitstream_format,
            BitstreamFraming::FourByteLength,
        ),
        _ => packet.buffer.clone(),
    }
}

fn type_check<R, T: FnOnce(&mut bytes::BytesMut) -> R>(f: T) -> T {
    f
}

macro_rules! write_base_descriptor {
    ($buf:expr, $tag:expr, $b:expr) => {
        #[allow(clippy::unnecessary_mut_passed)]
        {
            let _: &mut bytes::BytesMut = $buf; // type-check.
            let f = type_check($b); // type-check.
            let mut buf = BytesMut::new();
            let r = f(&mut buf);

            write_base_descriptor_header($buf, $tag, buf.len() as u32);
            $buf.extend_from_slice(&buf);

            r
        }
    };
}

fn write_mvhd(buf: &mut BytesMut) {
    write_box!(buf, b"mvhd", {
        buf.put_u32(1 << 24); // version
        buf.put_u64(0); // creation_time
        buf.put_u64(0); // modification_time
        buf.put_u32(1_000); // timescale
        buf.put_u64(0);
        buf.put_u32(0x00010000); // rate
        buf.put_u16(0x0100); // volume
        buf.put_u16(0); // reserved
        buf.put_u64(0); // reserved
        for v in &[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            buf.put_u32(*v); // matrix
        }
        for _ in 0..6 {
            buf.put_u32(0); // pre_defined
        }
        buf.put_u32(u32::MAX); // next_track_id
    });
}

#[derive(Clone)]
struct TrackBuilder {
    track: Track,
    id: u32,
    sample_entries: Vec<SampleEntry>,
}

impl TrackBuilder {
    fn new(track: Track, id: u32) -> Self {
        TrackBuilder {
            track,
            id,
            sample_entries: Vec::new(),
        }
    }

    fn add_sample(&mut self, entry: SampleEntry) {
        self.sample_entries.push(entry);
    }
}

#[derive(Clone)]
struct SampleEntry {
    is_sync: bool,
    size: u64,
    time: MediaTime,
}

fn write_trak(buf: &mut BytesMut, builder: TrackBuilder) -> anyhow::Result<()> {
    let stream = builder.track;
    let track_id = builder.id;

    let timebase = stream.timebase.simplify().denominator;

    write_box!(buf, b"trak", {
        write_tkhd(buf, track_id, 0, 0);

        write_box!(buf, b"mdia", {
            write_mdhd(buf, timebase);
            write_hdlr(buf);

            write_box!(buf, b"minf", {
                match stream.info.kind {
                    MediaKind::Video(_) => {
                        write_box!(buf, b"vmhd", {
                            buf.put_u32(1);
                            buf.put_u64(0);
                        });
                    }
                    MediaKind::Audio(_) => {
                        write_box!(buf, b"soun", {
                            buf.put_u32(1);
                            buf.put_u64(0);
                        });
                    }
                    _ => todo!(),
                }
                write_dinf(buf);

                write_stbl(buf, stream, &builder.sample_entries)?;
            });
        });
    });

    Ok(())
}

fn write_video_trak(buf: &mut BytesMut, builder: TrackBuilder) -> anyhow::Result<()> {
    let stream = builder.track;
    let track_id = builder.id;

    let info = stream
        .info
        .video()
        .expect("Video stream should contain video info");
    let timebase = stream.timebase.simplify().denominator;

    write_box!(buf, b"trak", {
        let width = u32::from(u16::try_from(info.width)?) << 16;
        let height = u32::from(u16::try_from(info.height)?) << 16;

        write_tkhd(buf, track_id, width, height);

        write_box!(buf, b"mdia", {
            write_mdhd(buf, timebase);
            write_hdlr(buf);

            write_box!(buf, b"minf", {
                write_box!(buf, b"vmhd", {
                    buf.put_u32(1);
                    buf.put_u64(0);
                });
                write_dinf(buf);

                write_video_stbl(buf, info, &builder.sample_entries)?;
            });
        });
    });

    Ok(())
}

fn write_audio_trak(buf: &mut BytesMut, builder: TrackBuilder) -> anyhow::Result<()> {
    let stream = builder.track;
    let track_id = builder.id;

    let info = stream
        .info
        .audio()
        .expect("Audio stream should contain audio info");
    let timebase = stream.timebase.simplify().denominator;

    write_box!(buf, b"trak", {
        write_tkhd(buf, track_id, 0, 0);

        write_box!(buf, b"mdia", {
            write_mdhd(buf, timebase);
            write_hdlr(buf);

            write_box!(buf, b"minf", {
                write_box!(buf, b"soun", {
                    buf.put_u32(1);
                    buf.put_u64(0);
                });
                write_dinf(buf);

                write_audio_stbl(buf, info)?;
            });
        });
    });

    Ok(())
}

fn write_stsd(buf: &mut BytesMut, track: Track) -> anyhow::Result<()> {
    write_box!(buf, b"stsd", {
        buf.put_u32(0); // version
        buf.put_u32(1); // entry_count

        match &track.info.kind {
            MediaKind::Video(info) => write_video_sample_entry(buf, info)?,
            MediaKind::Audio(info) => write_audio_sample_description(buf, info)?,
            _ => todo!(),
        }
    });

    Ok(())
}

fn write_stss(buf: &mut BytesMut, entries: &[SampleEntry]) {
    let sync_samples = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            if entry.is_sync {
                Some(idx as u32 + 1)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    write_box!(buf, b"stss", {
        buf.put_u32(0); // version
        buf.put_u32(sync_samples.len() as u32); // len

        for idx in sync_samples {
            buf.put_u32(idx); // sample_number
        }
    });
}

fn write_stbl(buf: &mut BytesMut, track: Track, entries: &[SampleEntry]) -> anyhow::Result<()> {
    write_box!(buf, b"stbl", {
        write_stsd(buf, track)?;
        write_stss(buf, entries);

        write_box!(buf, b"stsc", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stsz", {
            buf.put_u32(0); // version
            buf.put_u32(0); // sample_size
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stco", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
    });

    Ok(())
}

fn write_video_stbl(
    buf: &mut BytesMut,
    info: &VideoInfo,
    entries: &[SampleEntry],
) -> anyhow::Result<()> {
    write_box!(buf, b"stbl", {
        write_box!(buf, b"stsd", {
            buf.put_u32(0); // version
            buf.put_u32(1); // entry_count

            write_video_sample_entry(buf, info)?;
        });
        write_box!(buf, b"stss", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stts", {
            buf.put_u32(0);
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stsc", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stsz", {
            buf.put_u32(0); // version
            buf.put_u32(0); // sample_size
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stco", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
    });

    Ok(())
}

fn write_audio_stbl(buf: &mut BytesMut, info: &AudioInfo) -> anyhow::Result<()> {
    write_box!(buf, b"stbl", {
        write_box!(buf, b"stsd", {
            buf.put_u32(0); // version
            buf.put_u32(1); // entry_count

            write_audio_sample_description(buf, info)?;
        });
        write_box!(buf, b"stss", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stts", {
            buf.put_u32(0);
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stsc", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stsz", {
            buf.put_u32(0); // version
            buf.put_u32(0); // sample_size
            buf.put_u32(0); // len
        });
        write_box!(buf, b"stco", {
            buf.put_u32(0); // version
            buf.put_u32(0); // len
        });
    });

    Ok(())
}

fn write_tkhd(buf: &mut BytesMut, track_id: u32, width: u32, height: u32) {
    write_box!(buf, b"tkhd", {
        buf.put_u32((1 << 24) | 7); // version, flags
        buf.put_u64(0); // creation_time
        buf.put_u64(0); // modification_time
        buf.put_u32(track_id); // track_id
        buf.put_u32(0); // reserved
        buf.put_u64(0); // duration
        buf.put_u64(0); // reserved
        buf.put_u16(0); // layer
        buf.put_u16(0); // alternate_group
        buf.put_u16(0); // volume
        buf.put_u16(0); // reserved
        for v in &[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            buf.put_u32(*v); // matrix
        }
        buf.put_u32(width);
        buf.put_u32(height);
    });
}

fn write_mdhd(buf: &mut BytesMut, timebase: u32) {
    write_box!(buf, b"mdhd", {
        buf.put_u32(1 << 24); // version
        buf.put_u64(0); // creation_time
        buf.put_u64(0); // modification_time
        buf.put_u32(timebase); // timebase
        buf.put_u64(0);
        buf.put_u32(0x55c40000); // language=und + pre-defined
    });
}

fn write_hdlr(buf: &mut BytesMut) {
    write_box!(buf, b"hdlr", {
        buf.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x00, // version + flags
            0x00, 0x00, 0x00, 0x00, // pre_defined
            b's', b'o', b'u', b'n', // handler = vide
            0x00, 0x00, 0x00, 0x00, // reserved[0]
            0x00, 0x00, 0x00, 0x00, // reserved[1]
            0x00, 0x00, 0x00, 0x00, // reserved[2]
            0x00, // name, zero-terminated (empty)
        ]);
    });
}

fn write_dinf(buf: &mut BytesMut) {
    write_box!(buf, b"dinf", {
        write_box!(buf, b"dref", {
            buf.put_u32(0);
            buf.put_u32(1); // entry_count
            write_box!(buf, b"url ", {
                buf.put_u32(1); // version, flags=self-contained
            });
        });
    });
}

fn write_audio_sample_description(buf: &mut BytesMut, info: &AudioInfo) -> anyhow::Result<()> {
    match &info.codec {
        AudioCodec::Aac(params) => {
            write_box!(buf, b"mp4a", {
                write_audio_sample_entry(
                    buf,
                    1,
                    info.sound_type.channel_count(),
                    info.sample_bpp as u16,
                    info.sample_rate,
                );

                write_box!(buf, b"esds", {
                    buf.put_u32(0); // version

                    write_es_descriptor(buf, 2, 0x40, Some(&params.extra));
                });
            });
        }
    }

    Ok(())
}

fn write_video_sample_entry(buf: &mut BytesMut, info: &VideoInfo) -> anyhow::Result<()> {
    match &info.codec {
        VideoCodec::H264(params) => {
            write_box!(buf, b"avc1", {
                write_visual_sample_entry(buf, 1, info.width as u16, info.height as u16);

                write_box!(buf, b"avcC", {
                    buf.extend_from_slice(&[
                        1,
                        params.profile_indication,
                        params.profile_compatibility,
                        params.level_indication,
                        0b0000_0000 | 3, // length_minus_one, 1 + 1 == 2
                        0b0000_0000 | 1, // sps_count
                    ]);

                    let sps =
                        frame_nal_units(&[params.sps.clone()], BitstreamFraming::TwoByteLength);
                    for span in sps.spans() {
                        buf.extend_from_slice(span);
                    }

                    buf.put_u8(1); // pps_count
                    let pps =
                        frame_nal_units(&[params.pps.clone()], BitstreamFraming::TwoByteLength);
                    for span in pps.spans() {
                        buf.extend_from_slice(span);
                    }
                });
            });
        }
    }

    Ok(())
}

fn write_audio_sample_entry(
    buf: &mut BytesMut,
    data_reference_index: u16,
    channel_count: u16,
    sample_size: u16,
    sample_rate: u32,
) {
    write_sample_entry(buf, data_reference_index);

    buf.extend_from_slice(&[0u8; 8]);
    buf.put_u16(channel_count);
    buf.put_u16(sample_size);
    buf.put_u32(0);
    buf.put_u32(sample_rate << 16);
}

fn write_visual_sample_entry(
    buf: &mut BytesMut,
    data_reference_index: u16,
    width: u16,
    height: u16,
) {
    write_sample_entry(buf, data_reference_index);

    buf.extend_from_slice(&[0u8; 16]);
    buf.put_u16(width);
    buf.put_u16(height);
    buf.extend_from_slice(&[
        0x00, 0x48, 0x00, 0x00, // horizresolution
        0x00, 0x48, 0x00, 0x00, // vertresolution
        0x00, 0x00, 0x00, 0x00, // reserved
        0x00, 0x01, // frame count
        0x00, 0x00, 0x00, 0x00, // compressorname
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, //
        0x00, 0x18, 0xff, 0xff, // depth + pre_defined
    ]);
}

fn write_sample_entry(buf: &mut BytesMut, data_reference_index: u16) {
    buf.extend_from_slice(&[0u8; 6]);
    buf.put_u16(data_reference_index);
}

const ES_DESCR_TAG: u8 = 0x3;
const DECODER_CONFIG_DESCR_TAG: u8 = 0x4;
const DECODER_SPECIFIC_DESCR_TAG: u8 = 0x5;
const SL_CONFIG_DESCR_TAG: u8 = 0x6;

fn write_es_descriptor(
    buf: &mut BytesMut,
    es_id: u16,
    object_type_indication: u8,
    decoder_specific: Option<&[u8]>,
) {
    write_base_descriptor!(buf, ES_DESCR_TAG, |buf| {
        buf.put_u16(es_id);
        buf.put_u8(0); // flags and stream priority

        write_base_descriptor!(buf, DECODER_CONFIG_DESCR_TAG, |buf| {
            buf.put_u8(object_type_indication);
            buf.put_u8((0x05 << 2) | 1); // streamtype + upstream + reserved
            buf.extend_from_slice(&[0u8; 11]);

            if let Some(specific) = decoder_specific {
                write_base_descriptor!(buf, DECODER_SPECIFIC_DESCR_TAG, |buf| {
                    buf.extend_from_slice(specific);
                });
            }
        });

        // SL config descriptor
        write_base_descriptor!(buf, SL_CONFIG_DESCR_TAG, |buf| {
            buf.put_u8(2);
        });
    });
}

fn write_base_descriptor_header(buf: &mut BytesMut, tag: u8, size: u32) {
    buf.put_u8(tag);

    let size = 1 + size - size_of_length(size);
    let length_byte_count = size_of_length(size);

    for i in 0..length_byte_count {
        let offset = (length_byte_count - (i + 1)) * 7;
        let mut size = (size >> offset & 0b0111_1111) as u8;
        if (i + 1) < length_byte_count {
            size |= 0b1000_0000;
        }

        buf.put_u8(size);
    }
}

fn size_of_length(size: u32) -> u32 {
    match size {
        0x0..=0x7F => 1,
        0x80..=0x3FFF => 2,
        0x4000..=0x1FFFFF => 3,
        _ => 4,
    }
}
