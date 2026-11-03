//! 設定やテーマ一覧が壊れても残す、登録処理専用の事前記録。
use crate::{IconLocation, STATE_LIMIT, Settings};
use anyhow::{Context, Result, ensure};
use file_icons_core::registry::Change;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
pub(crate) struct Recovery {
    #[serde(default = "crate::protection::enabled_by_default")]
    preserve_existing_icons: bool,
    #[serde(default)]
    icon_exclusions: Vec<crate::IconExclusion>,
    version: u32,
    active: Option<String>,
    fallbacks: BTreeMap<String, IconLocation>,
    original_handlers: BTreeMap<String, String>,
    journal: Vec<Change>,
    created_keys: Vec<String>,
}

pub(crate) fn path(settings: &Path) -> PathBuf {
    settings.with_file_name("recovery.json")
}

impl Recovery {
    /// 通常設定の適用状態とは分けて、残存登録を作ったパックを識別する。
    pub(crate) fn applied_pack(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub(crate) fn read(settings: &Path) -> Result<Option<Self>> {
        let file = match fs::File::open(path(settings)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(STATE_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= STATE_LIMIT, "recovery record too large");
        let record: Self = serde_json::from_slice(&bytes).context("cannot read recovery record")?;
        ensure!(
            matches!(record.version, 1 | 2) && !record.journal.is_empty(),
            "unsupported or empty recovery record"
        );
        crate::registration_scope::validate(&record.journal, &record.created_keys)?;
        Ok(Some(record))
    }

    pub(crate) fn save_new(settings: &Settings, destination: &Path) -> Result<()> {
        let record = Self {
            version: 2,
            preserve_existing_icons: settings.preserve_existing_icons,
            icon_exclusions: settings.icon_exclusions.clone(),
            active: settings.active.clone(),
            fallbacks: settings.fallbacks.clone(),
            original_handlers: settings.original_handlers.clone(),
            journal: settings.journal.clone(),
            created_keys: settings.created_keys.clone(),
        };
        ensure!(
            !record.journal.is_empty(),
            "cannot save empty recovery record"
        );
        crate::registration_scope::validate(&record.journal, &record.created_keys)?;
        let bytes = serde_json::to_vec_pretty(&record)?;
        ensure!(bytes.len() <= STATE_LIMIT, "recovery record too large");
        let destination = path(destination);
        let root = destination.parent().context("recovery parent missing")?;
        fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        // 前回の復元記録は、壊れていても未確認のまま上書きしない。
        file.persist_noclobber(destination)?;
        Ok(())
    }

    pub(crate) fn merge(self, settings: &mut Settings) {
        settings.icon_exclusions = self.icon_exclusions;
        settings.preserve_existing_icons = self.preserve_existing_icons;
        if settings.active != self.active {
            settings.active = None;
        }
        settings.fallbacks = self.fallbacks;
        settings.original_handlers = self.original_handlers;
        settings.journal = self.journal;
        settings.created_keys = self.created_keys;
    }

    pub(crate) fn fallback(self) -> Settings {
        let mut settings = Settings::default();
        self.merge(&mut settings);
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SettingsCache, load_from, save_to};
    use file_icons_core::registry::Value;

    #[test]
    fn supported_records_keep_original_registry_bytes_and_reject_unknown_versions() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let record_path = path(&settings);
        // 独立した復元記録は通常設定の形式に依存しない。
        let mut document = serde_json::json!({
            "version": 1,
            "active": "example/theme",
            "fallbacks": {},
            "original_handlers": {},
            "journal": [{
                "key": "Software\\Classes\\Example.File\\DefaultIcon",
                "name": "",
                "before": { "kind": 2, "bytes": [7, 0, 0, 0] },
                "after": { "kind": 1, "bytes": [0, 0] }
            }],
            "created_keys": []
        });
        for version in [1, 2] {
            document["version"] = version.into();
            let bytes = serde_json::to_vec(&document).unwrap();
            fs::write(&record_path, &bytes).unwrap();
            let recovered = Recovery::read(&settings).unwrap().unwrap().fallback();
            assert_eq!(recovered.journal[0].before.as_ref().unwrap().kind, 2);
            assert_eq!(
                recovered.journal[0].before.as_ref().unwrap().bytes,
                [7, 0, 0, 0]
            );
            assert_eq!(fs::read(&record_path).unwrap(), bytes);
        }
        document["version"] = 99.into();
        let bytes = serde_json::to_vec(&document).unwrap();
        fs::write(&record_path, &bytes).unwrap();
        assert!(Recovery::read(&settings).is_err());
        assert_eq!(fs::read(&record_path).unwrap(), bytes);

        document["version"] = 2.into();
        document["journal"][0]["key"] =
            "Software\\Classes\\Example.File\\shell\\open\\command".into();
        let bytes = serde_json::to_vec(&document).unwrap();
        fs::write(&record_path, &bytes).unwrap();
        assert!(Recovery::read(&settings).is_err());
        assert_eq!(fs::read(&record_path).unwrap(), bytes);
    }

    #[test]
    fn generated_case_aliases_are_rejected_before_persistence() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("settings.json");
        let mut settings = Settings::default();
        crate::save_to(&settings, &path).unwrap();
        let before = fs::read(&path).unwrap();
        for (class, value) in [("Sample.File", "first.ico"), ("SAMPLE.FILE", "second.ico")] {
            settings.journal.push(Change {
                key: format!("Software\\Classes\\{class}\\DefaultIcon"),
                name: String::new(),
                before: Some(crate::string_value(value)),
                after: crate::string_value("%1"),
            });
        }
        assert!(Recovery::save_new(&settings, &path).is_err());
        assert!(!super::path(&path).exists());
        assert_eq!(fs::read(&path).unwrap(), before);
        let baseline = settings
            .journal
            .iter()
            .map(|c| ((c.key.clone(), c.name.clone()), c.before.clone()))
            .collect();
        let pack = file_icons_core::ImportedPack {
            id: "test".into(),
            version: "1".into(),
            label: "test".into(),
            theme: Default::default(),
            provenance: root.path().into(),
            warnings: Vec::new(),
        };
        assert!(
            crate::plan_with_baseline(
                &pack,
                Path::new("unused.dll"),
                &baseline,
                false,
                &Default::default()
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn independent_record_survives_settings_loss_and_cannot_be_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let settings_path = root.path().join("settings.json");
        save_to(&Settings::default(), &settings_path).unwrap();
        let mut registration = Settings {
            active: Some("sample/theme".into()),
            ..Default::default()
        };
        registration.fallbacks.insert(
            "Sample.File".into(),
            IconLocation {
                path: "original.dll".into(),
                index: -9,
            },
        );
        registration.original_handlers.insert(
            "Sample.File".into(),
            "{00021401-0000-0000-C000-000000000046}".into(),
        );
        let change = Change {
            key: "Software\\Classes\\Sample.File\\DefaultIcon".into(),
            name: String::new(),
            before: Some(Value {
                kind: 2,
                bytes: vec![7, 0, 0, 0],
            }),
            after: Value {
                kind: 1,
                bytes: vec![0, 0],
            },
        };
        registration.journal.push(change.clone());
        Recovery::save_new(&registration, &settings_path).unwrap();
        // 記録の保存後、設定を更新する前に終了しても復元情報を失わない。
        let pending = load_from(&settings_path).unwrap();
        assert!(pending.active.is_none());
        assert_eq!(pending.journal, vec![change]);
        let original = fs::read(path(&settings_path)).unwrap();
        assert!(Recovery::save_new(&registration, &settings_path).is_err());
        assert_eq!(fs::read(path(&settings_path)).unwrap(), original);
        for damaged in [true, false] {
            if damaged {
                fs::write(&settings_path, b"damaged preferences").unwrap();
            } else {
                fs::remove_file(&settings_path).unwrap();
            }
            let fresh = SettingsCache::default().get(&settings_path).unwrap();
            assert!(fresh.active.is_none());
            assert_eq!(fresh.fallbacks["Sample.File"].path, "original.dll");
            assert_eq!(fresh.fallbacks["Sample.File"].index, -9);
            assert_eq!(
                fresh.original_handlers["Sample.File"],
                "{00021401-0000-0000-C000-000000000046}"
            );
            assert_eq!(
                fresh.original_handler("SAMPLE.FILE"),
                Some("{00021401-0000-0000-C000-000000000046}")
            );
            assert_eq!(
                fresh.journal[0].before.as_ref().unwrap().bytes,
                [7, 0, 0, 0]
            );
        }
        fs::write(path(&settings_path), b"broken recovery").unwrap();
        assert!(Recovery::save_new(&registration, &settings_path).is_err());
        assert_eq!(fs::read(path(&settings_path)).unwrap(), b"broken recovery");
    }
}
