//! パッケージ登録の間接参照をWindowsで解決し、素材名とアプリの識別を分離する。
use std::collections::BTreeMap;
use windows::{
    Win32::{
        Storage::Packaging::Appx::PackageFamilyNameFromFullName, UI::Shell::SHLoadIndirectString,
    },
    core::{PCWSTR, PWSTR},
};
use winreg::{RegKey, enums::HKEY_CLASSES_ROOT};

// 値の長さを先に照会してから確保せず、書き換え競合でも64KiBの読込上限を守る。
fn application_value(key: &RegKey, name: &str) -> Option<String> {
    use windows::Win32::System::Registry::{
        HKEY, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE, RegQueryValueExW,
    };
    let name = crate::wide(name);
    let mut bytes = vec![0u8; 65536];
    let mut length = bytes.len() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    if unsafe {
        RegQueryValueExW(
            HKEY(key.raw_handle() as _),
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut length),
        )
    }
    .0 != 0
        || !matches!(kind, REG_SZ | REG_EXPAND_SZ)
        || !length.is_multiple_of(2)
    {
        return None;
    }
    let words = bytes
        .get(..length as usize)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .take_while(|v| *v != 0)
        .collect::<Vec<_>>();
    let text = String::from_utf16(&words).ok()?;
    (text.len() <= 32768).then_some(text)
}

struct Reference<'a> {
    full_name: &'a str,
    family: String,
}
fn reference(value: &str) -> Option<Reference<'_>> {
    if value.len() > 32768 || value.contains('\0') {
        return None;
    }
    let (full_name, uri) = value
        .strip_prefix("@{")?
        .strip_suffix('}')?
        .split_once('?')?;
    if full_name.len() > 255
        || !full_name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        || !uri.starts_with("ms-resource://")
        || uri.contains(['{', '}'])
    {
        return None;
    }
    let input = crate::wide(full_name);
    let mut output = [0u16; 256];
    let mut length = output.len() as u32;
    if unsafe {
        PackageFamilyNameFromFullName(
            PCWSTR(input.as_ptr()),
            &mut length,
            Some(PWSTR(output.as_mut_ptr())),
        )
    }
    .0 != 0
    {
        return None;
    }
    let family = String::from_utf16(output.get(..length.checked_sub(1)? as usize)?).ok()?;
    Some(Reference { full_name, family })
}

pub(crate) fn resolve_reference(value: &str) -> Option<String> {
    reference(value)?;
    let input = crate::wide(value);
    let mut output = vec![0u16; 32768];
    unsafe { SHLoadIndirectString(PCWSTR(input.as_ptr()), &mut output, None) }.ok()?;
    let end = output.iter().position(|c| *c == 0)?;
    let text = String::from_utf16(&output[..end]).ok()?;
    (!text.is_empty() && !text.starts_with('@') && !text.starts_with("ms-resource:"))
        .then_some(text)
}

#[derive(Clone)]
pub(crate) struct PackageSource {
    pub id: String,
    pub name: String,
    pub icon: Option<crate::IconLocation>,
    pub stamp: String,
}

/// 一回の調査にだけ保持し、再調査時の登録変更や言語変更を隠さない。
#[derive(Default)]
pub(crate) struct Resolver {
    strings: BTreeMap<String, Option<String>>,
    sources: BTreeMap<(String, String), Option<PackageSource>>,
}
impl Resolver {
    fn resolve(&mut self, value: &str) -> Option<String> {
        if let Some(found) = self.strings.get(value) {
            return found.clone();
        }
        if self.strings.len() >= 256 {
            return None;
        }
        let found = resolve_reference(value);
        self.strings.insert(value.into(), found.clone());
        found
    }
    pub fn describe(&mut self, raw: &str, class: &str) -> Option<PackageSource> {
        reference(raw)?;
        let cache_key = (raw.to_owned(), class.to_lowercase());
        if let Some(source) = self.sources.get(&cache_key) {
            return source.clone();
        }
        let key = RegKey::predef(HKEY_CLASSES_ROOT)
            .open_subkey(format!("{class}\\Application"))
            .ok();
        let read = |name| {
            key.as_ref()
                .and_then(|key| application_value(key, name))
                .unwrap_or_default()
        };
        let app_id = read("AppUserModelID");
        let name_reference = read("ApplicationName");
        let icon_reference = read("ApplicationIcon");
        let source = self.source(raw, class, app_id, name_reference, icon_reference);
        self.sources.insert(cache_key, source.clone());
        source
    }
    fn source(
        &mut self,
        raw: &str,
        class: &str,
        app_id: String,
        name_reference: String,
        icon_reference: String,
    ) -> Option<PackageSource> {
        let parsed = reference(raw)?;
        let id = app_id
            .split_once('!')
            .filter(|(family, app)| {
                family.eq_ignore_ascii_case(&parsed.family)
                    && !app.is_empty()
                    && app.len() <= 128
                    && app
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            })
            .map(|(_, app)| format!("package:{}!{app}", parsed.family.to_lowercase()))
            .unwrap_or_else(|| {
                format!(
                    "package:{}:class:{}",
                    parsed.family.to_lowercase(),
                    class.to_lowercase()
                )
            });
        let same_family = |value: &str| {
            reference(value).is_some_and(|r| r.family.eq_ignore_ascii_case(&parsed.family))
        };
        let name = if same_family(&name_reference) {
            self.resolve(&name_reference)
        } else if !name_reference.is_empty()
            && !name_reference.starts_with(['@', '%'])
            && !name_reference.starts_with("ms-resource:")
        {
            Some(name_reference.clone())
        } else {
            None
        };
        let name = name.unwrap_or_else(|| {
            parsed
                .full_name
                .split('_')
                .next()
                .unwrap_or(parsed.full_name)
                .into()
        });
        let icon = same_family(&icon_reference).then(|| crate::IconLocation {
            path: icon_reference.clone(),
            index: 0,
        });
        let original_path = self.resolve(raw);
        let icon_path = icon.as_ref().and_then(|i| self.resolve(&i.path));
        let stamp = serde_json::to_string(&[
            app_id,
            name_reference,
            icon_reference,
            original_path
                .as_deref()
                .map(crate::native_preview::module_stamp)
                .unwrap_or_default(),
            icon_path
                .as_deref()
                .map(crate::native_preview::module_stamp)
                .unwrap_or_default(),
        ])
        .ok()?;
        Some(PackageSource {
            id,
            name,
            icon,
            stamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FULL: &str = "Example.App_1.0.0.0_x64__8wekyb3d8bbwe";
    fn icon(full: &str, name: &str) -> String {
        format!("@{{{full}?ms-resource://Example.App/Files/Assets/{name}.png}}")
    }
    #[test]
    fn resources_and_versions_share_only_the_same_application_identity() {
        let mut resolver = Resolver::default();
        let a = resolver
            .source(
                &icon(FULL, "txtfile"),
                "AppX.First",
                "Example.App_8wekyb3d8bbwe!Editor".into(),
                "Example editor".into(),
                icon(FULL, "Logo"),
            )
            .unwrap();
        let newer = "Example.App_2.0.0.0_x64__8wekyb3d8bbwe";
        let b = resolver
            .source(
                &icon(newer, "ps1file"),
                "AppX.Second",
                "Example.App_8wekyb3d8bbwe!Editor".into(),
                "Example editor".into(),
                icon(newer, "Logo"),
            )
            .unwrap();
        assert_eq!(a.id, "package:example.app_8wekyb3d8bbwe!Editor");
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, "Example editor");
        assert_eq!(a.icon.as_ref().unwrap().path, icon(FULL, "Logo"));
        let other = resolver
            .source(
                &icon(FULL, "txtfile"),
                "AppX.First",
                "Example.App_8wekyb3d8bbwe!Viewer".into(),
                "Example viewer".into(),
                icon(FULL, "Logo"),
            )
            .unwrap();
        assert_ne!(other.id, b.id, "同一パッケージの別アプリを統合しないこと");
        let lower = resolver
            .source(
                &icon(FULL, "txtfile"),
                "AppX.Lower",
                "Example.App_8wekyb3d8bbwe!editor".into(),
                "Lower case app".into(),
                String::new(),
            )
            .unwrap();
        assert_ne!(
            a.id, lower.id,
            "アプリIDの大文字小文字を勝手に統合しないこと"
        );
        let foreign = resolver
            .source(
                &icon(FULL, "txtfile"),
                "AppX.Third",
                "Other.App_8wekyb3d8bbwe!Editor".into(),
                String::new(),
                String::new(),
            )
            .unwrap();
        assert_ne!(foreign.id, a.id);
        assert_eq!(foreign.name, "Example.App");
    }
    #[test]
    fn malformed_and_external_indirect_sources_are_rejected() {
        for raw in [
            r"@{C:\outside.pri?ms-resource://Example/Files/icon.png}",
            "@{Example?ms-resource://Example/Files/icon.png}",
            "@{Example.App_1.0.0.0_x64__8wekyb3d8bbwe?https://example.com/icon.png}",
            "@{Example.App_1.0.0.0_x64__8wekyb3d8bbwe?ms-resource://Example/Files/icon.png",
            "%1",
            "@evil.dll,-1",
        ] {
            assert!(reference(raw).is_none(), "{raw}");
            assert!(resolve_reference(raw).is_none());
        }
        assert!(reference(&format!("{}\0", icon(FULL, "Logo"))).is_none());
    }
    #[test]
    #[ignore = "実機のパッケージ登録だけを読み、workへ確認資料を出力する"]
    fn installed_packages_probe() {
        let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../work/package-icons-native");
        std::fs::create_dir_all(&output).unwrap();
        let classes = RegKey::predef(HKEY_CLASSES_ROOT);
        let mut resolver = Resolver::default();
        let mut report = Vec::new();
        for class in classes.enum_keys().filter_map(Result::ok) {
            let Some(raw) = classes
                .open_subkey(format!("{class}\\DefaultIcon"))
                .ok()
                .and_then(|key| key.get_value::<String, _>("").ok())
            else {
                continue;
            };
            if !raw.starts_with("@{") {
                continue;
            }
            let Some(source) = resolver.describe(&raw, &class) else {
                continue;
            };
            assert!(!source.name.contains(".png}"));
            assert!(!source.name.starts_with('@'));
            let index = report.len();
            let original = output.join(format!("{index}-original.png"));
            let software = output.join(format!("{index}-software.png"));
            let original_ok = crate::native_preview::write_original_icon(
                &crate::IconLocation {
                    path: raw,
                    index: 0,
                },
                &original,
            )
            .is_ok();
            let software_ok = source.icon.as_ref().is_some_and(|icon| {
                crate::native_preview::write_original_icon(icon, &software).is_ok()
            });
            report.push(serde_json::json!({"class":class,"id":source.id,"name":source.name,"original":original_ok,"software":software_ok}));
        }
        std::fs::write(
            output.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        assert!(!report.is_empty(), "検証対象のパッケージが未登録です");
        for package in ["windowsnotepad", "windows.photos"] {
            let rows = report
                .iter()
                .filter(|r| r["id"].as_str().unwrap().contains(package))
                .collect::<Vec<_>>();
            assert!(
                rows.len() >= 2,
                "同じアプリの複数素材を確認すること: {package}"
            );
            assert!(
                rows.iter()
                    .all(|r| r["id"] == rows[0]["id"] && r["name"] == rows[0]["name"])
            );
            assert!(
                rows.iter()
                    .all(|r| r["software"] == true && r["original"] == true)
            );
        }
        println!("{}", output.join("report.json").display());
    }
}
