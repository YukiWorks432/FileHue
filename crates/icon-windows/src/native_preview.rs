//! 元の画像資源だけを抽出する。外部COMハンドラーや関連付けコマンドは実行しない。
use anyhow::{Result, ensure};
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Graphics::Gdi::*,
        Storage::FileSystem::*,
        UI::{Shell::SHDefExtractIconW, WindowsAndMessaging::*},
    },
    core::PCWSTR,
};

fn local_module(path: &str) -> Result<PathBuf> {
    let mut path = PathBuf::from(path);
    if !path.is_absolute() && path.components().count() == 1 {
        path = PathBuf::from(std::env::var("SystemRoot")?)
            .join("System32")
            .join(path);
    }
    let initial = path.to_string_lossy();
    ensure!(
        initial.as_bytes().get(1) == Some(&b':')
            && matches!(initial.as_bytes().get(2), Some(b'\\' | b'/')),
        "local absolute path required"
    );
    let root = crate::wide(&initial[..3]);
    ensure!(
        unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } == 3,
        "fixed drive required"
    );
    let path = path.canonicalize()?;
    let text = path.to_string_lossy().replace(r"\\?\", "");
    ensure!(
        text.as_bytes().get(1) == Some(&b':'),
        "local drive required"
    );
    let root = crate::wide(&text[..3]);
    ensure!(
        unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } == 3,
        "fixed drive required"
    );
    ensure!(
        path.metadata()?.len() <= 64 * 1024 * 1024,
        "icon resource too large"
    );
    Ok(PathBuf::from(text))
}

pub(crate) fn module_identity(path: &str) -> String {
    local_module(path)
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_else(|_| path.to_lowercase())
}

pub(crate) fn module_stamp(path: &str) -> String {
    local_module(path)
        .ok()
        .and_then(|p| p.metadata().ok())
        .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
        .unwrap_or_default()
}

/// 画像素材が既定アプリの専用設置先にある場合だけ、そのアプリのまとまりと推定する。
pub(crate) fn application_resource(module: &str, executable: &str) -> bool {
    let module = module_identity(module);
    let executable = module_identity(executable);
    let Some(parent) = Path::new(&executable).parent() else {
        return false;
    };
    let system = std::env::var("SystemRoot")
        .unwrap_or_default()
        .to_lowercase();
    parent.components().count() >= 3
        && !parent.starts_with(system)
        && Path::new(&module).starts_with(parent)
}

pub(crate) fn module_name(path: &str) -> Option<String> {
    let path = local_module(path).ok()?;
    let path = crate::wide(&path.to_string_lossy());
    let length = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if !(1..=1024 * 1024).contains(&length) {
        return None;
    }
    let mut data = vec![0u8; length as usize];
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            length,
            data.as_mut_ptr().cast(),
        )
        .ok()?;
    }
    let mut translation = std::ptr::null_mut();
    let mut len = 0;
    let key = crate::wide(r"\VarFileInfo\Translation");
    if !unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(key.as_ptr()),
            &mut translation,
            &mut len,
        )
    }
    .as_bool()
        || len < 4
    {
        return None;
    }
    let words = unsafe { std::slice::from_raw_parts(translation.cast::<u16>(), 2) };
    let key = crate::wide(&format!(
        r"\StringFileInfo\{:04x}{:04x}\FileDescription",
        words[0], words[1]
    ));
    let mut value = std::ptr::null_mut();
    if !unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(key.as_ptr()),
            &mut value,
            &mut len,
        )
    }
    .as_bool()
        || !(2..=1024).contains(&len)
    {
        return None;
    }
    let text = String::from_utf16(unsafe {
        std::slice::from_raw_parts(value.cast::<u16>(), len as usize - 1)
    })
    .ok()?;
    Some(text)
}

pub fn write_original_icon(location: &crate::IconLocation, destination: &Path) -> Result<()> {
    let resolved = if location.path.starts_with("@{") {
        crate::package_icons::resolve_reference(&location.path)
            .ok_or_else(|| anyhow::anyhow!("package icon unavailable"))?
    } else {
        location.path.clone()
    };
    let path = local_module(&resolved)?;
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return write_png(&path, destination);
    }
    let path = crate::wide(&path.to_string_lossy());
    let mut icon = HICON::default();
    unsafe {
        SHDefExtractIconW(
            PCWSTR(path.as_ptr()),
            location.index,
            0,
            Some(&mut icon),
            None,
            32,
        )
        .ok()?;
    }
    ensure!(!icon.is_invalid(), "icon missing");
    let result = render(icon);
    unsafe {
        let _ = DestroyIcon(icon);
    }
    std::fs::write(destination, result?)?;
    Ok(())
}

fn write_png(path: &Path, destination: &Path) -> Result<()> {
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 64 * 1024 * 1024, "icon resource too large");
    let mut reader = image::ImageReader::with_format(Cursor::new(data), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()?
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut output = image::RgbaImage::new(32, 32);
    image::imageops::overlay(
        &mut output,
        &image,
        i64::from((32 - image.width()) / 2),
        i64::from((32 - image.height()) / 2),
    );
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(output).write_to(&mut encoded, image::ImageFormat::Png)?;
    std::fs::write(destination, encoded.into_inner())?;
    Ok(())
}

fn render(icon: HICON) -> Result<Vec<u8>> {
    let dc = unsafe { CreateCompatibleDC(None) };
    ensure!(!dc.is_invalid(), "drawing context unavailable");
    let result = (|| -> Result<Vec<u8>> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
        let previous = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        let result = (|| -> Result<Vec<u8>> {
            ensure!(!bits.is_null(), "bitmap unavailable");
            unsafe {
                bits.cast::<u8>().write_bytes(0, 32 * 32 * 4);
                DrawIconEx(dc, 0, 0, icon, 32, 32, 0, None, DI_NORMAL)?;
                GdiFlush().ok()?;
            }
            let mut rgba =
                unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), 32 * 32 * 4) }.to_vec();
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            // 古い資源ではANDマスクから透明度を復元し、黒い四角で埋めない。
            if rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                unsafe {
                    DrawIconEx(dc, 0, 0, icon, 32, 32, 0, None, DI_MASK)?;
                    GdiFlush().ok()?;
                }
                let mask_bytes =
                    unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), 32 * 32 * 4) };
                for (pixel, mask) in rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(mask_bytes.as_chunks::<4>().0)
                {
                    pixel[3] = 255 - mask[0];
                }
            } else {
                for pixel in rgba.as_chunks_mut::<4>().0 {
                    let alpha = u32::from(pixel[3]);
                    for value in &mut pixel[..3] {
                        *value = (u32::from(*value) * 255)
                            .checked_div(alpha)
                            .unwrap_or(0)
                            .min(255) as u8;
                    }
                }
            }
            let image = image::RgbaImage::from_raw(32, 32, rgba).unwrap();
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, image::ImageFormat::Png)?;
            Ok(encoded.into_inner())
        })();
        unsafe {
            let _ = SelectObject(dc, previous);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        result
    })();
    unsafe {
        let _ = DeleteDC(dc);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_resource_preview_preserves_channels_and_transparency() {
        let source = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("package.json"), r#"{"name":"native-test","publisher":"test","version":"1.0.0","contributes":{"iconThemes":[{"id":"test","label":"Test","path":"theme.json"}]}}"#).unwrap();
        std::fs::write(source.path().join("theme.json"), r#"{"iconDefinitions":{"icon":{"iconPath":"icon.svg"}},"fileExtensions":{"probe":"icon"}}"#).unwrap();
        std::fs::write(source.path().join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="16" height="32" fill="#f0140a"/><rect x="16" width="16" height="32" fill="#f0140a" fill-opacity="0.5"/></svg>"##).unwrap();
        let pack = file_icons_core::import(source.path(), "test", cache.path()).unwrap();
        let destination = cache.path().join("preview.png");
        write_original_icon(
            &crate::IconLocation {
                path: pack
                    .theme
                    .resolve("sample.probe")
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                index: 0,
            },
            &destination,
        )
        .unwrap();
        let image = image::open(destination).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (32, 32));
        assert_eq!(image.get_pixel(8, 16).0, [240, 20, 10, 255]);
        let translucent = image.get_pixel(24, 16).0;
        assert_eq!(translucent[3], 128);
        // SVGのICO化とWindowsのアルファ合成の各8bit量子化を含む。
        // 不透明色は完全一致し、半透明色は二回の乗算・逆変換で最大4階調を許容する。
        for (actual, expected) in translucent[..3].iter().copied().zip([240u8, 20, 10]) {
            assert!(actual.abs_diff(expected) <= 4, "{translucent:?}");
        }
    }

    #[test]
    fn classic_bitmap_icons_keep_their_transparent_mask() {
        let root = tempfile::tempdir().unwrap();
        let resource = root.path().join("classic.ico");
        // 独立した24bit DIBとANDマスク。右半分を透明、左半分を赤にする。
        let mut bitmap = vec![0u8; 40];
        bitmap[..4].copy_from_slice(&40u32.to_le_bytes());
        bitmap[4..8].copy_from_slice(&16i32.to_le_bytes());
        bitmap[8..12].copy_from_slice(&32i32.to_le_bytes());
        bitmap[12..14].copy_from_slice(&1u16.to_le_bytes());
        bitmap[14..16].copy_from_slice(&24u16.to_le_bytes());
        for _ in 0..16 * 16 {
            bitmap.extend_from_slice(&[10, 20, 240]);
        }
        for _ in 0..16 {
            bitmap.extend_from_slice(&[0, 255, 0, 0]);
        }
        let mut ico = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 24, 0];
        ico.extend_from_slice(&(bitmap.len() as u32).to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend(bitmap);
        std::fs::write(&resource, ico).unwrap();
        let output = root.path().join("classic.png");
        write_original_icon(
            &crate::IconLocation {
                path: resource.to_string_lossy().into_owned(),
                index: 0,
            },
            &output,
        )
        .unwrap();
        let image = image::open(output).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(8, 16).0, [240, 20, 10, 255]);
        assert_eq!(image.get_pixel(24, 16).0[3], 0);
    }

    #[test]
    fn raster_previews_fit_the_fixed_canvas_and_reject_large_dimensions() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("asset.png");
        let destination = root.path().join("preview.png");
        image::RgbaImage::from_pixel(16, 8, image::Rgba([20, 80, 200, 128]))
            .save(&source)
            .unwrap();
        write_original_icon(
            &crate::IconLocation {
                path: source.to_string_lossy().into_owned(),
                index: 0,
            },
            &destination,
        )
        .unwrap();
        let decoded = image::open(&destination).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (32, 32));
        assert_eq!(decoded.get_pixel(16, 16).0, [20, 80, 200, 128]);
        assert_eq!(decoded.get_pixel(16, 0).0[3], 0);
        std::fs::remove_file(&destination).unwrap();
        image::RgbaImage::new(4097, 1).save(&source).unwrap();
        assert!(
            write_original_icon(
                &crate::IconLocation {
                    path: source.to_string_lossy().into_owned(),
                    index: 0
                },
                &destination
            )
            .is_err()
        );
        assert!(!destination.exists());
    }

    #[test]
    fn unsupported_or_oversized_resources_do_not_write_previews() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("preview.png");
        for path in [r"\\invalid-server\share\icon.dll", "C:", "%1"] {
            assert!(
                write_original_icon(
                    &crate::IconLocation {
                        path: path.into(),
                        index: 0
                    },
                    &destination
                )
                .is_err()
            );
        }
        let large = root.path().join("large.dll");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(
            write_original_icon(
                &crate::IconLocation {
                    path: large.to_string_lossy().into_owned(),
                    index: 0
                },
                &destination
            )
            .is_err()
        );
        assert!(!destination.exists());
        assert!(application_resource(
            r"C:\Program Files\Example\resources\file.ico",
            r"C:\Program Files\Example\app.exe"
        ));
        assert!(!application_resource(
            r"C:\Other\file.ico",
            r"C:\Program Files\Example\app.exe"
        ));
        let system = std::env::var("SystemRoot").unwrap();
        assert!(!application_resource(
            &format!("{system}\\System32\\shell32.dll"),
            &format!("{system}\\System32\\notepad.exe")
        ));
    }
}
