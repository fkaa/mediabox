use bytes::BytesMut;

use crate::{
    codec::{nal::get_codec_from_mp4, AssCodec, SubtitleCodec, SubtitleInfo},
    demuxer,
    format::{ProbeResult, Demuxer, Movie},
    io::Io,
    AacCodec, AudioCodec, AudioInfo, Fraction, MediaInfo, MediaKind, MediaTime, Packet, SoundType,
    Track,
};

use super::*;

pub async fn vstr(io: &mut Io, size: u64) -> Result<String, MkvError> {
    let mut data = vec![0u8; size as usize];

    io.read_exact(&mut data).await?;

    Ok(String::from_utf8(data)?)
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

pub async fn vid(io: &mut Io) -> Result<(u8, u32), MkvError> {
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

    let mut value = byte as u32;

    for i in 0..extra_bytes {
        value <<= 8;
        value |= bytes[i as usize] as u32;
    }

    Ok((len as u8, value))
}

fn write_vint(buf: &mut BytesMut, value: u64) {

}


#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;
    use std::io::Cursor;
    use test_case::test_case;

    #[test_case(&[0b1000_0010], 2)]
    #[test_case(&[0b0100_0000, 0b0000_0010], 2)]
    #[test_case(&[0b0010_0000, 0b0000_0000, 0b0000_0010], 2)]
    #[test_case(&[0b0001_0000, 0b0000_0000, 0b0000_0000, 0b0000_0010], 2)]
    #[tokio::test]
    async fn vint(bytes: &[u8], expected: u64) {
        let cursor = Cursor::new(bytes.to_vec());
        let mut io = Io::from_reader(Box::new(cursor));

        let value = super::vint(&mut io).await;

        assert_matches!(value, Ok(expected));
    }

    #[tokio::test]
    async fn read_write_vint() {
        let mut buf = BytesMut::new();

        for i in 0..100 { // u32::max_value() {
            buf.clear();
            write_vint(&mut buf, i);

            let mut io = Io::from_reader(Box::new(Cursor::new(buf.to_vec())));
            let (_len, value) = super::vint(&mut io).await.unwrap();

            assert_eq!(i, value);
        }
    }
}
