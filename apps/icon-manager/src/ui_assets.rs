//! 標準部品の素材に、この画面で使うLucideアイコンだけを追加する。
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        BrushCleaning,
        Check,
        ChevronDown,
        ChevronUp,
        Download,
        ExternalLink,
        File,
        FolderOpen,
        Info,
        Library,
        Loader,
        Monitor,
        Moon,
        Package,
        PackageOpen,
        Palette,
        PanelsTopLeft,
        RefreshCw,
        RotateCcw,
        Search,
        ShieldCheck,
        TriangleAlert,
        Sun,
        Trash,
        X,
    ]
);

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_and_component_icons_are_available_without_network() {
        // 実画面で空白になったアプリ用素材と、標準部品用素材の両方を確認する。
        for path in [
            "icons/package.svg",
            "icons/monitor.svg",
            "icons/trash.svg",
            "icons/brush-cleaning.svg",
            "icons/chevron-down.svg",
            "icons/loader.svg",
            "icons/shield-check.svg",
            "icons/triangle-alert.svg",
        ] {
            let bytes = Assets.load(path).unwrap().expect(path);
            assert!(std::str::from_utf8(&bytes).unwrap().contains("<svg"));
        }
    }
}
