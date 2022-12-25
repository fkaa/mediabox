use anyhow::Context;
use h264_reader::{
    Context as H264Context,
    nal::sps::SeqParameterSet,
    nal::pps::PicParameterSet,
    rbsp::{decode_nal, BitReader},
};

use mediabox::format::*;
use mediabox::io::*;
use mediabox::*;

mod cli;

use cli::*;

#[tokio::main]
async fn main() {
    let args = Mbox::from_env_or_exit();

    if let Err(e) = run(args).await {
        eprintln!("{e:?}");
    }
}

async fn run(args: Mbox) -> anyhow::Result<()> {
    match args.subcommand {
        MboxCmd::Analyze(args) => {
            analyze(args).await?;
        }
    }

    Ok(())
}

async fn analyze(args: Analyze) -> anyhow::Result<()> {
    let path = args.input.unwrap();
    let mut io = Io::open_file(&path).await?;
    let mut cxt = MediaContext::default();
    cxt.register_all();

    let meta = cxt.probe(&mut io).await?;
    let mut demuxer = meta.create(io);

    match args.subcommand {
        AnalyzeCmd::Codec(args) => analyze_codec(args, demuxer).await?,
        AnalyzeCmd::Packets(args) => analyze_packets(args, demuxer).await?,
    }

    Ok(())
}

async fn analyze_codec(args: Codec, mut demuxer: Box<dyn Demuxer>) -> anyhow::Result<()> {
    let movie = demuxer.start().await?;

    for track in movie.tracks {
        println!("Track #{} ({}):", track.id, track.info.name);
        if let Err(e) = print_track_codec(track) {
            eprintln!("Failed to parse track codec: {e}");
        }
    }

    Ok(())
}

fn print_track_codec(track: Track) -> anyhow::Result<()> {
    let info = track.info;
    match &info.kind {
        MediaKind::Video(info) => {
            print_video_codec(info)?;
        }
        _ => {}
    }

    Ok(())
}

fn print_video_codec(info: &VideoInfo) -> anyhow::Result<()> {
    match &info.codec {
        VideoCodec::H264(codec) => print_h264_codec(codec)?,
    }

    Ok(())
}

fn print_h264_codec(codec: &H264Codec) -> anyhow::Result<()> {
    let sps_slice = codec.sps.to_slice();
    let nal = decode_nal(&sps_slice[1..])?;

    let reader = BitReader::new(nal.as_ref());
    let sps = SeqParameterSet::from_bits(reader).map_err(|e| anyhow::anyhow!("{:?}", e))?;

    println!("seq_parameter_set_data()");
    println!("\tprofile_idc: {}", u8::from(sps.profile_idc));
    println!("\tconstraint_flags: {:08b}", u8::from(sps.constraint_flags));
    println!("\tlevel_idc: {}", sps.level_idc);
    println!("\tseq_parameter_set_id: {}", sps.seq_parameter_set_id.id());

    let chroma = &sps.chroma_info;
    println!("\tchroma_format_idc: {:?}", chroma.chroma_format);
    println!("\tseparate_colour_plane_flag: {}", chroma.separate_colour_plane_flag);
    println!("\tbit_depth_luma_minus8: {}", chroma.bit_depth_luma_minus8);
    println!("\tbit_depth_chroma_minus8: {}", chroma.bit_depth_chroma_minus8);
    println!("\tqpprime_y_zero_transform_bypass_flag: {}", chroma.qpprime_y_zero_transform_bypass_flag);
    println!("\tscaling_matrix: {:?}", chroma.scaling_matrix);

    println!("\tlog2_max_frame_num_minus4: {}", sps.log2_max_frame_num_minus4);
    println!("\tpic_order_cnt: {:?}", sps.pic_order_cnt);
    println!("\tmax_num_ref_frames: {}", sps.max_num_ref_frames);
    println!("\tgaps_in_frame_num_value_allowed_flag: {}", sps.gaps_in_frame_num_value_allowed_flag);
    println!("\tpic_width_in_mbs_minus1: {}", sps.pic_width_in_mbs_minus1);
    println!("\tpic_height_in_map_units_minus1: {}", sps.pic_height_in_map_units_minus1);
    println!("\tframe_mbs_flags: {:?}", sps.frame_mbs_flags);
    println!("\tdirect_8x8_inference_flag: {}", sps.direct_8x8_inference_flag);

    if let Some(crop) = &sps.frame_cropping {
        println!("\tframe_cropping_flag: true");
        println!("\tframe_crop_left_offset: {}", crop.left_offset);
        println!("\tframe_crop_right_offset: {}", crop.right_offset);
        println!("\tframe_crop_top_offset: {}", crop.top_offset);
        println!("\tframe_crop_bottom_offset: {}", crop.bottom_offset);

    } else {
        println!("\tframe_cropping_flag: false");
    }

    if let Some(vui) = &sps.vui_parameters {
        println!("\tvui_parameters_present_flag: true");
        println!("\tvui: {:#?}", vui);

    } else {
        println!("\tvui_parameters_present_flag: false");
    }

    let mut context = H264Context::new();
    context.put_seq_param_set(sps.clone());

    let pps_slice = codec.pps.to_slice();
    let nal = decode_nal(&pps_slice[1..])?;

    let reader = BitReader::new(nal.as_ref());
    let pps = PicParameterSet::from_bits(&context, reader).map_err(|e| anyhow::anyhow!("{:?}", e))?;

    // println!("{:#?}", pps);

    Ok(())
}

async fn analyze_packets(args: Packets, mut demuxer: Box<dyn Demuxer>) -> anyhow::Result<()> {
    let movie = demuxer.start().await?;

    eprintln!("Tracks:");
    for track in movie.tracks {
        eprintln!("{}\t{:?}", track.id, track.info);
    }
    eprintln!("");

    println!("idx\ttrack\ttime\tsize");
    for i in 0.. {
        let pkt = demuxer.read().await?;

        print!("{i}\t");
        print!("{}\t", pkt.track.id);
        print!("{:?}\t", pkt.time);
        print!("{}\t", pkt.buffer.len());

        //print_packet(i, pkt, &args.packets, &args.nal);

        println!();
    }

    Ok(())
}

fn print_packet(
    idx: usize,
    pkt: Packet,
    packet_filter: &Option<PacketFilter>,
    nal_filter: &Option<NalFilter>,
) {
    if packet_filter.is_some() {}
}
