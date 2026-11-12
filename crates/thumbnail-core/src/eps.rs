//! EPSの埋め込みTIFFまたはEPSIプレビューを抽出する。

use crate::Error;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

const MAX_PREVIEW_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 1024 * 1024;

/// EPSIプレビューを要求サイズ以下のアルファ付きグレースケールへ変換する。
pub fn extract_epsi<R: Read + Seek>(
    reader: &mut R,
    max_side: u32,
) -> Result<Option<crate::Image>, Error> {
    if max_side == 0 || max_side > 1024 {
        return Err(Error::Limit);
    }
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    if length < 4 {
        return Err(Error::Unsupported);
    }
    let mut signature = [0; 4];
    reader.read_exact(&mut signature)?;
    let (start, section_length) = if signature == [0xc5, 0xd0, 0xd3, 0xc6] {
        if length < 30 {
            return Err(Error::Invalid);
        }
        let mut header = [0; 26];
        reader.read_exact(&mut header)?;
        let start = u64::from(u32::from_le_bytes(header[0..4].try_into().unwrap()));
        let section_length = u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap()));
        if start < 30
            || section_length == 0
            || start
                .checked_add(section_length)
                .is_none_or(|end| end > length)
        {
            return Err(Error::Invalid);
        }
        (start, section_length)
    } else {
        (0, length)
    };
    reader.seek(SeekFrom::Start(start))?;
    let mut input = BufReader::new(reader.take(section_length.min(MAX_TEXT_BYTES + 1)));
    let mut line = Vec::new();
    if input.read_until(b'\n', &mut line)? == 0
        || !line.starts_with(b"%!PS-Adobe-")
        || !line.windows(5).any(|part| part == b"EPSF-")
    {
        return Err(Error::Unsupported);
    }
    let mut bounds: Option<(f64, f64)> = None;
    let mut high_resolution_bounds = false;
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            return if section_length > MAX_TEXT_BYTES && input.get_ref().limit() == 0 {
                Err(Error::Limit)
            } else {
                Ok(None)
            };
        }
        if line.starts_with(b"%%BeginProlog") || line.starts_with(b"%%EOF") {
            return Ok(None);
        }
        let bounding_box = line
            .strip_prefix(b"%%HiResBoundingBox:")
            .map(|value| (value, true))
            .or_else(|| {
                line.strip_prefix(b"%%BoundingBox:")
                    .map(|value| (value, false))
            });
        if let Some((value, high_resolution)) = bounding_box
            && (!high_resolution_bounds || high_resolution)
            && let Ok(value) = std::str::from_utf8(value)
        {
            let fields = value
                .split_ascii_whitespace()
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>();
            if let Ok(fields) = fields
                && let [left, bottom, right, top] = fields.as_slice()
            {
                let (width, height) = (right - left, top - bottom);
                if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
                    bounds = Some((width, height));
                    high_resolution_bounds = high_resolution;
                }
            }
        }
        if let Some(header) = line.strip_prefix(b"%%BeginPreview:") {
            let header = std::str::from_utf8(header).map_err(|_| Error::Invalid)?;
            let fields: Vec<u32> = header
                .split_ascii_whitespace()
                .map(|value| value.parse().map_err(|_| Error::Invalid))
                .collect::<Result<_, _>>()?;
            let [width, height, depth, lines] = fields.as_slice() else {
                return Err(Error::Invalid);
            };
            if *width == 0
                || *height == 0
                || *width > 4096
                || *height > 4096
                || !matches!(depth, 1 | 2 | 4 | 8)
                || *lines == 0
                || *lines > 65536
            {
                return Err(Error::Limit);
            }
            let row_bytes = (u64::from(*width) * u64::from(*depth)).div_ceil(8);
            let expected = (row_bytes * u64::from(*height)) as usize;
            if expected > MAX_TEXT_BYTES as usize / 2 {
                return Err(Error::Limit);
            }
            let mut packed = Vec::with_capacity(expected);
            let mut high = None;
            for _ in 0..*lines {
                line.clear();
                if input.read_until(b'\n', &mut line)? == 0 {
                    return Err(Error::Invalid);
                }
                let content = line.strip_suffix(b"\n").unwrap_or(&line);
                let content = content.strip_suffix(b"\r").unwrap_or(content);
                if content.len() > 255 || !content.starts_with(b"%") || content.starts_with(b"%%") {
                    return Err(Error::Invalid);
                }
                for &byte in content {
                    let digit = match byte {
                        b'0'..=b'9' => Some(byte - b'0'),
                        b'a'..=b'f' => Some(byte - b'a' + 10),
                        b'A'..=b'F' => Some(byte - b'A' + 10),
                        _ => None,
                    };
                    if let Some(digit) = digit {
                        if let Some(first) = high.take() {
                            if packed.len() == expected {
                                return Err(Error::Invalid);
                            }
                            packed.push(first * 16 + digit);
                        } else {
                            high = Some(digit);
                        }
                    }
                }
            }
            line.clear();
            if high.is_some()
                || packed.len() != expected
                || input.read_until(b'\n', &mut line)? == 0
                || line.trim_ascii() != b"%%EndPreview"
            {
                return Err(Error::Invalid);
            }
            let longest = max_side.min((*width).max(*height));
            let (display_width, display_height) =
                bounds.unwrap_or((f64::from(*width), f64::from(*height)));
            let scale = f64::from(longest) / display_width.max(display_height);
            let out_width = (display_width * scale).round().max(1.0) as u32;
            let out_height = (display_height * scale).round().max(1.0) as u32;
            let mut rgba = Vec::with_capacity((out_width * out_height * 4) as usize);
            let mask = (1u16 << depth) - 1;
            for y in 0..out_height {
                // EPSIの最初の行は画像座標の下端にある。
                let source_y = *height - 1 - y * *height / out_height;
                for x in 0..out_width {
                    let source_x = x * *width / out_width;
                    let bit = source_x * *depth;
                    let byte =
                        packed[(u64::from(source_y) * row_bytes + u64::from(bit / 8)) as usize];
                    let sample = (u16::from(byte) >> (8 - *depth - bit % 8)) & mask;
                    let gray = 255 - (sample * 255 / mask) as u8;
                    rgba.extend_from_slice(&[gray, gray, gray, 255]);
                }
            }
            return Ok(Some(crate::Image {
                width: out_width,
                height: out_height,
                rgba,
            }));
        }
    }
}

/// TIFFが存在しない場合は `Ok(None)`。PostScript本文は実行しない。
pub fn extract_tiff<R: Read + Seek>(reader: &mut R) -> Result<Option<Vec<u8>>, Error> {
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    if length < 30 {
        return Err(Error::Invalid);
    }
    let mut header = [0; 30];
    reader.read_exact(&mut header)?;
    if header[..4] != [0xc5, 0xd0, 0xd3, 0xc6] {
        return Err(Error::Unsupported);
    }
    let offset = u64::from(u32::from_le_bytes(header[20..24].try_into().unwrap()));
    let size = u64::from(u32::from_le_bytes(header[24..28].try_into().unwrap()));
    if offset == 0 && size == 0 {
        return Ok(None);
    }
    if offset < 30 || size < 8 || offset.checked_add(size).is_none_or(|end| end > length) {
        return Err(Error::Invalid);
    }
    if size > MAX_PREVIEW_BYTES {
        return Err(Error::Limit);
    }
    reader.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0; size as usize];
    reader.read_exact(&mut data)?;
    if !data.starts_with(b"II\x2a\0") && !data.starts_with(b"MM\0\x2a") {
        return Err(Error::Invalid);
    }
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn extracts_tiff_only_from_the_documented_range() {
        let mut document = vec![0; 30];
        document[..4].copy_from_slice(&[0xc5, 0xd0, 0xd3, 0xc6]);
        document[20..24].copy_from_slice(&34u32.to_le_bytes());
        document[24..28].copy_from_slice(&8u32.to_le_bytes());
        document.extend_from_slice(b"junkII\x2a\0\x08\0\0\0trailing");
        assert_eq!(
            extract_tiff(&mut Cursor::new(document)).unwrap(),
            Some(b"II\x2a\0\x08\0\0\0".to_vec())
        );
    }

    #[test]
    fn absent_preview_and_out_of_bounds_preview_are_distinct() {
        let mut document = vec![0; 30];
        document[..4].copy_from_slice(&[0xc5, 0xd0, 0xd3, 0xc6]);
        assert_eq!(extract_tiff(&mut Cursor::new(&document)).unwrap(), None);
        document[20..24].copy_from_slice(&100u32.to_le_bytes());
        document[24..28].copy_from_slice(&8u32.to_le_bytes());
        assert!(matches!(
            extract_tiff(&mut Cursor::new(document)),
            Err(Error::Invalid)
        ));
    }

    #[test]
    fn epsi_decodes_row_padding_and_bottom_up_coordinates() {
        let document = b"%!PS-Adobe-3.0 EPSF-3.0\n%%EndComments\n%%BeginPreview: 2 2 1 2\n%80\n%40\n%%EndPreview\n";
        let image = extract_epsi(&mut Cursor::new(document), 2)
            .unwrap()
            .unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(
            image.rgba,
            [
                255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255
            ]
        );
    }

    #[test]
    fn epsi_handles_grayscale_and_rejects_truncation() {
        let document = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BeginPreview: 4 1 2 1\n%1B\n%%EndPreview\n";
        let image = extract_epsi(&mut Cursor::new(document), 4)
            .unwrap()
            .unwrap();
        assert_eq!(
            image.rgba,
            [
                255, 255, 255, 255, 170, 170, 170, 255, 85, 85, 85, 255, 0, 0, 0, 255
            ]
        );
        let truncated = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BeginPreview: 4 1 2 1\n%1\n%%EndPreview\n";
        assert!(matches!(
            extract_epsi(&mut Cursor::new(truncated), 4),
            Err(Error::Invalid)
        ));
    }

    #[test]
    fn epsi_distinguishes_missing_preview_from_search_limit() {
        let document = b"%!PS-Adobe-3.0 EPSF-3.0\n%%EndComments\n%%BeginProlog\n";
        assert!(
            extract_epsi(&mut Cursor::new(document), 32)
                .unwrap()
                .is_none()
        );
        let mut document = b"%!PS-Adobe-3.0 EPSF-3.0\n".to_vec();
        document.extend(std::iter::repeat_n(b'%', MAX_TEXT_BYTES as usize + 1));
        assert!(matches!(
            extract_epsi(&mut Cursor::new(document), 32),
            Err(Error::Limit)
        ));
    }

    #[test]
    fn epsi_uses_artwork_bounding_box_for_display_ratio() {
        let document = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 4 2\n%%BeginPreview: 2 2 1 2\n%80\n%40\n%%EndPreview\n";
        let image = extract_epsi(&mut Cursor::new(document), 8)
            .unwrap()
            .unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, [255, 255, 255, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn dos_epsi_is_limited_to_the_declared_postscript_section() {
        let postscript = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BeginPreview: 1 1 1 1\n%80\n%%EndPreview\n";
        let mut document = vec![0; 30];
        document[..4].copy_from_slice(&[0xc5, 0xd0, 0xd3, 0xc6]);
        document[4..8].copy_from_slice(&30u32.to_le_bytes());
        document[8..12].copy_from_slice(&(postscript.len() as u32).to_le_bytes());
        document.extend_from_slice(postscript);
        let image = extract_epsi(&mut Cursor::new(&document), 16)
            .unwrap()
            .unwrap();
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.rgba, [0, 0, 0, 255]);
        document[8..12].copy_from_slice(&((postscript.len() - 10) as u32).to_le_bytes());
        assert!(matches!(
            extract_epsi(&mut Cursor::new(document), 16),
            Err(Error::Invalid)
        ));
    }
}
