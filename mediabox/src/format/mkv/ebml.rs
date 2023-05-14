use std::str::Utf8Error;

use bytes::{BufMut, BytesMut};
use nom::{bytes::streaming::take, error::ParseError, sequence::pair, IResult, Needed, Parser};

use crate::{format::DemuxerError, io::Io, Span};

use super::*;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct EbmlId(pub u64);

impl std::fmt::Debug for EbmlId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EbmlId(0x{:x})", self.0)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum EbmlLength {
    Known(u64),
    Unknown(u8),
}
impl EbmlLength {
    pub fn require(self) -> Result<u64, EbmlError> {
        let EbmlLength::Known(size) = self else {
            return Err(EbmlError::UnknownSize);
        };

        Ok(size)
    }
}
#[derive(Debug)]
pub struct EbmlMasterElement<'a>(pub EbmlId, pub &'a [EbmlElement<'a>]);

#[derive(Debug)]
pub struct EbmlElement<'a>(pub EbmlId, pub EbmlValue<'a>);
#[derive(Debug)]
pub enum EbmlValue<'a> {
    Int(i64),
    UInt(u64),
    String(&'a str),
    Binary(Span<'a>),
    Children(&'a [EbmlElement<'a>]),
}

impl<'a> EbmlValue<'a> {
    pub fn size(&self) -> u64 {
        match self {
            &EbmlValue::Int(value) => int_element_bytes_required(value) as u64,
            &EbmlValue::UInt(value) => uint_element_bytes_required(value) as u64 + 1,
            EbmlValue::String(string) => string.as_bytes().len() as u64,
            EbmlValue::Binary(binary) => binary.len() as u64,
            EbmlValue::Children(el) => el.iter().map(|el| el.full_size()).sum::<u64>(),
        }
    }

    pub fn write(&self, buf: &mut dyn BufMut) {
        match self {
            &EbmlValue::Int(value) => write_int_elem(buf, value),
            &EbmlValue::UInt(value) => write_uint_elem(buf, value),
            EbmlValue::String(string) => buf.put_slice(string.as_bytes()),
            EbmlValue::Binary(binary) => {
                binary.visit(&mut |b| buf.put_slice(b));
            }
            EbmlValue::Children(el) => {
                for el in *el {
                    el.write(buf);
                }
            }
        }
    }
}

impl EbmlId {
    pub fn size(&self) -> u64 {
        (self.0.ilog2() as u64 + 7) / 8
    }

    pub fn write(&self, buf: &mut dyn BufMut) {
        write_vid(buf, self.0);
    }
}

impl EbmlLength {
    pub fn size(&self) -> u64 {
        match self {
            &EbmlLength::Known(length) => vint_bytes_required(length),
            &EbmlLength::Unknown(bytes) => 1,
        }
    }

    pub fn write(&self, buf: &mut dyn BufMut) {
        match self {
            &EbmlLength::Known(length) => write_vint(buf, length),
            &EbmlLength::Unknown(bytes) => buf.put_u8(0b1111_1111),
        }
    }
}

impl<'a> EbmlMasterElement<'a> {
    pub fn full_size(&self) -> u64 {
        let content_size = self.size();

        self.0.size() + EbmlLength::Known(content_size).size() + content_size
        // self.0.size() + self.size()
    }

    fn size(&self) -> u64 {
        self.1.iter().map(|v| v.full_size()).sum::<u64>()
    }

    pub fn write(&self, buf: &mut dyn BufMut) {
        self.0.write(buf);
        EbmlLength::Known(self.size()).write(buf);

        for element in self.1 {
            element.write(buf);
        }
    }
}

impl<'a> EbmlElement<'a> {
    fn full_size(&self) -> u64 {
        let content_size = self.size();

        self.0.size() + EbmlLength::Known(content_size).size() + content_size
    }

    fn size(&self) -> u64 {
        self.1.size()
    }

    pub fn write(&self, buf: &mut dyn BufMut) {
        self.0.write(buf);
        EbmlLength::Known(self.size()).write(buf);

        self.1.write(buf);
    }
}

pub fn write_ebml<F: FnOnce(&mut dyn BufMut) -> R, R: Into<Span<'static>>>(
    id: EbmlId,
    func: F,
) -> Span<'static> {
    let mut content = BytesMut::new();
    let span = func(&mut content);

    let mut buf = BytesMut::with_capacity(8);
    id.write(&mut buf);
    EbmlLength::Known(content.len() as u64).write(&mut buf);

    [Span::from(buf.freeze()), Span::from(content.freeze())]
        .into_iter()
        .collect()
}

fn t() {
    write_ebml(EBML_HEADER, |buf| {
        write_ebml(EBML_DOC_TYPE, |buf| write_vstr(buf, "matroska"))
    });
}

#[macro_export]
macro_rules! write_ebml {
    ($id:expr, $buf:ident => [$($b:expr),*]) => {
        {
            let mut content_spans = Vec::new();

            $(
                let mut $buf = bytes::BytesMut::new();
                let b = $b;
                // dbg!(&$buf);
                dbg!(&b);
                if ($buf.len() > 0) {
                    content_spans.push($crate::Span::from($buf.freeze()));
                }
            )*

            let content = content_spans.into_iter().collect::<$crate::Span>();

            let mut buf = bytes::BytesMut::with_capacity(8);
            $crate::format::mkv::ebml::write_vid(&mut buf, $id as u64);
            $crate::format::mkv::ebml::write_vint(&mut buf, content.len() as u64);

            [$crate::Span::from(buf.freeze()), content].into_iter().collect::<$crate::Span>()
        }
    }
}

#[macro_export]
macro_rules! ebml {
    ($io:expr, $size:expr, $( $pat:pat_param => $blk:block ),* ) => {
        let mut i = 0;
        while i < $size {
            let (len, id) = vid($io).await?;
            i += len as u64;
            let (len, size) = vint($io).await?;
            i += len as u64;

            match (id, size) {
                $( $pat => $blk, )*
                _ => {
                    log::debug!("Ignoring element: 0x{id:08x} ({size} B) ({i}/{})", $size);

                    $io.skip(size).await?;
                }
            }

            i += size;
        }
    }
}

pub async fn vstr(io: &mut Io, size: u64) -> Result<String, MkvError> {
    let mut data = vec![0u8; size as usize];

    io.read_exact(&mut data).await?;

    Ok(String::from_utf8(data)?)
}

pub fn write_vstr(buf: &mut dyn BufMut, string: &str) {
    write_vint(buf, string.as_bytes().len() as u64);
    buf.put_slice(string.as_bytes());
}

pub async fn vfloat(io: &mut Io, size: u64) -> Result<f64, MkvError> {
    let mut data = [0u8; 8];

    let value = match size {
        0 => 0.0,
        4 => {
            io.read_exact(&mut data[..4]).await?;

            f32::from_be_bytes(data[..4].try_into().unwrap()) as f64
        }
        8 => {
            io.read_exact(&mut data[..8]).await?;

            f64::from_be_bytes(data)
        }
        _ => return Err(MkvError::InvalidFloatSize(size)),
    };

    Ok(value)
}

pub async fn vu(io: &mut Io, size: u64) -> Result<u64, MkvError> {
    if size > 8 {
        return Err(MkvError::UnsupportedVint(size));
    }

    let mut data = [0u8; 8];
    io.read_exact(&mut data[..size as usize]).await?;

    let mut value = 0u64;
    for i in 0..size {
        value <<= 8;
        value |= data[i as usize] as u64;
    }

    Ok(value)
}

pub async fn uint_elem(io: &mut Io) -> Result<u64, MkvError> {
    use tokio::io::AsyncReadExt;

    let reader = io.reader()?;

    let len = reader.read_u8().await?;

    if len > 7 {
        return Err(MkvError::UnsupportedVint(len as u64));
    }

    let mut bytes = [0u8; 7];
    if len > 0 {
        reader.read_exact(&mut bytes[..len as usize]).await?;
    }

    let mut value = 0;

    for i in 0..len {
        value <<= 8;
        value |= bytes[i as usize] as u64;
    }

    Ok(value)
}

#[derive(Debug, thiserror::Error)]
pub enum EbmlError {
    #[error("element")]
    Element(&'static str),
    #[error("Unexpected EBML element. Expected {0:?} but found {1:?} ({2:?}.")]
    UnexpectedElement(EbmlId, EbmlId, EbmlLength),
    #[error("Expected known size, but was unknown")]
    UnknownSize,
    #[error("Unsupported length size: {0}")]
    UnsupportedSize(u8),
    #[error("{0}")]
    InvalidString(Utf8Error),
}

impl<'a> ParseError<&'a [u8]> for EbmlError {
    fn from_error_kind(input: &'a [u8], kind: nom::error::ErrorKind) -> Self {
        EbmlError::Element("test")
    }

    fn append(input: &'a [u8], kind: nom::error::ErrorKind, other: Self) -> Self {
        other
    }
}

impl From<EbmlError> for nom::Err<EbmlError> {
    fn from(value: EbmlError) -> Self {
        nom::Err::Error(value)
    }
}

impl From<nom::Err<EbmlError>> for DemuxerError {
    fn from(value: nom::Err<EbmlError>) -> Self {
        match value {
            nom::Err::Incomplete(Needed::Size(sz)) => DemuxerError::NeedMore(sz.into()),
            nom::Err::Incomplete(Needed::Unknown) => DemuxerError::NeedMore(4096),
            nom::Err::Error(e) => DemuxerError::Misc(e.into()),
            nom::Err::Failure(_) => todo!(),
        }
    }
}

pub fn ebml_vint(input: &[u8]) -> IResult<&[u8], u64, EbmlError> {
    if input.is_empty() {
        return Err(nom::Err::Incomplete(Needed::new(1)));
    }

    let byte = input[0];
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 7 {
        todo!()
    }

    if input.len() < len {
        return Err(nom::Err::Incomplete(Needed::new(len - input.len())));
    }

    let mut value = byte as u64 & ((1 << (8 - len)) - 1) as u64;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= input[1 + i as usize] as u64;
    }

    Ok((&input[len..], value))
}

pub fn ebml_len(input: &[u8]) -> IResult<&[u8], EbmlLength, EbmlError> {
    if input.is_empty() {
        return Err(nom::Err::Incomplete(Needed::new(1)));
    }

    let byte = input[0];
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 7 {
        todo!()
    }

    if input.len() < len {
        return Err(nom::Err::Incomplete(Needed::new(len - input.len())));
    }

    let mut value = byte as u64 & ((1 << (8 - len)) - 1) as u64;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= input[1 + i as usize] as u64;
    }

    let length = if value == 1 << (7 * len) {
        EbmlLength::Unknown(len as u8)
    } else {
        EbmlLength::Known(value)
    };

    Ok((&input[len..], length))
}

pub fn ebml_vid(input: &[u8]) -> IResult<&[u8], EbmlId, EbmlError> {
    if input.is_empty() {
        return Err(nom::Err::Incomplete(Needed::new(1)));
    }

    let byte = input[0];
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 7 {
        return Err(EbmlError::UnsupportedSize(extra_bytes).into());
    }

    if input.len() < len {
        return Err(nom::Err::Incomplete(Needed::new(len - input.len())));
    }

    let mut value = byte as u64;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= input[1 + i as usize] as u64;
    }

    Ok((&input[len..], EbmlId(value)))
}

pub fn ebml_int(input: &[u8], size: usize) -> IResult<&[u8], u64, EbmlError> {
    if input.len() < size {
        return Err(nom::Err::Incomplete(Needed::new(size - input.len())));
    }

    let value = input[..size]
        .iter()
        .fold(0, |acc, b| (acc << 8) | *b as u64);

    Ok((&input[size..], value))
}

pub fn ebml_master_element_fold<'a, F, Q>(
    expected_id: EbmlId,
    default: Q,
    mut parser: F,
) -> impl FnMut(&'a [u8]) -> IResult<&'a [u8], Q, EbmlError>
where
    Q: Clone,
    F: FnMut(&mut Q, &'a [u8]) -> Result<(), nom::Err<EbmlError>>,
{
    move |input| {
        let mut default = default.clone();
        let (input, (id, len)) = ebml_element_header()(input)?;

        // eprintln!("id={id:?}, len={len:?}");

        if id != expected_id {
            return Err(nom::Err::Error(EbmlError::UnexpectedElement(
                expected_id,
                id,
                len,
            )));
        }

        let len = len.require()?;

        let (remaining, mut input) = take(len)(input)?;

        while !input.is_empty() {
            let (remaining, (id, len)) = ebml_element_header()(input)?;
            // eprintln!("> id={id:?}, len={len:?}");
            let len = len.require()? as usize;

            parser(&mut default, input)?;

            input = &remaining[len..];
        }

        Ok((remaining, default))
    }
}

pub fn ebml_element<'a, P, F, T>(
    expected_id: EbmlId,
    parser: F,
) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], T, EbmlError>
where
    P: Parser<&'a [u8], T, EbmlError>,
    F: Fn(EbmlLength) -> P,
{
    move |input| {
        let (input, (id, length)) = ebml_element_header()(input)?;

        if id != expected_id {
            return Err(nom::Err::Error(EbmlError::UnexpectedElement(
                expected_id,
                id,
                length,
            )));
        }

        parser(length).parse(input)
    }
}

pub fn ebml_element_header<'a>(
) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], (EbmlId, EbmlLength), EbmlError> {
    move |input| pair(ebml_vid, ebml_len)(input)
}

pub fn ebml_match<'a>(id: EbmlId) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], &'a [u8], EbmlError> {
    ebml_element(id, |size| {
        move |input: &'a [u8]| {
            let size = size.require()?;

            let (remaining, bytes) = take(size)(input)?;

            Ok((remaining, bytes))
        }
    })
}

pub fn ebml_uint<'a>(id: EbmlId) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], u64, EbmlError> {
    ebml_element(id, |size| {
        move |input: &'a [u8]| {
            let size = size.require()?;

            let (remaining, bytes) = take(size)(input)?;

            let value = bytes
                .iter()
                .fold(0u64, |acc, val| (acc << 8u64) | *val as u64);

            Ok((remaining, value))
        }
    })
}

pub fn ebml_float<'a>(id: EbmlId) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], f64, EbmlError> {
    ebml_element(id, |size| {
        move |input: &'a [u8]| {
            let size = size.require()?;

            let (remaining, bytes) = take(size)(input)?;

            let value = if bytes.len() >= 8 {
                f64::from_be_bytes(bytes[..8].try_into().unwrap())
            } else if bytes.len() >= 4 {
                f32::from_be_bytes(bytes[..4].try_into().unwrap()) as f64
            } else {
                0f64
            };

            Ok((remaining, value))
        }
    })
}

pub fn ebml_int2<'a>(id: EbmlId) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], i64, EbmlError> {
    ebml_element(id, |size| {
        move |input: &'a [u8]| {
            let size = size.require()?;

            let (remaining, bytes) = take(size)(input)?;

            let value = bytes
                .iter()
                .fold(0i64, |acc, val| (acc << 8i64) | *val as i64);

            Ok((remaining, value))
        }
    })
}

pub fn ebml_str<'a>(id: EbmlId) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], &'a str, EbmlError> {
    ebml_element(id, |size| {
        move |input: &'a [u8]| {
            let size = size.require()?;

            let (remaining, bytes) = take(size)(input)?;

            let value = std::str::from_utf8(bytes).map_err(EbmlError::InvalidString)?;

            Ok((remaining, value))
        }
    })
}

pub fn ebml_bin<'a>(id: EbmlId) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], &'a [u8], EbmlError> {
    ebml_element(id, |size| {
        move |input: &'a [u8]| {
            let size = size.require()?;

            let (remaining, bytes) = take(size)(input)?;

            Ok((remaining, bytes))
        }
    })
}

pub async fn vint(io: &mut Io) -> Result<(u8, u64), MkvError> {
    use tokio::io::AsyncReadExt;

    let reader = io.reader()?;

    let byte = reader.read_u8().await?;
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 7 {
        return Err(MkvError::UnsupportedVint(extra_bytes as u64));
    }

    let mut bytes = [0u8; 7];
    if extra_bytes > 0 {
        reader
            .read_exact(&mut bytes[..extra_bytes as usize])
            .await?;
    }

    let mut value = byte as u64 & ((1 << (8 - len)) - 1) as u64;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= bytes[i as usize] as u64;
    }

    Ok((len as u8, value))
}

pub async fn vid(io: &mut Io) -> Result<(u8, u64), MkvError> {
    use tokio::io::AsyncReadExt;

    let reader = io.reader()?;

    let byte = reader.read_u8().await?;
    let extra_bytes = byte.leading_zeros() as u8;
    let len = 1 + extra_bytes as usize;

    if extra_bytes > 3 {
        return Err(MkvError::UnsupportedVid(extra_bytes));
    }

    let mut bytes = [0u8; 3];
    if extra_bytes > 0 {
        reader
            .read_exact(&mut bytes[..extra_bytes as usize])
            .await?;
    }

    let mut value = byte as u64;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= bytes[i as usize] as u64;
    }

    Ok((len as u8, value))
}

pub fn write_vint(buf: &mut dyn BufMut, mut value: u64) {
    let bytes_required = vint_bytes_required(value);
    let len = 1 << (8 - bytes_required);

    value |= len << ((bytes_required - 1) * 8);

    let bytes = value.to_be_bytes();

    buf.put_slice(&bytes[8 - bytes_required as usize..]);
}

pub fn write_vid(buf: &mut dyn BufMut, id: u64) {
    let len = (id.ilog2() + 7) / 8;

    for i in (0..len).rev() {
        buf.put_u8((id >> (i * 8)) as u8);
    }
}

fn write_int_elem(buf: &mut dyn BufMut, mut value: i64) {
    while value > 0 {
        buf.put_u8((value & 0xff) as u8);

        value >>= 8;
    }
}

fn write_uint_elem(buf: &mut dyn BufMut, mut value: u64) {
    while value > 0 {
        buf.put_u8((value & 0xff) as u8);

        value >>= 8;
    }
}

fn int_element_bytes_required(value: i64) -> u8 {
    if value == 0 {
        return 1;
    }

    (value.ilog2() as u8) / 8
}

fn uint_element_bytes_required(value: u64) -> u8 {
    if value == 0 {
        return 1;
    }

    (value.ilog2() as u8) / 8
}

pub fn vint_bytes_required(value: u64) -> u64 {
    if value == 0 {
        return 1;
    }

    match value.ilog2() + 1 {
        0..=7 => 1,
        8..=14 => 2,
        15..=21 => 3,
        22..=28 => 4,
        29..=35 => 5,
        36..=42 => 6,
        43..=49 => 7,
        50..=56 => 8,
        _ => todo!("error"),
    }
}

#[cfg(test)]
mod test {
    /*use super::*;
    use assert_matches::assert_matches;
    use std::io::Cursor;
    use test_case::test_case;*/

    /*#[test_case(&[0b1000_0010], 2)]
    #[test_case(&[0b0100_0000, 0b0000_0010], 2)]
    #[test_case(&[0b0010_0000, 0b0000_0000, 0b0000_0010], 2)]
    #[test_case(&[0b0001_0000, 0b0000_0000, 0b0000_0000, 0b0000_0010], 2)]
    #[tokio::test]
    async fn test_vint(bytes: &[u8], expected: u64) {
        let cursor = Cursor::new(bytes.to_vec());
        let mut io = Io::from_reader(Box::new(cursor));

        let value = super::vint(&mut io).await;

        assert_matches!(value, Ok(expected));
    }

    #[test_case(0)]
    #[test_case(1)]
    #[test_case(u8::max_value() as u64)]
    #[test_case(u8::max_value() as u64 + 1)]
    #[test_case(u16::max_value() as u64)]
    #[test_case(u16::max_value() as u64 + 1)]
    #[test_case(u32::max_value() as u64)]
    #[test_case(u32::max_value() as u64 + 1)]
    #[test_case((1u64 << 56) - 1)]
    #[tokio::test]
    async fn read_write_vint(expected_value: u64) {
        let mut buf = BytesMut::new();
        write_vint(&mut buf, expected_value);

        let mut io = Io::from_reader(Box::new(Cursor::new(buf.to_vec())));
        let (_len, value) = super::vint(&mut io).await.unwrap();

        assert_eq!(expected_value, value);
    }

    #[test_case(EBML_HEADER as u64)]
    #[test_case(EBML_DOC_TYPE as u64)]
    #[test_case(SEGMENT as u64)]
    #[test_case(TRACK_ENTRY as u64)]
    #[tokio::test]
    async fn read_write_vid_u32(expected_value: u64) {
        let mut buf = BytesMut::new();
        EbmlId(expected_value).write(&mut buf);

        let mut io = Io::from_reader(Box::new(Cursor::new(buf.to_vec())));
        let (_len, value) = super::vid(&mut io).await.unwrap();

        assert_eq!(expected_value as u64, value as u64);
    }

    #[tokio::test]
    async fn read_write_ebml() {
        let header = EbmlMasterElement(
            EbmlId(EBML_HEADER),
            vec![
                EbmlElement(EbmlId(EBML_DOC_TYPE), EbmlValue::String("matroska".into())),
                EbmlElement(EbmlId(EBML_DOC_TYPE_VERSION), EbmlValue::UInt(1)),
            ],
        );

        let mut bytes = BytesMut::new();
        header.write(&mut bytes);

        let mut doc_type = None;
        let mut doc_version = None;

        let len = bytes.len();
        dbg!(&bytes);

        let bytes = bytes.to_vec();
        let mut io = Io::from_reader(Box::new(Cursor::new(bytes)));

        let a: anyhow::Result<()> = try {
            ebml!(&mut io, len as u64,
                (EBML_HEADER, size) => {
                    eprintln!("siz={size}"
                    );
                    ebml!(&mut io, size,
                        (self::EBML_DOC_TYPE, size) => {
                            doc_type = Some(vstr(&mut io, size).await.unwrap());
                        },
                        (self::EBML_DOC_TYPE_VERSION, size) => {
                            doc_version = Some(vu(&mut io, size).await.unwrap());
                        }
                    );
                }
            );
        };

        assert_eq!(doc_type.as_deref(), Some("matroska"));
        assert_eq!(doc_version, Some(1));
    }*/
}
