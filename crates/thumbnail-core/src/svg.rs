//! 外部資源を読み込まず、SVGを指定された大きさに描画する。

use crate::{Error, Image};
use resvg::usvg::{ImageKind, Options, Tree, roxmltree};
use std::{
    io::{Cursor, Read},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 4096 * 4096;
const MAX_NODES: u64 = 20_000;
const MAX_VECTOR_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DEPTH: u8 = 4;

struct Budget {
    nodes: AtomicU64,
    vector_bytes: AtomicU64,
    pixels: AtomicU64,
    failed: AtomicBool,
}

fn consume(value: &AtomicU64, amount: u64) -> Result<(), Error> {
    value
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
            left.checked_sub(amount)
        })
        .map(|_| ())
        .map_err(|_| Error::Limit)
}

/// 文書を8 MiBまで読み、長辺が要求値以下の画像へ描画する。
pub fn render(input: &mut impl Read, max_side: u32) -> Result<Image, Error> {
    if max_side == 0 || max_side > 1024 {
        return Err(Error::Limit);
    }
    let mut data = Vec::new();
    input.take((MAX_INPUT + 1) as u64).read_to_end(&mut data)?;
    let budget = Arc::new(Budget {
        nodes: AtomicU64::new(MAX_NODES),
        vector_bytes: AtomicU64::new(MAX_VECTOR_BYTES),
        pixels: AtomicU64::new(MAX_PIXELS),
        failed: AtomicBool::new(false),
    });
    let tree = parse(&data, budget, 0)?;
    let size = tree.size();
    let scale = (max_side as f32 / size.width().max(size.height())).min(1.0);
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or(Error::Limit)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(Image {
        width,
        height,
        rgba: pixmap.take(),
    })
}

fn parse(data: &[u8], budget: Arc<Budget>, depth: u8) -> Result<Tree, Error> {
    if data.len() > MAX_INPUT || depth >= MAX_DEPTH || data.starts_with(&[0x1f, 0x8b]) {
        return Err(Error::Limit);
    }
    let text = std::str::from_utf8(data).map_err(|_| Error::Invalid)?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: budget.nodes.load(Ordering::Relaxed) as u32,
            entity_resolver: None,
        },
    )
    .map_err(|_| Error::Invalid)?;
    consume(&budget.nodes, doc.descendants().count() as u64)?;
    for node in doc.descendants() {
        if node.is_text() {
            consume(&budget.vector_bytes, node.text().unwrap_or("").len() as u64)?;
        }
        for attr in node.attributes() {
            if attr.name() == "href"
                && attr.value().starts_with("data:")
                && (node.has_tag_name("image") || node.has_tag_name("feImage"))
            {
                continue;
            }
            consume(&budget.vector_bytes, attr.value().len() as u64)?;
        }
    }
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let mut options = Options::default();
    if doc.descendants().any(|node| node.has_tag_name("text")) {
        options.fontdb = FONTS
            .get_or_init(|| {
                let mut fonts = resvg::usvg::fontdb::Database::new();
                fonts.load_system_fonts();
                Arc::new(fonts)
            })
            .clone();
    }
    let shared = budget.clone();
    options.image_href_resolver.resolve_string = Box::new(move |_, _| {
        shared.failed.store(true, Ordering::Relaxed);
        None
    });
    let shared = budget.clone();
    options.image_href_resolver.resolve_data = Box::new(move |mime, data, _| {
        let result = (|| -> Result<ImageKind, Error> {
            match image::guess_format(&data) {
                Ok(format @ (image::ImageFormat::Png | image::ImageFormat::Jpeg)) => {
                    let mut reader = image::ImageReader::new(Cursor::new(data.as_slice()))
                        .with_guessed_format()
                        .map_err(|_| Error::Invalid)?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(4096);
                    limits.max_image_height = Some(4096);
                    limits.max_alloc = Some(64 * 1024 * 1024);
                    reader.limits(limits);
                    let decoded = reader.decode().map_err(|_| Error::Invalid)?;
                    consume(
                        &shared.pixels,
                        u64::from(decoded.width()) * u64::from(decoded.height()),
                    )?;
                    Ok(if format == image::ImageFormat::Png {
                        ImageKind::PNG(data)
                    } else {
                        ImageKind::JPEG(data)
                    })
                }
                _ if matches!(mime, "image/svg+xml" | "text/plain") => {
                    Ok(ImageKind::SVG(parse(&data, shared.clone(), depth + 1)?))
                }
                _ => Err(Error::Unsupported),
            }
        })();
        match result {
            Ok(image) => Some(image),
            Err(_) => {
                shared.failed.store(true, Ordering::Relaxed);
                None
            }
        }
    });
    let tree = Tree::from_xmltree(&doc, &options).map_err(|_| Error::Invalid)?;
    if depth > 0 {
        let size = tree.size().to_int_size();
        if size.width() > 4096 || size.height() > 4096 {
            return Err(Error::Limit);
        }
        consume(
            &budget.pixels,
            u64::from(size.width()) * u64::from(size.height()),
        )?;
    }
    if budget.failed.load(Ordering::Relaxed) {
        return Err(Error::Invalid);
    }
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_transparent_svg_at_requested_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;
        let image = render(&mut svg.as_slice(), 10).unwrap();
        assert_eq!((image.width, image.height), (10, 5));
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[9 * 4..10 * 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn rejects_external_images_and_entities() {
        let external = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image href="file:///does-not-exist.png" width="1" height="1"/></svg>"#;
        assert!(render(&mut external.as_slice(), 10).is_err());
        let entity = br#"<!DOCTYPE svg [<!ENTITY x "hello">]><svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><text>&x;</text></svg>"#;
        assert!(render(&mut entity.as_slice(), 10).is_err());
    }

    #[test]
    fn rejects_input_over_limit() {
        assert!(render(&mut vec![b' '; MAX_INPUT + 1].as_slice(), 10).is_err());
    }

    #[test]
    fn renders_embedded_png() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image width="1" height="1" href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=="/></svg>"#;
        let image = render(&mut svg.as_slice(), 1).unwrap();
        assert_eq!(image.rgba, [255, 0, 0, 255]);
    }
}
