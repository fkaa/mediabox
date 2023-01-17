use std::{collections::HashMap, fmt};

use crate::{MediaInfo, MediaTime, Packet, Track};

pub mod ass;
pub mod h264;
pub mod nal;
pub mod webvtt;

/// Registers a decoder with mediabox
#[macro_export]
macro_rules! decoder {
    ($name:literal, $create:expr) => {
        const META: crate::codec::DecoderMetadata = crate::codec::DecoderMetadata {
            name: $name,
            create: $create,
        };
    };
}

/// Registers an encoder with mediabox
#[macro_export]
macro_rules! encoder {
    ($name:literal, $create:expr) => {
        const META: EncoderMetadata = EncoderMetadata {
            name: $name,
            create: $create,
        };
    };
}

pub trait Decoder {
    fn start(&mut self, info: &MediaInfo) -> anyhow::Result<()>;
    fn feed(&mut self, packet: Packet) -> anyhow::Result<()>;
    fn receive(&mut self) -> Option<Decoded>;
}

pub trait Encoder {
    fn start(&mut self, desc: CodecDescription) -> anyhow::Result<Track>;
    fn feed(&mut self, raw: Decoded) -> anyhow::Result<()>;
    fn receive(&mut self) -> Option<Packet>;
}

#[derive(Clone)]
pub struct DecoderMetadata {
    pub(crate) name: &'static str,
    create: fn() -> Box<dyn Decoder>,
}

impl DecoderMetadata {
    pub fn create(&self) -> Box<dyn Decoder> {
        (self.create)()
    }
}

#[derive(Clone)]
pub struct EncoderMetadata {
    pub(crate) name: &'static str,
    create: fn() -> Box<dyn Encoder>,
}

impl EncoderMetadata {
    pub fn create(&self) -> Box<dyn Encoder> {
        (self.create)()
    }
}

pub enum CodecDescription {
    Subtitle(SubtitleDescription),
}

impl CodecDescription {
    pub fn into_subtitle(self) -> Option<SubtitleDescription> {
        match self {
            CodecDescription::Subtitle(desc) => Some(desc),
        }
    }
}

/// Result from decoding a [`Packet`].
pub enum Decoded {
    Subtitle(TextCue),
}

impl Decoded {
    pub fn into_subtitle(self) -> Option<TextCue> {
        match self {
            Decoded::Subtitle(cue) => Some(cue),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AssCodec {
    pub header: String,
}

#[derive(Clone, Debug)]
pub struct WebVttCodec {
    pub header: String,
}

#[derive(Clone, Debug)]
pub enum SubtitleCodec {
    Ass(AssCodec),
    WebVtt(WebVttCodec),
}

/// Information about a piece of subtitle media
#[derive(Clone)]
pub struct SubtitleInfo {
    pub codec: SubtitleCodec,
}

impl fmt::Debug for SubtitleInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.codec {
            SubtitleCodec::Ass(_) => {
                write!(f, "ASS")?;
            }
            SubtitleCodec::WebVtt(_) => {
                write!(f, "WebVTT")?;
            }
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct SubtitleDescription {
    styles: HashMap<String, TextStyle>,
}

#[derive(Default, Debug)]
pub struct TextStyle {
    font: Option<String>,
    primary_color: Option<u32>,
    secondary_color: Option<u32>,
    outline_color: Option<u32>,
    back_color: Option<u32>,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
    scale_x: f32,
    scale_y: f32,
    spacing: i32,
    angle: i32,
    border_style: Option<i32>,
    outline: Option<i32>,
    shadow: Option<i32>,
    alignment: Option<i32>,
    margin_left: Option<i32>,
    margin_right: Option<i32>,
    margin_vertical: Option<i32>,
}

#[derive(Debug)]
pub struct TextCue {
    pub time: MediaTime,
    pub style: String,
    pub text: Vec<TextPart>,
}

#[derive(Eq, PartialEq, Debug)]
pub enum TextAlign {
    TopLeft,
    Top,
    TopRight,
    MidLeft,
    Mid,
    MidRight,
    BotLeft,
    Bot,
    BotRight,
}

#[derive(Eq, PartialEq, Debug)]
pub enum ColorType {
    Primary,
    Karaoke,
    Outline,
    Shadow,
}

#[derive(Debug, PartialEq)]
pub struct TextPosition(f32, f32);

#[derive(Eq, PartialEq, Debug)]
pub struct TextFill(ColorType, u32);

#[derive(Eq, PartialEq, Debug)]
pub struct TextAlpha(ColorType, u8);

#[derive(Debug)]
pub enum TextPart {
    Text(String),
    Italic(bool),
    Underline(bool),
    Strikeout(bool),
    Border(f32),
    FontSize(u32),
    Position(TextPosition),
    Fill(TextFill),
    Alpha(TextAlpha),
    LineBreak,
    SmartBreak,
}
