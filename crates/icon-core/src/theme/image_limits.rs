//! 素材のヘッダーと展開後の大きさを、デコーダーへ渡す前に制限する。
use anyhow::{Context, Result, bail, ensure};
use resvg::usvg::{ImageKind, Options, Tree, roxmltree};
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: u64 = 4096 * 4096;
const MAX_DECODE: u64 = 64 * 1024 * 1024;
const MAX_SVG: usize = 8 * 1024 * 1024;
const MAX_NODES: u64 = 20_000;
const MAX_VECTOR_BYTES: u64 = 512 * 1024;
const MAX_DEPTH: u8 = 4;

fn dimensions(width: u32, height: u32) -> Result<u64> {
    let pixels = u64::from(width) * u64::from(height);
    ensure!(
        width > 0 && height > 0 && width <= MAX_SIDE && height <= MAX_SIDE && pixels <= MAX_PIXELS,
        "image dimensions exceed conversion limits"
    );
    Ok(pixels)
}
fn reader(data: &[u8]) -> Result<image::ImageReader<Cursor<&[u8]>>> {
    let mut reader = image::ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    ensure!(
        matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
        ),
        "unsupported raster format; use PNG or JPEG"
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE);
    reader.limits(limits);
    Ok(reader)
}
fn raster_size(data: &[u8]) -> Result<(u32, u32)> {
    let size = reader(data)?
        .into_dimensions()
        .context("image header is invalid or exceeds dimension limits")?;
    dimensions(size.0, size.1)?;
    Ok(size)
}
pub(super) fn raster(data: &[u8]) -> Result<image::DynamicImage> {
    raster_size(data)?;
    reader(data)?
        .decode()
        .context("invalid or oversized raster image")
}

struct Budget {
    nodes: AtomicU64,
    vector_bytes: AtomicU64,
    pixels: AtomicU64,
    failed: AtomicBool,
}
fn consume(budget: &AtomicU64, amount: u64) -> Result<()> {
    budget
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
            left.checked_sub(amount)
        })
        .map_err(|_| anyhow::anyhow!("SVG complexity or embedded image limit exceeded"))?;
    Ok(())
}
pub(super) fn svg(data: &[u8]) -> Result<Tree> {
    parse_svg(
        data,
        Arc::new(Budget {
            nodes: AtomicU64::new(MAX_NODES),
            vector_bytes: AtomicU64::new(MAX_VECTOR_BYTES),
            pixels: AtomicU64::new(MAX_PIXELS),
            failed: AtomicBool::new(false),
        }),
        0,
    )
}
fn parse_svg(data: &[u8], budget: Arc<Budget>, depth: u8) -> Result<Tree> {
    ensure!(
        data.len() <= MAX_SVG && depth < MAX_DEPTH,
        "SVG size or nesting limit exceeded"
    );
    ensure!(
        !data.starts_with(&[0x1f, 0x8b]),
        "compressed SVG is unsupported"
    );
    let text = std::str::from_utf8(data).context("SVG must be UTF-8")?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: budget.nodes.load(Ordering::Relaxed) as u32,
            entity_resolver: None,
        },
    )
    .context("SVG XML is invalid or exceeds parsing limits")?;
    consume(&budget.nodes, doc.descendants().count() as u64)?;
    for node in doc.descendants() {
        if node.is_text() {
            consume(&budget.vector_bytes, node.text().unwrap_or("").len() as u64)?;
        }
        for attr in node.attributes() {
            // 埋め込みデータ自体は入力長と画素数で制限し、ベクターの属性とは別に数える。
            if attr.name() == "href"
                && attr.value().starts_with("data:")
                && (node.has_tag_name("image") || node.has_tag_name("feImage"))
            {
                continue;
            }
            consume(&budget.vector_bytes, attr.value().len() as u64)?;
        }
    }
    let mut options = Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let shared = budget.clone();
    options.image_href_resolver.resolve_data = Box::new(move |mime, data, _| {
        let result = (|| -> Result<ImageKind> {
            ensure!(
                !data.starts_with(&[0x1f, 0x8b]),
                "compressed embedded SVG is unsupported"
            );
            match image::guess_format(&data) {
                Ok(format @ (image::ImageFormat::Png | image::ImageFormat::Jpeg)) => {
                    let (width, height) = raster_size(&data)?;
                    consume(&shared.pixels, dimensions(width, height)?)?;
                    // 不正な画像を無言で描画から落とさないよう、上限付きで一度検証する。
                    raster(&data)?;
                    Ok(if format == image::ImageFormat::Png {
                        ImageKind::PNG(data)
                    } else {
                        ImageKind::JPEG(data)
                    })
                }
                _ if matches!(mime, "image/svg+xml" | "text/plain") => {
                    Ok(ImageKind::SVG(parse_svg(&data, shared.clone(), depth + 1)?))
                }
                _ => bail!("unsupported embedded image"),
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
    let tree = Tree::from_xmltree(&doc, &options)?;
    let size = tree.size().to_int_size();
    let pixels = dimensions(size.width(), size.height())?;
    if depth > 0 {
        consume(&budget.pixels, pixels)?;
    }
    ensure!(
        !budget.failed.load(Ordering::Relaxed),
        "embedded image is invalid or exceeds SVG limits"
    );
    Ok(tree)
}

pub(super) fn validate_icon(data: &[u8]) -> Result<()> {
    ensure!(
        data.len() >= 6 && data[..4] == [0, 0, 1, 0],
        "invalid ICO header"
    );
    let count = u16::from_le_bytes(data[4..6].try_into()?) as usize;
    ensure!(
        (1..=32).contains(&count) && data.len() >= 6 + count * 16,
        "ICO frame count limit exceeded"
    );
    let mut remaining = 32 * 1024 * 1024usize;
    let mut pixels_left = MAX_PIXELS;
    for entry in data[6..6 + count * 16].as_chunks::<16>().0 {
        let length = u32::from_le_bytes(entry[8..12].try_into()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into()?) as usize;
        ensure!(
            length > 0 && length <= remaining && offset >= 6 + count * 16,
            "ICO frame size limit exceeded"
        );
        let frame = data
            .get(offset..offset.checked_add(length).context("ICO span overflow")?)
            .context("ICO frame data outside file")?;
        remaining -= length;
        let (width, height) = if frame.starts_with(b"\x89PNG\r\n\x1a\n") {
            raster_size(frame)?
        } else {
            ensure!(frame.len() >= 40, "invalid ICO bitmap header");
            let width = i32::from_le_bytes(frame[4..8].try_into()?);
            let height = i32::from_le_bytes(frame[8..12].try_into()?);
            ensure!(
                width > 0 && height > 0 && height % 2 == 0,
                "invalid ICO bitmap dimensions"
            );
            (width as u32, height as u32 / 2)
        };
        let pixels = dimensions(width, height)?;
        ensure!(pixels <= pixels_left, "ICO total pixel limit exceeded");
        pixels_left -= pixels;
    }
    Ok(())
}
pub(super) fn icon(data: &[u8]) -> Result<Vec<image::DynamicImage>> {
    validate_icon(data)?;
    // 範囲と寸法の検証が済むまで、ライブラリによるフレーム用領域の確保を始めない。
    let dir = ico::IconDir::read(Cursor::new(data))?;
    let mut selected = std::collections::BTreeSet::new();
    for size in super::SIZES {
        let area = |(_, entry): &(usize, &ico::IconDirEntry)| {
            u64::from(entry.width()) * u64::from(entry.height())
        };
        let index = dir
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.width() >= size && entry.height() >= size)
            .min_by_key(area)
            .or_else(|| dir.entries().iter().enumerate().max_by_key(area))
            .context("no ICO image")?
            .0;
        selected.insert(index);
    }
    let mut images = Vec::with_capacity(selected.len());
    for (index, entry) in dir.entries().iter().enumerate() {
        dimensions(entry.width(), entry.height())?;
        // 未使用フレームも復号して検査する。壊れた画像を選別で見逃さない。
        let decoded = entry.decode()?;
        if selected.contains(&index) {
            let rgba = image::RgbaImage::from_raw(
                decoded.width(),
                decoded.height(),
                decoded.into_rgba_data(),
            )
            .context("invalid ICO pixels")?;
            images.push(image::DynamicImage::ImageRgba8(rgba));
        }
    }
    Ok(images)
}
