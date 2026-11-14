//! 表示名とは分離した提供元の識別と、登録内容に結び付けた保護例外。
use crate::{Baseline, IconExclusion, IconLocation, ProtectionReason, Settings, baseline_string};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use windows::{
    Win32::UI::Shell::*,
    core::{PCWSTR, PWSTR},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconProtectionRules {
    pub providers: BTreeMap<String, BTreeMap<String, ProtectionRule>>,
    pub extensions: BTreeMap<String, ProtectionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectionRule {
    pub fingerprint: String,
    pub protect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingIcon {
    pub registration: IconExclusion,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub provider_inferred: bool,
    pub open_app: Option<String>,
    pub fingerprint: String,
    pub reference: String,
    pub location: Option<IconLocation>,
    pub provider_location: Option<IconLocation>,
    pub protect: bool,
    pub needs_review: bool,
    pub always_protected: bool,
}

impl ExistingIcon {
    /// 調査済みの登録に下書きの方針を適用する。レジストリや画像資源へアクセスしない。
    pub fn update_protection(&mut self, rules: &IconProtectionRules, default: bool) {
        let (protect, needs_review) = rules.decision(self, default);
        self.protect = self.always_protected || protect;
        self.needs_review = needs_review;
    }

    pub fn group_id(&self) -> String {
        self.provider_id
            .clone()
            .unwrap_or_else(|| format!("unknown:{}", self.registration.class.to_lowercase()))
    }
}

impl IconProtectionRules {
    pub fn set_extension(&mut self, row: &ExistingIcon, protect: Option<bool>) {
        if let Some(protect) = protect {
            self.extensions.insert(
                row.registration.extension.clone(),
                ProtectionRule {
                    fingerprint: row.fingerprint.clone(),
                    protect,
                },
            );
        } else {
            self.extensions.remove(&row.registration.extension);
        }
    }

    /// 一括許可は確認済みの種類に限定する。新しく関連付いた種類には伝播させない。
    pub fn set_provider(&mut self, rows: &[ExistingIcon], provider: &str, protect: Option<bool>) {
        let affected = rows
            .iter()
            .filter(|r| r.provider_id.as_deref() == Some(provider))
            .map(|r| r.registration.extension.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for (id, rules) in &mut self.providers {
            if id != provider {
                rules.retain(|extension, _| !affected.contains(extension));
            }
        }
        self.providers.retain(|_, rules| !rules.is_empty());
        if let Some(protect) = protect {
            let rules = self.providers.entry(provider.into()).or_default();
            for row in rows
                .iter()
                .filter(|r| r.provider_id.as_deref() == Some(provider))
            {
                rules.insert(
                    row.registration.extension.clone(),
                    ProtectionRule {
                        fingerprint: row.fingerprint.clone(),
                        protect,
                    },
                );
            }
        } else {
            self.providers.remove(provider);
        }
    }

    pub(crate) fn decision(&self, row: &ExistingIcon, default: bool) -> (bool, bool) {
        self.decision_with_override(
            row,
            default,
            self.extensions.get(&row.registration.extension),
        )
    }

    /// 個別指定を解除した場合の使用側を、登録変更と常時保護も含めて返す。
    pub fn inherited_protection(&self, row: &ExistingIcon, default: bool) -> bool {
        row.always_protected || self.decision_with_override(row, default, None).0
    }

    fn decision_with_override(
        &self,
        row: &ExistingIcon,
        default: bool,
        individual: Option<&ProtectionRule>,
    ) -> (bool, bool) {
        let extension = &row.registration.extension;
        let provider = row
            .provider_id
            .as_ref()
            .and_then(|id| self.providers.get(id))
            .and_then(|rules| rules.get(extension));
        let rule = individual.or(provider);
        match rule {
            Some(rule) if rule.fingerprint == row.fingerprint => (rule.protect, false),
            Some(_) => (true, true),
            None if self
                .providers
                .values()
                .any(|rules| rules.contains_key(extension)) =>
            {
                (true, true)
            }
            None => (default, false),
        }
    }
}

fn association_text(extension: &str, kind: ASSOCSTR) -> Option<String> {
    let key = crate::wide(&format!(".{extension}"));
    let mut len = 0;
    let _ = unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            kind,
            PCWSTR(key.as_ptr()),
            PCWSTR::null(),
            None,
            &mut len,
        )
    };
    if !(2..32768).contains(&len) {
        return None;
    }
    let mut buffer = vec![0; len as usize];
    unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            kind,
            PCWSTR(key.as_ptr()),
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
        .ok()
        .ok()?;
    }
    let end = buffer.iter().position(|v| *v == 0)?;
    String::from_utf16(&buffer[..end]).ok()
}

/// 現在の関連付けが指定する静的アイコン資源を読み取る。
pub fn association_icon_location(extension: &str) -> Option<IconLocation> {
    let value = association_text(extension.trim_start_matches('.'), ASSOCSTR_DEFAULTICON)?;
    if value.is_empty() || value == "%1" {
        return None;
    }
    crate::location_from_value(&value).ok()
}

#[derive(Clone)]
struct Module {
    identity: String,
    name: Option<String>,
    stamp: String,
}

/// 登録の調査一回で共有し、次の調査・保存・適用へ観測結果を持ち越さない。
#[derive(Default)]
pub(crate) struct Descriptions {
    packages: crate::package_icons::Resolver,
    modules: BTreeMap<String, Module>,
    applications: BTreeMap<(String, String), bool>,
}

impl Descriptions {
    fn module(&mut self, path: &str) -> &Module {
        self.modules
            .entry(path.to_lowercase())
            .or_insert_with(|| Module {
                identity: crate::native_preview::module_identity(path),
                name: crate::native_preview::module_name(path),
                stamp: crate::native_preview::module_stamp(path),
            })
    }
}

pub(crate) fn describe(
    registration: IconExclusion,
    baseline: &Baseline,
    settings: &Settings,
    descriptions: &mut Descriptions,
) -> Result<ExistingIcon> {
    let raw = baseline_string(&registration.source, baseline)?.unwrap_or_default();
    let handler = registration.reason == ProtectionReason::IconHandler;
    let module = if handler {
        baseline_string(&format!("CLSID\\{raw}\\InprocServer32"), baseline)?
    } else if registration.reason == ProtectionReason::DefaultIcon && raw != "%1" {
        crate::location_from_value(&raw).ok().map(|l| l.path)
    } else {
        None
    };
    let location =
        if !handler && registration.reason == ProtectionReason::DefaultIcon && raw != "%1" {
            crate::location_from_value(&raw).ok()
        } else {
            None
        };
    let module = module.filter(|s| !s.is_empty());
    let mut provider_id = module.as_ref().map(|p| {
        format!(
            "{}:{}",
            if handler { "handler" } else { "module" },
            descriptions.module(p).identity
        )
    });
    let mut provider_name = module.as_ref().map(|path| {
        descriptions.module(path).name.clone().unwrap_or_else(|| {
            std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
    });
    let open_exe = association_text(&registration.extension, ASSOCSTR_EXECUTABLE);
    let open_app = association_text(&registration.extension, ASSOCSTR_FRIENDLYAPPNAME);
    let provider_inferred = module
        .as_ref()
        .zip(open_exe.as_ref())
        .is_some_and(|(module, exe)| {
            *descriptions
                .applications
                .entry((module.to_lowercase(), exe.to_lowercase()))
                .or_insert_with(|| crate::native_preview::application_resource(module, exe))
        });
    if provider_inferred {
        provider_id = open_exe
            .as_ref()
            .map(|exe| format!("application:{}", descriptions.module(exe).identity));
        provider_name = open_app.clone().or(provider_name);
    }
    let application_location = open_exe.as_ref().map(|path| IconLocation {
        path: path.clone(),
        index: 0,
    });
    let mut provider_location = if provider_inferred {
        application_location.clone()
    } else {
        module.as_ref().map(|path| IconLocation {
            path: path.clone(),
            index: 0,
        })
    };
    let package = descriptions.packages.describe(&raw, &registration.class);
    if let Some(package) = &package {
        provider_id = Some(package.id.clone());
        provider_name = Some(package.name.clone());
        provider_location = package.icon.clone();
    } else if raw.starts_with("@{") {
        // 壊れた間接参照の末尾をソフト名として見せない。
        provider_id = None;
        provider_name = None;
        provider_location = None;
    }
    let mut hash = Sha256::new();
    if let Some(package) = &package {
        hash.update(package.stamp.as_bytes());
    }
    if let Some(module) = &module {
        hash.update(descriptions.module(module).stamp.as_bytes());
    }

    for item in [
        registration.class.to_lowercase(),
        registration.source.to_lowercase(),
        raw.clone(),
        module.unwrap_or_default().to_lowercase(),
        open_exe.unwrap_or_default().to_lowercase(),
    ] {
        hash.update((item.len() as u64).to_le_bytes());
        hash.update(item.as_bytes());
    }
    for key in [&registration.class, &format!(".{}", registration.extension)] {
        for suffix in ["DefaultIcon", "shellex\\IconHandler"] {
            let value = baseline_string(&format!("{key}\\{suffix}"), baseline)?.unwrap_or_default();
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
    }
    let always_protected = registration.reason == ProtectionReason::ProtectedFileType;
    let mut row = ExistingIcon {
        registration,
        provider_id,
        provider_name,
        provider_inferred,
        open_app,
        fingerprint: hex::encode(hash.finalize()),
        reference: raw,
        location,
        provider_location,
        protect: true,
        needs_review: false,
        always_protected,
    };
    row.update_protection(
        &settings.icon_protection_rules,
        settings.preserve_existing_icons,
    );
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_observations_expire_at_the_end_of_an_inspection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resource.dll");
        std::fs::write(&path, b"first").unwrap();
        let path = path.to_string_lossy();
        let mut inspection = Descriptions::default();
        let before = inspection.module(&path).stamp.clone();
        std::fs::write(path.as_ref(), b"replacement resource").unwrap();
        assert_eq!(inspection.module(&path).stamp, before);
        let mut next = Descriptions::default();
        assert_ne!(next.module(&path).stamp, before);
    }
    fn row(extension: &str, provider: &str) -> ExistingIcon {
        ExistingIcon {
            registration: IconExclusion {
                extension: extension.into(),
                class: format!("{provider}.File"),
                source: format!("{provider}.File\\DefaultIcon"),
                reason: ProtectionReason::DefaultIcon,
            },
            provider_id: Some(provider.into()),
            provider_name: Some(provider.into()),
            provider_inferred: false,
            open_app: Some("別の既定アプリ".into()),
            fingerprint: format!("{provider}-original"),
            reference: "resource.ico".into(),
            location: None,
            provider_location: None,
            protect: true,
            needs_review: false,
            always_protected: false,
        }
    }
    #[test]
    fn provider_rules_are_scoped_and_extension_overrides_require_current_identity() {
        let a = row("ai", "Adobe");
        let b = row("eps", "Adobe");
        let c = row("txt", "Editor");
        let mut rules = IconProtectionRules::default();
        rules.set_provider(&[a.clone(), b.clone(), c.clone()], "Adobe", Some(false));
        assert_eq!(rules.decision(&a, true), (false, false));
        assert_eq!(rules.decision(&b, true), (false, false));
        assert_eq!(rules.decision(&c, true), (true, false));
        assert_eq!(rules.decision(&row("pdf", "Adobe"), true), (true, false));
        rules.set_extension(&a, Some(true));
        assert_eq!(rules.decision(&a, false), (true, false));
        assert!(
            !rules.inherited_protection(&a, true),
            "個別保護を外すとソフトのパック指定を継承する"
        );
        assert_eq!(rules.decision(&b, true), (false, false));
        rules.set_extension(&a, None);
        assert_eq!(rules.decision(&a, true), (false, false));
        let mut changed = a.clone();
        changed.fingerprint = "changed-resource".into();
        assert_eq!(rules.decision(&changed, false), (true, true));
        assert!(
            rules.inherited_protection(&changed, false),
            "登録変更の保護は継承結果でも維持する"
        );
        changed.provider_id = Some("NewProvider".into());
        assert_eq!(rules.decision(&changed, false), (true, true));
        let mut reviewed = rules.clone();
        reviewed.set_provider(&[changed.clone()], "NewProvider", None);
        assert_eq!(reviewed.decision(&changed, false), (false, false));
        assert_eq!(reviewed.decision(&b, true), (false, false));
        let saved = serde_json::to_vec(&rules).unwrap();
        let reloaded: IconProtectionRules = serde_json::from_slice(&saved).unwrap();
        assert_eq!(reloaded.decision(&a, true), (false, false));
        rules.set_provider(&[], "Adobe", None);
        assert_eq!(rules.decision(&a, true), (true, false));
        rules.set_extension(&a, Some(false));
        assert!(
            rules.inherited_protection(&a, true),
            "ソフト指定がなければ全体設定へ戻る"
        );
        let mut protected = a.clone();
        protected.always_protected = true;
        assert!(rules.inherited_protection(&protected, false));
    }
}
