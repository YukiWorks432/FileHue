//! 表示寸法に合うICO画像を非同期で読み込み、GPUIの資産キャッシュで共有する。
use gpui_kit::{App, Asset, ImageCacheError, Img, RenderImage, Window, img};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Copy)]
pub enum PreviewSize {
    Rem(f32),
    Pixels(f32),
}

#[derive(Clone, Hash)]
struct Source {
    path: PathBuf,
    pixels: u32,
}

enum PreviewAsset {}

impl Asset for PreviewAsset {
    type Source = Source;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        source: Source,
        cx: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        cx.background_executor().spawn(async move {
            load_icon(&source.path, source.pixels)
                .map(|image| {
                    let frame = image::Frame::new(image);
                    Arc::new(RenderImage::new([frame]))
                })
                .map_err(ImageCacheError::from)
        })
    }
}

pub fn preview(path: PathBuf, size: PreviewSize) -> Img {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"))
    {
        return img(path);
    }
    img(move |window: &mut Window, cx: &mut App| {
        let logical = match size {
            PreviewSize::Rem(value) => f32::from(window.rem_size()) * value,
            PreviewSize::Pixels(value) => value,
        };
        let pixels = (logical * window.scale_factor()).ceil().clamp(1., 512.) as u32;
        window.use_asset::<PreviewAsset>(
            &Source {
                path: path.clone(),
                pixels,
            },
            cx,
        )
    })
}

fn load_icon(path: &std::path::Path, pixels: u32) -> anyhow::Result<image::RgbaImage> {
    use anyhow::Context as _;
    use std::io::Read as _;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 32 * 1024 * 1024, "icon too large");
    file_icons_core::theme::validate_icon(&bytes)?;
    let directory = ico::IconDir::read(std::io::Cursor::new(bytes))?;
    let entry = directory
        .entries()
        .iter()
        .min_by_key(|entry| {
            let size = entry.width().max(entry.height());
            (size < pixels, size.abs_diff(pixels))
        })
        .context("icon contains no images")?;
    let decoded = entry.decode()?;
    let mut image = image::RgbaImage::from_raw(
        decoded.width(),
        decoded.height(),
        decoded.rgba_data().to_vec(),
    )
    .context("invalid icon pixels")?;
    if image.width() != pixels || image.height() != pixels {
        // 補間時に透明画素の色が輪郭へ混ざらないよう、事前乗算してから戻す。
        for p in image.pixels_mut() {
            let a = u32::from(p[3]);
            for c in &mut p.0[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }
        image = image::imageops::resize(
            &image,
            pixels,
            pixels,
            image::imageops::FilterType::Lanczos3,
        );
        for p in image.pixels_mut() {
            let a = u32::from(p[3]);
            for c in &mut p.0[..3] {
                *c = (u32::from(*c) * 255 + a / 2)
                    .checked_div(a)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
    }
    // GPUIのRenderImageはBGRA形式。
    for p in image.pixels_mut() {
        p.0.swap(0, 2);
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "test-support")]
    #[test]
    fn requests_a_new_preview_after_display_scale_changes() {
        use gpui_kit::{Context, IntoElement, Render, Styled, TestApp};

        struct View(PathBuf);
        impl Render for View {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                preview(self.0.clone(), PreviewSize::Rem(1.5)).size_6()
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icon.ico");
        let mut icons = ico::IconDir::new(ico::ResourceType::Icon);
        icons.add_entry(
            ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
                48,
                48,
                [255, 0, 0, 255].repeat(48 * 48),
            ))
            .unwrap(),
        );
        icons.write(std::fs::File::create(&path).unwrap()).unwrap();
        let mut app = TestApp::new();
        let mut window = app.open_window(|window, _| {
            window.set_rem_size(16.);
            View(path.clone())
        });
        for (scale, expected) in [(1., 24), (1.25, 30), (1.5, 36), (2., 48)] {
            window.simulate_scale_factor_change(scale);
            window.draw();
            app.run_until_parked();
            window.draw();
            window.update(|_, window, cx| {
                let result = window.get_asset::<PreviewAsset>(
                    &Source {
                        path: path.clone(),
                        pixels: expected,
                    },
                    cx,
                );
                let image = result
                    .expect("表示した画像の読み込み結果")
                    .expect("ICOの復号");
                assert_eq!(image.size(0).width.0, expected as i32);
            });
        }
    }

    #[test]
    fn selects_nearest_sufficient_frame_for_display_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sizes.ico");
        let mut icons = ico::IconDir::new(ico::ResourceType::Icon);
        for (size, color) in [
            (16, [0, 0, 255, 255]),
            (48, [255, 0, 0, 255]),
            (256, [0, 255, 0, 255]),
        ] {
            let rgba = color.repeat((size * size) as usize);
            icons.add_entry(
                ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(size, size, rgba))
                    .unwrap(),
            );
        }
        icons.write(std::fs::File::create(&path).unwrap()).unwrap();
        let image = load_icon(&path, 36).unwrap();
        assert_eq!(image.dimensions(), (36, 36));
        assert_eq!(image.get_pixel(18, 18).0, [0, 0, 255, 255]);
        let large = load_icon(&path, 128).unwrap();
        assert_eq!(large.get_pixel(64, 64).0, [0, 255, 0, 255]);
    }

    #[test]
    fn transparent_colors_do_not_bleed_into_visible_edges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transparent.ico");
        let mut rgba = Vec::new();
        for _y in 0..16 {
            for x in 0..16 {
                rgba.extend_from_slice(if x < 8 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 0]
                });
            }
        }
        let mut icons = ico::IconDir::new(ico::ResourceType::Icon);
        icons.add_entry(
            ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(16, 16, rgba)).unwrap(),
        );
        icons.write(std::fs::File::create(&path).unwrap()).unwrap();
        let image = load_icon(&path, 36).unwrap();
        assert!(image.pixels().any(|p| p[3] > 0 && p[3] < 255));
        assert!(image.pixels().filter(|p| p[3] > 0).all(|p| p[0] == 0));
    }

    #[test]
    fn rejects_invalid_frame_directory_before_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.ico");
        std::fs::write(&path, [0, 0, 1, 0, 255, 255]).unwrap();
        assert!(load_icon(&path, 36).is_err());
    }
}
