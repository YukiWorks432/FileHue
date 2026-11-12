//! 文書内のXMPサムネイルを、先頭と末尾の有限な範囲から取り出す。

use crate::Error;
use base64::{Engine, engine::general_purpose::STANDARD};
use resvg::usvg::roxmltree::{self, Node};
use std::io::{Read, Seek, SeekFrom};

const WINDOW: u64 = 1024 * 1024;
const META_NS: &str = "adobe:ns:meta/";
const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";
const IMAGE_NS: &str = "http://ns.adobe.com/xap/1.0/g/img/";
const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// AIまたはEPSのXMPに格納されたJPEGを返す。
pub fn extract<R: Read + Seek>(reader: &mut R) -> Result<Option<Vec<u8>>, Error> {
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut signature = [0; 5];
    if length < signature.len() as u64 {
        return Err(Error::Unsupported);
    }
    reader.read_exact(&mut signature)?;
    if !signature.starts_with(b"%PDF-")
        && !signature.starts_with(b"%!PS")
        && !signature.starts_with(&[0xc5, 0xd0, 0xd3, 0xc6])
    {
        return Err(Error::Unsupported);
    }
    for offset in [0, length.saturating_sub(WINDOW)] {
        reader.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; (length - offset).min(WINDOW) as usize];
        reader.read_exact(&mut bytes)?;
        if let Some(image) = parse_window(&bytes) {
            return Ok(Some(image));
        }
        if length <= WINDOW {
            break;
        }
    }
    Ok(None)
}

fn parse_window(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut from = 0;
    let mut candidates = 0;
    while let Some(relative) = bytes[from..].windows(7).position(|part| part == b"xmpmeta") {
        candidates += 1;
        if candidates > 64 {
            return None;
        }
        let marker = from + relative;
        from = marker + 7;
        let search_start = marker.saturating_sub(128);
        let Some(relative_start) = bytes[search_start..marker]
            .iter()
            .rposition(|&byte| byte == b'<')
        else {
            continue;
        };
        let start = search_start + relative_start;
        let name = &bytes[start + 1..marker + 7];
        if name.len() > 128 || name.first() == Some(&b'/') || name.contains(&b' ') {
            continue;
        }
        let mut closing = b"</".to_vec();
        closing.extend_from_slice(name);
        closing.push(b'>');
        let Some(end) = bytes[from..]
            .windows(closing.len())
            .position(|part| part == closing)
        else {
            continue;
        };
        let Ok(xml) = std::str::from_utf8(&bytes[start..from + end + closing.len()]) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse_with_options(
            xml,
            roxmltree::ParsingOptions {
                allow_dtd: false,
                nodes_limit: 20_000,
                entity_resolver: None,
            },
        ) else {
            continue;
        };
        if !doc.root_element().has_tag_name((META_NS, "xmpmeta")) {
            continue;
        }
        for thumbnail in doc.descendants().filter(|node| {
            node.has_tag_name((RDF_NS, "li"))
                && node
                    .ancestors()
                    .any(|parent| parent.has_tag_name((XMP_NS, "Thumbnails")))
        }) {
            if let Some(image) = image_from_thumbnail(thumbnail) {
                return Some(image);
            }
        }
    }
    None
}

fn image_from_thumbnail(thumbnail: Node<'_, '_>) -> Option<Vec<u8>> {
    let mut format = None;
    let mut encoded = None;
    for node in thumbnail.descendants().filter(|node| node.is_element()) {
        if node.has_tag_name((IMAGE_NS, "format")) {
            format = node.text();
        } else if node.has_tag_name((IMAGE_NS, "image")) {
            encoded = node.text();
        }
        for attr in node.attributes() {
            if attr.namespace() == Some(IMAGE_NS) && attr.name() == "format" {
                format = Some(attr.value());
            } else if attr.namespace() == Some(IMAGE_NS) && attr.name() == "image" {
                encoded = Some(attr.value());
            }
        }
    }
    if !format?.trim().eq_ignore_ascii_case("JPEG") {
        return None;
    }
    let data: Vec<u8> = encoded?
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    let image = STANDARD.decode(data).ok()?;
    image.starts_with(b"\xff\xd8\xff").then_some(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn packet(attribute: bool) -> (String, Vec<u8>) {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[255, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        let data = STANDARD.encode(&jpeg);
        let content = if attribute {
            format!(r#"<rdf:li i:format="JPEG" i:image="{data}"/>"#)
        } else {
            format!(r#"<rdf:li><i:format>JPEG</i:format><i:image>{data}</i:image></rdf:li>"#)
        };
        (
            format!(
                r#"<m:xmpmeta xmlns:m="adobe:ns:meta/" xmlns:rdf="{RDF_NS}" xmlns:x="{XMP_NS}" xmlns:i="{IMAGE_NS}"><rdf:RDF><rdf:Description><x:Thumbnails><rdf:Alt>{content}</rdf:Alt></x:Thumbnails></rdf:Description></rdf:RDF></m:xmpmeta>"#
            ),
            jpeg,
        )
    }

    #[test]
    fn extracts_element_and_attribute_forms_from_document_tail() {
        for attribute in [false, true] {
            let (xml, jpeg) = packet(attribute);
            let mut document = b"%!PS-Adobe-3.0 EPSF-3.0\n".to_vec();
            document.resize((WINDOW * 2) as usize, b' ');
            document.extend_from_slice(xml.as_bytes());
            assert_eq!(extract(&mut Cursor::new(document)).unwrap(), Some(jpeg));
        }
    }

    #[test]
    fn rejects_invalid_namespace_and_non_jpeg() {
        let (xml, _) = packet(false);
        let wrong_namespace = xml.replace(IMAGE_NS, "urn:other");
        let document = format!("%PDF-1.7\n{wrong_namespace}");
        assert!(extract(&mut Cursor::new(document)).unwrap().is_none());
        let wrong_format = xml.replace(">JPEG<", ">PNG<");
        let document = format!("%PDF-1.7\n{wrong_format}");
        assert!(extract(&mut Cursor::new(document)).unwrap().is_none());
    }
}
