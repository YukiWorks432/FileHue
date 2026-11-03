//! 公開の取り込み経路で、不正な素材を公開せず、正常な素材の画素と表示サイズを保つ。
use base64::{Engine, engine::general_purpose::STANDARD};
use file_icons_core::import;
use image::ImageEncoder;
use std::fs;

fn fixture(name: &str, bytes: &[u8]) -> (tempfile::TempDir, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("package.json"), r#"{"name":"limits","publisher":"test","version":"1.0.0","contributes":{"iconThemes":[{"id":"test","label":"Image limits","path":"theme.json"}]}}"#).unwrap();
    fs::write(root.path().join("theme.json"), serde_json::to_vec(&serde_json::json!({"iconDefinitions":{"icon":{"iconPath":name}},"fileExtensions":{"wfi-limit":"icon"}})).unwrap()).unwrap();
    fs::write(root.path().join(name), bytes).unwrap();
    (root, tempfile::tempdir().unwrap())
}
fn rejects(name: &str, bytes: &[u8], reason: &str) {
    let (root, cache) = fixture(name, bytes);
    let error = import(root.path(), "test", cache.path()).unwrap_err();
    assert!(format!("{error:#}").contains(reason), "{error:#}");
    assert_eq!(fs::read(root.path().join(name)).unwrap(), bytes);
    for folder in fs::read_dir(cache.path()).unwrap() {
        assert!(!folder.unwrap().path().join("provenance.json").exists());
    }
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let pixels = [18, 228, 48, 255].repeat((width * height) as usize);
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}
fn svg(body: &str) -> Vec<u8> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">{body}</svg>"#)
        .into_bytes()
}
fn embedded(mime: &str, data: &[u8]) -> String {
    format!(
        r#"<image width="16" height="16" href="data:{mime};base64,{}"/>"#,
        STANDARD.encode(data)
    )
}
fn valid(name: &str, bytes: &[u8]) {
    let (root, cache) = fixture(name, bytes);
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let icon = ico::IconDir::read(
        fs::File::open(pack.theme.resolve("sample.wfi-limit").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        icon.entries()
            .iter()
            .map(|e| (e.width(), e.height()))
            .collect::<Vec<_>>(),
        [
            (16, 16),
            (20, 20),
            (24, 24),
            (32, 32),
            (48, 48),
            (128, 128),
            (256, 256)
        ]
    );
    for entry in icon.entries() {
        let image = entry.decode().unwrap();
        let offset = ((image.height() / 2 * image.width() + image.width() / 2) * 4) as usize;
        assert_eq!(&image.rgba_data()[offset..offset + 4], &[18, 228, 48, 255]);
    }
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(&pack.provenance).unwrap()).unwrap();
    assert_eq!(record["conversion_revision"], 3);
    // 再取得も同じ検証済みの出力を使い、キャッシュで不正な寸法を素通りさせない。
    assert_eq!(
        import(root.path(), "test", cache.path())
            .unwrap()
            .provenance,
        pack.provenance
    );
}
#[test]
fn supported_images_keep_pixels_and_all_windows_icon_sizes() {
    valid("icon.png", &png(16, 16));
    valid("icon.svg", &svg(&embedded("image/png", &png(16, 16))));
    valid(
        "icon.svg",
        &svg(&embedded(
            "image/svg+xml",
            &svg(r##"<rect width="16" height="16" fill="#12e430"/>"##),
        )),
    );
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    dir.add_entry(
        ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
            16,
            16,
            [18, 228, 48, 255].repeat(16 * 16),
        ))
        .unwrap(),
    );
    let mut data = Vec::new();
    dir.write(&mut data).unwrap();
    valid("icon.ico", &data);
}
#[test]
fn raster_headers_are_limited_before_large_pixel_allocation() {
    let (root, cache) = fixture("boundary.png", &png(4096, 1));
    assert!(import(root.path(), "test", cache.path()).is_ok());
    rejects("icon.png", &png(5000, 1), "dimension");
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpg)
        .encode(
            &[18, 228, 48].repeat(5000),
            5000,
            1,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    rejects("icon.jpg", &jpg, "dimension");
    rejects(
        "icon.svg",
        &svg(&embedded("image/png", &png(5000, 1))),
        "embedded image",
    );
}
#[test]
fn svg_compression_xml_and_complexity_cannot_bypass_limits() {
    let compressed = include_bytes!("fixtures/compressed.svg.gz");
    rejects("icon.svg", compressed, "compressed SVG");
    rejects(
        "icon.svg",
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="5000" height="1"/>"#,
        "dimensions exceed",
    );
    rejects(
        "icon.svg",
        &svg(&embedded("image/svg+xml", compressed)),
        "embedded image",
    );
    rejects(
        "icon.svg",
        br#"<!DOCTYPE svg [<!ENTITY x "abc">]><svg xmlns="http://www.w3.org/2000/svg">&x;</svg>"#,
        "parsing limits",
    );
    rejects("icon.svg", &svg(&"<g/>".repeat(20_100)), "parsing limits");
    rejects(
        "icon.svg",
        &svg(&format!(r#"<path d="{}"/>"#, "M0 0 L1 1 ".repeat(60_000))),
        "complexity",
    );
    let mut nested = svg(r##"<rect width="16" height="16" fill="#12e430"/>"##);
    for _ in 0..5 {
        nested = svg(&embedded("image/svg+xml", &nested));
    }
    rejects("icon.svg", &nested, "embedded image");
}
#[test]
fn embedded_images_share_one_pixel_budget() {
    let image = png(3000, 3000);
    rejects(
        "icon.svg",
        &svg(&embedded("image/png", &image).repeat(2)),
        "embedded image",
    );
}
#[test]
fn ico_frame_spans_and_dimensions_are_checked_before_reading() {
    rejects("icon.ico", &[0, 0, 1, 0, 33, 0], "frame count limit");
    let mut data = vec![0u8; 22];
    data[2] = 1;
    data[4] = 1;
    data[6] = 16;
    data[7] = 16;
    data[14..18].copy_from_slice(&(64u32 * 1024 * 1024 + 1).to_le_bytes());
    data[18..22].copy_from_slice(&22u32.to_le_bytes());
    rejects("icon.ico", &data, "frame size limit");
    data[14..18].copy_from_slice(&1024u32.to_le_bytes());
    rejects("icon.ico", &data, "outside file");
    data[14..18].copy_from_slice(&40u32.to_le_bytes());
    data.resize(62, 0);
    data[22..26].copy_from_slice(&40u32.to_le_bytes());
    data[26..30].copy_from_slice(&5000i32.to_le_bytes());
    data[30..34].copy_from_slice(&2i32.to_le_bytes());
    rejects("icon.ico", &data, "dimensions exceed");
    let mut aggregate = vec![0u8; 118];
    aggregate[2] = 1;
    aggregate[4] = 2;
    for index in 0..2 {
        let row = 6 + index * 16;
        let offset = 38 + index * 40;
        aggregate[row + 8..row + 12].copy_from_slice(&40u32.to_le_bytes());
        aggregate[row + 12..row + 16].copy_from_slice(&(offset as u32).to_le_bytes());
        aggregate[offset..offset + 4].copy_from_slice(&40u32.to_le_bytes());
        aggregate[offset + 4..offset + 8].copy_from_slice(&4096i32.to_le_bytes());
        aggregate[offset + 8..offset + 12].copy_from_slice(&8192i32.to_le_bytes());
    }
    rejects("icon.ico", &aggregate, "total pixel limit");
}

#[test]
fn ico_keeps_distinct_artwork_for_small_and_large_sizes() {
    let mut source = ico::IconDir::new(ico::ResourceType::Icon);
    for (size, color) in [(16, [18, 228, 48, 255]), (256, [4, 33, 200, 255])] {
        source.add_entry(
            ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
                size,
                size,
                color.repeat((size * size) as usize),
            ))
            .unwrap(),
        );
    }
    let mut bytes = Vec::new();
    source.write(&mut bytes).unwrap();
    let (root, cache) = fixture("icon.ico", &bytes);
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let result = ico::IconDir::read(
        fs::File::open(pack.theme.resolve("sample.wfi-limit").unwrap()).unwrap(),
    )
    .unwrap();
    for (size, color) in [(16, [18, 228, 48, 255]), (256, [4, 33, 200, 255])] {
        let frame = result
            .entries()
            .iter()
            .find(|f| f.width() == size)
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(&frame.rgba_data()[..4], &color);
    }
}

#[test]
fn ico_checks_unused_frames_and_keeps_equal_size_order() {
    let mut source = ico::IconDir::new(ico::ResourceType::Icon);
    for color in [[220, 10, 20, 255], [10, 220, 20, 255], [10, 20, 220, 255]] {
        source.add_entry(
            ico::IconDirEntry::encode_as_bmp(&ico::IconImage::from_rgba_data(
                16,
                16,
                color.repeat(16 * 16),
            ))
            .unwrap(),
        );
    }
    let mut bytes = Vec::new();
    source.write(&mut bytes).unwrap();
    let (root, cache) = fixture("icon.ico", &bytes);
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let result = ico::IconDir::read(
        fs::File::open(pack.theme.resolve("sample.wfi-limit").unwrap()).unwrap(),
    )
    .unwrap();
    // 最小候補の同面積は先頭、拡大時の最大候補の同面積は末尾を使う。
    assert_eq!(
        &result.entries()[0].decode().unwrap().rgba_data()[..4],
        &[220, 10, 20, 255]
    );
    assert_eq!(
        &result
            .entries()
            .last()
            .unwrap()
            .decode()
            .unwrap()
            .rgba_data()[..4],
        &[10, 20, 220, 255]
    );
    // 中央の同寸法フレームは表示に使わないが、復号の検査を省略しない。
    let offset = u32::from_le_bytes(bytes[34..38].try_into().unwrap()) as usize;
    bytes[offset + 14..offset + 16].copy_from_slice(&3u16.to_le_bytes());
    let (root, cache) = fixture("icon.ico", &bytes);
    assert!(import(root.path(), "test", cache.path()).is_err());
    for folder in fs::read_dir(cache.path()).unwrap() {
        assert!(!folder.unwrap().path().join("provenance.json").exists());
    }
}

#[test]
fn cached_ico_is_checked_before_library_frame_allocation() {
    let (root, cache) = fixture("icon.png", &png(16, 16));
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let path = pack
        .theme
        .resolve("sample.wfi-limit")
        .unwrap()
        .to_path_buf();
    let provenance = fs::read(&pack.provenance).unwrap();
    let mut forged = vec![0u8; 22];
    forged[2] = 1;
    forged[4] = 1;
    forged[6] = 16;
    forged[7] = 16;
    forged[14..18].copy_from_slice(&(64u32 * 1024 * 1024 + 1).to_le_bytes());
    forged[18..22].copy_from_slice(&22u32.to_le_bytes());
    fs::write(&path, &forged).unwrap();
    let error = import(root.path(), "test", cache.path()).unwrap_err();
    assert!(format!("{error:#}").contains("frame size limit"));
    assert_eq!(fs::read(path).unwrap(), forged);
    assert_eq!(fs::read(pack.provenance).unwrap(), provenance);
}
