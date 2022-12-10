use super::{
    ColorType, Decoded, Decoder, TextAlign, TextAlpha, TextCue, TextFill, TextPart,
    TextPosition, TextStyle,
};
use crate::{decoder, MediaInfo, Packet};

use logos::{Lexer, Logos};

use std::{borrow::Borrow, collections::VecDeque, str};

decoder!("ass", AssDecoder::create);

#[derive(Debug, thiserror::Error)]
pub enum AssError {
    #[error("Missing field '{0}'")]
    MissingField(&'static str),
}

pub struct AssDecoder {
    styles: Vec<TextStyle>,
    cues: VecDeque<TextCue>,
}

impl AssDecoder {
    pub fn new() -> Self {
        AssDecoder {
            styles: Vec::new(),
            cues: VecDeque::new(),
        }
    }

    fn create() -> Box<dyn Decoder> {
        Box::new(Self::new())
    }
}

impl Default for AssDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for AssDecoder {
    fn start(&mut self, info: &MediaInfo) -> anyhow::Result<()> {
        Ok(())
    }

    fn feed(&mut self, pkt: Packet) -> anyhow::Result<()> {
        let data = pkt.buffer.to_slice();
        let line = str::from_utf8(data.borrow())?;
        let mut entries = line.split(',');

        let read_order = entries.next().ok_or(AssError::MissingField("ReadOrder"))?;
        let _layer = entries.next().ok_or(AssError::MissingField("Layer"))?;
        let style = entries.next().ok_or(AssError::MissingField("Style"))?;
        let name = entries.next().ok_or(AssError::MissingField("Name"))?;
        let _margin_l = entries.next().ok_or(AssError::MissingField("MarginL"))?;
        let _margin_r = entries.next().ok_or(AssError::MissingField("MarginR"))?;
        let _margin_v = entries.next().ok_or(AssError::MissingField("MarginV"))?;
        let _effect = entries.next().ok_or(AssError::MissingField("Effect"))?;
        let text = entries.as_str();

        let cue = TextCue {
            time: pkt.time.clone(),
            style: style.to_string(),
            text: parse_ass_text(text),
        };

        self.cues.push_back(cue);

        Ok(())
    }

    fn receive(&mut self) -> Option<Decoded> {
        self.cues.pop_front().map(Decoded::Subtitle)
    }
}

fn parse_ass_text(text: &str) -> Vec<TextPart> {
    let mut parts = Vec::new();

    let parser = AssParser::new(text);
    for part in parser {
        match part {
            Ass::Text(text) => {
                parts.push(TextPart::Text(text.to_string()));
            }
            Ass::Italic(on) => parts.push(TextPart::Italic(on)),
            Ass::Fill(pos) => parts.push(TextPart::Fill(pos)),
            Ass::Alpha(pos) => parts.push(TextPart::Alpha(pos)),
            Ass::Position(pos) => parts.push(TextPart::Position(pos)),
            Ass::LineBreak => parts.push(TextPart::LineBreak),
            Ass::SmartBreak => parts.push(TextPart::SmartBreak),
            _ => {}
        }
    }

    parts
}

fn italics<'a>(lex: &mut Lexer<'a, Ass<'a>>) -> Option<bool> {
    let span = lex.slice();

    match &span[2..3] {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn align<'a>(lex: &mut Lexer<'a, Ass<'a>>) -> Option<TextAlign> {
    let span = lex.slice();

    match &span[3..4] {
        "1" => Some(TextAlign::BotLeft),
        "2" => Some(TextAlign::Bot),
        "3" => Some(TextAlign::BotRight),
        "4" => Some(TextAlign::MidLeft),
        "5" => Some(TextAlign::Mid),
        "6" => Some(TextAlign::MidRight),
        "7" => Some(TextAlign::TopLeft),
        "8" => Some(TextAlign::Top),
        "9" => Some(TextAlign::TopRight),
        _ => None,
    }
}

fn fontsize<'a>(lex: &mut Lexer<'a, Ass<'a>>) -> Option<u32> {
    let span = lex.slice();

    span[3..].parse().ok()
}

fn text_fill<'a>(lex: &mut Lexer<'a, Ass<'a>>) -> Option<TextFill> {
    let span = lex.slice();

    let (kind, hex_offset) = match &span[1..2] {
        "c" => (ColorType::Primary, 4),
        "1" => (ColorType::Primary, 5),
        "2" => (ColorType::Karaoke, 5),
        "3" => (ColorType::Outline, 5),
        "4" => (ColorType::Shadow, 5),
        _ => return None,
    };

    let color = u32::from_str_radix(&span[hex_offset..(span.len() - 1)], 16).ok()?;

    Some(TextFill(kind, color))
}

fn text_alpha<'a>(lex: &mut Lexer<'a, Ass<'a>>) -> Option<TextAlpha> {
    let span = lex.slice();

    let (kind, hex_offset) = match &span[1..2] {
        "a" => (ColorType::Primary, 4),
        "1" => (ColorType::Primary, 5),
        "2" => (ColorType::Karaoke, 5),
        "3" => (ColorType::Outline, 5),
        "4" => (ColorType::Shadow, 5),
        _ => return None,
    };

    let alpha = u8::from_str_radix(&span[hex_offset..(span.len() - 1)], 16).ok()?;

    Some(TextAlpha(kind, alpha))
}

fn text_pos<'a>(lex: &mut Lexer<'a, Ass<'a>>) -> Option<TextPosition> {
    let span = lex.slice();

    let inside = &span[5..(span.len() - 1)];
    let (x, y) = inside.split_once(',')?;
    let x = x.parse().ok()?;
    let y = y.parse().ok()?;

    Some(TextPosition(x, y))
}

fn strip_last<'a>(lex: &mut Lexer<'a, AssText<'a>>) -> Option<&'a str> {
    let span = lex.slice();

    Some(&span[..span.len() - 1])
}

#[derive(Debug, PartialEq, Logos)]
pub enum AssText<'a> {
    #[error]
    Error,

    #[regex(r"\\n", priority = 50)]
    LineBreak,

    #[regex(r"\\N", priority = 50)]
    SmartBreak,

    #[regex(r"[^\\]*", priority = 25)]
    Text(&'a str),
}

#[derive(Debug, PartialEq, Logos)]
pub enum Ass<'a> {
    #[regex(r"[ \t\n\f]+", logos::skip)]
    #[error]
    Error,

    #[regex(r"\\i\d", italics)]
    Italic(bool),

    #[regex(r"\\an\d", align, priority = 50)]
    Align(TextAlign),

    #[regex(r"\\r")]
    Reset,

    #[regex(r"\\fs\d+", fontsize)]
    FontSize(u32),

    #[regex(r"\\\d?c&H[a-fA-F0-9]+&", text_fill)]
    Fill(TextFill),

    #[regex(r"\\\d?a&H[a-fA-F0-9]+&", text_alpha)]
    Alpha(TextAlpha),

    #[regex(r"\\pos\(\d+(\.\d+)?,\d+(\.\d+)?\)", text_pos)]
    Position(TextPosition),

    Text(&'a str),
    LineBreak,
    SmartBreak,

    Underline(bool),
    Strikeout(bool),
    Border(i32),
}

struct AssParser<'a> {
    src: &'a str,
    in_braces: bool,
    lexer: Lexer<'a, Ass<'a>>,
    text_lexer: Lexer<'a, AssText<'a>>,
}

impl<'a> AssParser<'a> {
    fn new(src: &'a str) -> Self {
        AssParser {
            src,
            in_braces: false,
            lexer: Ass::lexer(""),
            text_lexer: AssText::lexer(""),
        }
    }
}

impl<'a> Iterator for AssParser<'a> {
    type Item = Ass<'a>;

    fn next(&mut self) -> Option<Ass<'a>> {
        loop {
            if let Some(part) = self.text_lexer.next() {
                match part {
                    AssText::LineBreak => return Some(Ass::LineBreak),
                    AssText::SmartBreak => return Some(Ass::SmartBreak),
                    AssText::Text(txt) => return Some(Ass::Text(txt)),
                    _ => {}
                }
            }

            if let Some(part) = self.lexer.next() {
                return Some(part);
            }

            if self.src.is_empty() {
                return None;
            }

            if !self.in_braces {
                if let Some((text, begin)) = self.src.split_once('{') {
                    self.in_braces = true;
                    self.src = begin;

                    if !text.is_empty() {
                        self.text_lexer = AssText::lexer(text);
                        continue;
                        // return Some(Ass::Text(text));
                    }
                }
            }

            if self.in_braces {
                if let Some((brace, rest)) = self.src.split_once('}') {
                    self.in_braces = false;
                    self.src = rest;

                    self.lexer = Ass::lexer(brace);
                    continue;
                }
            }

            self.text_lexer = AssText::lexer(self.src);
            self.src = &self.src[..0];
        }
    }
}

#[cfg(test)]
mod test {
    use super::Ass::*;
    use super::ColorType::*;
    use super::*;
    use test_case::test_case;

    #[test_case("", &[])]
    #[test_case(r"Yes..\n\nNo!", &[Text("Yes.."), LineBreak, LineBreak, Text("No!")])]
    #[test_case(r"\n", &[LineBreak])]
    #[test_case(r"\N", &[SmartBreak])]
    #[test_case(r"First line\nNext line", &[Text("First line"), LineBreak, Text("Next line")])]
    #[test_case(
        r"{\fs16\c&H00ff00&\an8}Whammy!",
        &[
            FontSize(16),
            Fill(TextFill(Primary, 0x00ff00)),
            Align(TextAlign::Top),
            Text("Whammy!")
        ])]
    #[test_case(r"{\fs16}Whammy!", &[FontSize(16), Text("Whammy!")])]
    #[test_case(r"{\i1}", &[Italic(true)])]
    #[test_case(r"{\i1}{\i0}", &[Italic(true), Italic(false)])]
    #[test_case(r"{\i1} {\i0}", &[Italic(true), Text(" "), Italic(false)])]
    #[test_case(
        r"Foo {\i1}Bar{\r} Baz",
        &[
            Text("Foo "),
            Italic(true),
            Text("Bar"),
            Reset,
            Text(" Baz")
        ])]
    #[test_case(r"{\an1}Hello!", &[Align(TextAlign::BotLeft), Text("Hello!")])]
    #[test_case("just some text!", &[Text("just some text!")])]
    #[test_case(
        r"just {\i1}some{\i0} text!",
        &[
            Text("just "),
            Italic(true),
            Text("some"),
            Italic(false),
            Text(" text!")
        ])]
    #[test_case(
        r"{\c&Hdeadbe&}{\a&H99&}Hello!",
        &[
            Fill(TextFill(Primary, 0xdeadbe)),
            Alpha(TextAlpha(Primary, 0x99)),
            Text("Hello!")
        ])]
    #[test_case(
        r"{\4c&Hdeadbe&}{\4a&H99&}Hello!",
        &[
            Fill(TextFill(Shadow, 0xdeadbe)),
            Alpha(TextAlpha(Shadow, 0x99)),
            Text("Hello!")
        ])]
    #[test_case(
        r"{\pos(123.456,5.0)}Position",
        &[
            Position(TextPosition(123.456, 5.0)),
            Text("Position")
        ])]
    fn parse(ass: &str, expected: &[Ass]) {
        eprintln!("{}", ass);

        let parser = AssParser::new(ass);

        let tokens = parser.collect::<Vec<_>>();

        assert_eq!(&tokens[..], expected);
    }

    #[test_case(
        r"abc\ndef\Nghj",
        &[
            AssText::Text("abc"),
            AssText::LineBreak,
            AssText::Text("def"),
            AssText::SmartBreak,
            AssText::Text("ghj"),
        ])]
    fn parse_text(ass: &str, expected: &[AssText]) {
        eprintln!("{}", ass);

        let lexer = AssText::lexer(ass);

        let tokens = lexer.collect::<Vec<_>>();

        assert_eq!(&tokens[..], expected);
    }
}
