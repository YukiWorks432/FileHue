//! 既存のアイコン指定と確認済みの方針から、拡張子ごとの保護を決める。
use crate::{Baseline, Settings, baseline_string};
use anyhow::Result;
use file_icons_core::ImportedPack;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use winreg::{RegKey, enums::HKEY_CLASSES_ROOT};

pub(crate) fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionReason {
    DefaultIcon,
    IconHandler,
    ProtectedFileType,
    ChangedRegistration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconExclusion {
    /// 空文字は拡張子なし・名前パターン用のUnknownクラスを示す。
    pub extension: String,
    pub class: String,
    pub reason: ProtectionReason,
    /// 保護の根拠となった拡張子またはクラスの登録位置。
    pub source: String,
}

pub fn protection_report(pack: &ImportedPack, settings: &Settings) -> Result<Vec<IconExclusion>> {
    let baseline = settings
        .journal
        .iter()
        .map(|c| ((c.key.clone(), c.name.clone()), c.before.clone()))
        .collect();
    report_with_rules(
        pack,
        &baseline,
        settings.preserve_existing_icons,
        &settings.icon_protection_rules,
    )
}

fn registrations(
    targets: &[String],
    baseline: &Baseline,
    scan: &RegistrationScan,
) -> Result<Vec<IconExclusion>> {
    let mut classes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for extension in targets {
        let class = if extension.is_empty() {
            "unknown"
        } else {
            &scan.associations[extension]
        };
        // Windowsのクラス名は大文字・小文字を区別しない。
        classes
            .entry(class.to_lowercase())
            .or_default()
            .push(extension.clone());
    }
    let mut excluded = Vec::new();
    for (class, extensions) in classes {
        let mandatory = crate::protected_class(&class)?;
        for extension in extensions {
            let mut found = mandatory.then(|| (ProtectionReason::ProtectedFileType, class.clone()));
            if found.is_none() {
                // 専用処理を静的な画像より優先し、拡張子自身の指定をクラスより先に使う。
                for suffix in ["shellex\\IconHandler", "DefaultIcon"] {
                    for key in (!extension.is_empty())
                        .then(|| format!(".{extension}"))
                        .into_iter()
                        .chain(std::iter::once(class.clone()))
                    {
                        let source = format!("{key}\\{suffix}");
                        if let Some(value) = baseline_string(&source, baseline)?
                            && !value.trim().is_empty()
                        {
                            let reason = if suffix == "DefaultIcon" {
                                ProtectionReason::DefaultIcon
                            } else {
                                anyhow::ensure!(
                                    crate::handler_clsid(&value).ok() != Some(crate::CLSID),
                                    "本アプリの既存登録に対応する復元記録がありません / Recovery record is missing for our handler"
                                );
                                ProtectionReason::IconHandler
                            };
                            found = Some((reason, source));
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
            }
            if let Some((reason, source)) = found {
                excluded.push(IconExclusion {
                    extension,
                    class: class.clone(),
                    reason,
                    source,
                });
            }
        }
    }
    Ok(excluded)
}

/// 一回の調査内だけで観測を共有する。保存・適用時には必ず新しく作る。
pub(crate) struct RegistrationScan {
    associations: BTreeMap<String, String>,
    own_icons: Vec<IconExclusion>,
}

impl RegistrationScan {
    pub(crate) fn association(&self, extension: &str) -> &str {
        &self.associations[extension]
    }

    pub(crate) fn read(pack: &ImportedPack, baseline: &Baseline) -> Result<Self> {
        let mut scan = Self {
            associations: BTreeMap::new(),
            own_icons: Vec::new(),
        };
        for extension in pack.theme.registration_extensions() {
            scan.associations
                .insert(extension.clone(), crate::association(&extension)?);
        }
        for key in RegKey::predef(HKEY_CLASSES_ROOT).enum_keys() {
            let key = key?;
            let Some(extension) = key
                .strip_prefix('.')
                .filter(|extension| !extension.is_empty())
            else {
                continue;
            };
            for (suffix, reason) in [
                ("shellex\\IconHandler", ProtectionReason::IconHandler),
                ("DefaultIcon", ProtectionReason::DefaultIcon),
            ] {
                let source = format!("{key}\\{suffix}");
                if baseline_string(&source, baseline)?.is_some_and(|value| !value.trim().is_empty())
                {
                    let extension = extension.to_lowercase();
                    let class = match scan.associations.entry(extension.clone()) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(crate::association(&extension)?)
                        }
                    };
                    scan.own_icons.push(IconExclusion {
                        extension,
                        class: class.to_lowercase(),
                        source,
                        reason,
                    });
                    break;
                }
            }
        }
        Ok(scan)
    }
}

pub fn inspect_existing_icons(
    pack: &ImportedPack,
    settings: &Settings,
) -> Result<Vec<crate::ExistingIcon>> {
    inspect_icons(pack, settings, true, &[])
}

/// 画面を開いている間に指定を解除した種類も、新しく登録を調査して表示する。
pub fn inspect_retained_icons(
    pack: &ImportedPack,
    settings: &Settings,
    retained: &[String],
) -> Result<Vec<crate::ExistingIcon>> {
    inspect_icons(pack, settings, true, retained)
}

/// 対象画面の見込みは、実際の登録計画と同じ範囲で判定する。
pub fn inspect_pack_icons(
    pack: &ImportedPack,
    settings: &Settings,
) -> Result<Vec<crate::ExistingIcon>> {
    inspect_icons(pack, settings, false, &[])
}

fn inspect_icons(
    pack: &ImportedPack,
    settings: &Settings,
    include_saved: bool,
    retained: &[String],
) -> Result<Vec<crate::ExistingIcon>> {
    let baseline = settings
        .journal
        .iter()
        .map(|c| ((c.key.clone(), c.name.clone()), c.before.clone()))
        .collect();
    let mut scan = RegistrationScan::read(pack, &baseline)?;
    // 一覧では、別パックで確認した保存済み指定も見失わない。
    if include_saved {
        for extension in settings
            .icon_protection_rules
            .extensions
            .keys()
            .chain(
                settings
                    .icon_protection_rules
                    .providers
                    .values()
                    .flat_map(|rules| rules.keys()),
            )
            .chain(retained.iter())
        {
            if !extension.is_empty() && !scan.associations.contains_key(extension) {
                scan.associations
                    .insert(extension.clone(), crate::association(extension)?);
            }
        }
    }
    inventory(pack, &baseline, settings, &scan, include_saved, retained)
}

fn inventory(
    pack: &ImportedPack,
    baseline: &Baseline,
    settings: &Settings,
    scan: &RegistrationScan,
    include_saved: bool,
    retained: &[String],
) -> Result<Vec<crate::ExistingIcon>> {
    let mut targets = pack.theme.registration_extensions();
    let retained: std::collections::BTreeSet<_> = retained.iter().map(String::as_str).collect();
    if pack.theme.needs_unknown_class() {
        targets.push(String::new());
    }
    if include_saved {
        targets.extend(settings.icon_protection_rules.extensions.keys().cloned());
        targets.extend(
            settings
                .icon_protection_rules
                .providers
                .values()
                .flat_map(|rules| rules.keys().cloned()),
        );
        targets.extend(retained.iter().map(|extension| (*extension).to_owned()));
        targets.sort();
        targets.dedup();
    }
    let mut registrations = registrations(&targets, baseline, scan)?;
    // 元の指定が消えた場合も、以前に確認した例外を無言で別の登録へ移さない。
    for extension in targets {
        let has_rule = settings
            .icon_protection_rules
            .extensions
            .contains_key(&extension)
            || settings
                .icon_protection_rules
                .providers
                .values()
                .any(|r| r.contains_key(&extension));
        if (has_rule || retained.contains(extension.as_str()))
            && !registrations.iter().any(|r| r.extension == extension)
        {
            let class = if extension.is_empty() {
                "unknown".into()
            } else {
                scan.associations[&extension].to_lowercase()
            };
            registrations.push(IconExclusion {
                extension,
                source: format!("{class}\\DefaultIcon"),
                class,
                reason: ProtectionReason::ChangedRegistration,
            });
        }
    }
    let mut classes = pack
        .theme
        .registration_extensions()
        .iter()
        .map(|extension| scan.associations[extension].to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    if pack.theme.needs_unknown_class() {
        classes.insert("unknown".into());
    }
    // 同じクラスを共有するパック外の種類も一緒に判断する。別の提供元を無言で上書きしない。
    for own in &scan.own_icons {
        if !classes.contains(&own.class) {
            continue;
        }
        if let Some(row) = registrations
            .iter_mut()
            .find(|r| r.extension.eq_ignore_ascii_case(&own.extension))
        {
            if row.reason != ProtectionReason::ProtectedFileType
                && !(row.reason == ProtectionReason::IconHandler
                    && own.reason == ProtectionReason::DefaultIcon)
            {
                row.source = own.source.clone();
                row.reason = own.reason.clone();
            }
        } else {
            let mut own = own.clone();
            if crate::protected_class(&own.class)? {
                own.reason = ProtectionReason::ProtectedFileType;
            }
            registrations.push(own);
        }
    }
    let mut descriptions = crate::icon_policy::Descriptions::default();
    let rows = registrations
        .into_iter()
        .map(|r| crate::icon_policy::describe(r, baseline, settings, &mut descriptions))
        .collect::<Result<_>>()?;
    Ok(rows)
}

pub(crate) fn report_with_rules(
    pack: &ImportedPack,
    baseline: &Baseline,
    enabled: bool,
    rules: &crate::IconProtectionRules,
) -> Result<Vec<IconExclusion>> {
    let scan = RegistrationScan::read(pack, baseline)?;
    report_with_scan(pack, baseline, enabled, rules, &scan)
}

pub(crate) fn report_with_scan(
    pack: &ImportedPack,
    baseline: &Baseline,
    enabled: bool,
    rules: &crate::IconProtectionRules,
    scan: &RegistrationScan,
) -> Result<Vec<IconExclusion>> {
    let settings = Settings {
        preserve_existing_icons: enabled,
        icon_protection_rules: rules.clone(),
        ..Default::default()
    };
    let rows = inventory(pack, baseline, &settings, scan, false, &[])?;
    // パック外の種類も、共有クラス経由で名前パターンに一致し得るため方針を保持する。
    Ok(rows
        .into_iter()
        .filter(|row| row.protect)
        .map(|row| row.registration)
        .collect())
}

/// 共有クラスへの登録で拡張子側の元の画像・専用処理を失わないようにする。
/// 登録自体は変更しないため、復元用の書き込み記録には追加しない。
pub(crate) fn extension_originals(
    scan: &RegistrationScan,
    baseline: &Baseline,
    classes: &std::collections::BTreeSet<String>,
    fallbacks: &mut BTreeMap<String, crate::IconLocation>,
    handlers: &mut BTreeMap<String, String>,
) -> Result<()> {
    for own in &scan.own_icons {
        if !classes.contains(&own.class) {
            continue;
        }
        let key = format!(".{}", own.extension);
        for suffix in ["shellex\\IconHandler", "DefaultIcon"] {
            let Some(value) = baseline_string(&format!("{key}\\{suffix}"), baseline)?
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            if suffix == "DefaultIcon" {
                fallbacks.insert(key.clone(), crate::location_from_value(&value)?);
            } else {
                let id = crate::handler_clsid(&value)?;
                anyhow::ensure!(id != crate::CLSID, "original handler points to FileHue");
                handlers.insert(key.clone(), value);
            }
        }
    }
    Ok(())
}
