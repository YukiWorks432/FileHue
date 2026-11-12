//! PSD・PSBの画像資源から保存済みサムネイルを取り出す。

use crate::Error;
use std::io::{Read, Seek, SeekFrom};

const MAX_RESOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCES: usize = 10_000;
const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: u64 = 4096 * 4096;

/// Photoshop 4.0の資源は色の並びが後の版と異なる。
#[derive(Debug, PartialEq, Eq)]
pub enum ColorOrder {
    Rgb,
    Bgr,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ThumbnailData {
    Jpeg(Vec<u8>),
    Raw { row_bytes: u32, pixels: Vec<u8> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum DocumentVersion {
    Psd,
    Psb,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub version: DocumentVersion,
    pub width: u32,
    pub height: u32,
    pub color_order: ColorOrder,
    pub data: ThumbnailData,
}

/// 埋め込み画像がなければ `Ok(None)` を返す。
///
/// 画像資源の本体は選択したサムネイル以外読み込まない。
pub fn extract<R: Read + Seek>(reader: &mut R) -> Result<Option<Thumbnail>, Error> {
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    if length < 34 || read_array::<4>(reader)? != *b"8BPS" {
        return Err(Error::Invalid);
    }
    let version = match read_u16(reader)? {
        1 => DocumentVersion::Psd,
        2 => DocumentVersion::Psb,
        _ => return Err(Error::Unsupported),
    };
    reader.seek(SeekFrom::Start(26))?;
    let color_length = u64::from(read_u32(reader)?);
    let resources_length_offset = bounded_end(reader.stream_position()?, color_length, length)?;
    reader.seek(SeekFrom::Start(resources_length_offset))?;
    let resources_length = u64::from(read_u32(reader)?);
    let end = bounded_end(reader.stream_position()?, resources_length, length)?;
    let mut chosen = None;

    for _ in 0..MAX_RESOURCES {
        let position = reader.stream_position()?;
        if position == end {
            break;
        }
        if end - position < 12 || read_array::<4>(reader)? != *b"8BIM" {
            return Err(Error::Invalid);
        }
        let id = read_u16(reader)?;
        let name_length = u64::from(read_array::<1>(reader)?[0]);
        let name_bytes = name_length + ((name_length + 1) % 2);
        let size_offset = bounded_end(reader.stream_position()?, name_bytes, end)?;
        bounded_end(size_offset, 4, end)?;
        reader.seek(SeekFrom::Start(size_offset))?;
        let size = u64::from(read_u32(reader)?);
        let data_start = reader.stream_position()?;
        let next = bounded_end(data_start, size + size % 2, end)?;
        if id == 1036 || (id == 1033 && chosen.is_none()) {
            chosen = Some((id, data_start, size));
        }
        reader.seek(SeekFrom::Start(next))?;
    }
    if reader.stream_position()? != end {
        return Err(Error::Limit);
    }
    let Some((id, offset, size)) = chosen else {
        return Ok(None);
    };
    if size < 28 {
        return Err(Error::Invalid);
    }
    if size > MAX_RESOURCE_BYTES {
        return Err(Error::Limit);
    }
    reader.seek(SeekFrom::Start(offset))?;
    let format = read_u32(reader)?;
    let width = read_u32(reader)?;
    let height = read_u32(reader)?;
    let row_bytes = read_u32(reader)?;
    let _total_bytes = read_u32(reader)?;
    let compressed_bytes = u64::from(read_u32(reader)?);
    let bits = read_u16(reader)?;
    let planes = read_u16(reader)?;
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::Limit);
    }
    if bits != 24 || planes != 1 || compressed_bytes > size - 28 {
        return Err(Error::Invalid);
    }
    if format == 0
        && (u64::from(row_bytes) < u64::from(width) * 3
            || u64::from(row_bytes) * u64::from(height) != compressed_bytes)
    {
        return Err(Error::Invalid);
    }
    if format > 1 {
        return Err(Error::Unsupported);
    }
    let mut bytes = vec![0; compressed_bytes as usize];
    reader.read_exact(&mut bytes)?;
    let data = if format == 1 {
        ThumbnailData::Jpeg(bytes)
    } else {
        ThumbnailData::Raw {
            row_bytes,
            pixels: bytes,
        }
    };
    Ok(Some(Thumbnail {
        version,
        width,
        height,
        color_order: if id == 1033 {
            ColorOrder::Bgr
        } else {
            ColorOrder::Rgb
        },
        data,
    }))
}

fn bounded_end(start: u64, count: u64, end: u64) -> Result<u64, Error> {
    start
        .checked_add(count)
        .filter(|position| *position <= end)
        .ok_or(Error::Invalid)
}

fn read_array<const N: usize>(reader: &mut impl Read) -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_u16(reader: &mut impl Read) -> Result<u16, Error> {
    Ok(u16::from_be_bytes(read_array(reader)?))
}

fn read_u32(reader: &mut impl Read) -> Result<u32, Error> {
    Ok(u32::from_be_bytes(read_array(reader)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    fn resource(id: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = b"8BIM".to_vec();
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload);
        if !payload.len().is_multiple_of(2) {
            bytes.push(0);
        }
        bytes
    }

    fn jpeg_resource(id: u16, jpeg: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        for value in [1u32, 2, 1, 8, 8, jpeg.len() as u32] {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&24u16.to_be_bytes());
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.extend_from_slice(jpeg);
        resource(id, &payload)
    }

    fn document(version: u16, resources: &[u8]) -> Vec<u8> {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(&[0; 20]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&(resources.len() as u32).to_be_bytes());
        bytes.extend_from_slice(resources);
        bytes
    }

    #[test]
    fn prefers_modern_thumbnail_and_skips_unrelated_odd_resource() {
        let mut resources = resource(1000, &[11, 12, 13]);
        resources.extend(jpeg_resource(1033, &[0xff, 0xd8, 1, 0xd9]));
        resources.extend(jpeg_resource(1036, &[0xff, 0xd8, 2, 0xd9]));
        let result = extract(&mut Cursor::new(document(1, &resources)))
            .unwrap()
            .unwrap();
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 1);
        assert_eq!(result.color_order, ColorOrder::Rgb);
        assert_eq!(result.data, ThumbnailData::Jpeg(vec![0xff, 0xd8, 2, 0xd9]));
    }

    struct LargeDocument {
        cursor: Cursor<Vec<u8>>,
        bytes_read: usize,
    }

    impl Read for LargeDocument {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.cursor.read(buffer)?;
            self.bytes_read += count;
            Ok(count)
        }
    }

    impl Seek for LargeDocument {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            if let SeekFrom::End(0) = position {
                self.cursor.set_position(8 * 1024 * 1024 * 1024);
                return Ok(self.cursor.position());
            }
            self.cursor.seek(position)
        }
    }

    #[test]
    fn psb_extracts_preview_without_reading_large_document() {
        let bytes = document(2, &jpeg_resource(1036, &[0xff, 0xd8, 0xff, 0xd9]));
        let mut reader = LargeDocument {
            cursor: Cursor::new(bytes),
            bytes_read: 0,
        };
        let thumbnail = extract(&mut reader).unwrap().unwrap();
        assert_eq!(
            thumbnail.data,
            ThumbnailData::Jpeg(vec![0xff, 0xd8, 0xff, 0xd9])
        );
        assert!(reader.bytes_read < 100);
    }

    #[test]
    fn absent_and_truncated_resource_sections_are_distinct() {
        assert_eq!(extract(&mut Cursor::new(document(1, &[]))).unwrap(), None);
        let mut invalid = document(1, &resource(1036, &[0; 28]));
        invalid.truncate(invalid.len() - 1);
        assert!(matches!(
            extract(&mut Cursor::new(invalid)),
            Err(Error::Invalid)
        ));
    }

    #[test]
    fn legacy_uncompressed_thumbnail_preserves_padding_and_color_order() {
        let mut payload = Vec::new();
        for value in [0u32, 1, 1, 4, 4, 4] {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&24u16.to_be_bytes());
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.extend_from_slice(&[7, 8, 9, 0]);
        let thumbnail = extract(&mut Cursor::new(document(1, &resource(1033, &payload))))
            .unwrap()
            .unwrap();
        assert_eq!(thumbnail.color_order, ColorOrder::Bgr);
        assert_eq!(
            thumbnail.data,
            ThumbnailData::Raw {
                row_bytes: 4,
                pixels: vec![7, 8, 9, 0],
            }
        );
    }
}
