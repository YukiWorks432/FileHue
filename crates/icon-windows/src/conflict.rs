//! 明示確認に結び付けた登録の復旧。計画・退避・再開記録をGUIとCLIで共有する。
use crate::registration_scope::validate_key;
use crate::{Baseline, Settings, UserRegistry, home, load, recovery::Recovery};
use anyhow::{Context, Result, ensure};
use file_icons_core::registry::{Registry, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    PreserveExternal,
    ReapplyTheme,
}

#[derive(Debug)]
pub struct ConfirmationChanged;
impl std::fmt::Display for ConfirmationChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("確認後に状態が変わりました。変更予定を再取得してください / State changed; preview again")
    }
}
impl std::error::Error for ConfirmationChanged {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub state: String,
    pub original: Option<Value>,
    pub recorded: Option<Value>,
    pub current: Option<Value>,
    pub planned: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u32,
    pub confirmation: String,
    pub strategy: Strategy,
    pub pack_id: Option<String>,
    pub pack_label: Option<String>,
    pub pack_version: Option<String>,
    pub selection: file_icons_core::selection::Selection,
    pub backup: PathBuf,
    pub resuming: bool,
    pub conflicts: usize,
    pub writes: usize,
    pub overwrites: usize,
    pub entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct SavedBaseline {
    key: String,
    name: String,
    value: Option<Value>,
}
#[derive(Serialize, Deserialize)]
struct Pending {
    version: u32,
    original: Settings,
    baseline: Vec<SavedBaseline>,
    entries: Vec<Entry>,
}
struct Prepared {
    plan: Plan,
    original: Settings,
    final_settings: Settings,
    baseline: Baseline,
    inputs: BTreeMap<String, Option<Vec<u8>>>,
    dll: Option<PathBuf>,
}
const INPUTS: [&str; 4] = [
    "settings.json",
    "settings.library.json",
    "recovery.json",
    "conflict-operation.json",
];
fn pending_path() -> Result<PathBuf> {
    Ok(home()?.join("conflict-operation.json"))
}
pub fn pending() -> Result<bool> {
    Ok(pending_path()?.try_exists()?)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(crate::STATE_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= crate::STATE_LIMIT,
                "recovery input too large"
            );
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn inputs() -> Result<BTreeMap<String, Option<Vec<u8>>>> {
    let root = home()?;
    INPUTS
        .into_iter()
        .map(|name| Ok((name.into(), read_optional(&root.join(name))?)))
        .collect()
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("recovery parent missing")?)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    file_icons_core::bounded_json::encode(value, crate::STATE_LIMIT)
}

fn validate_assets(
    pack: &file_icons_core::ImportedPack,
    selection: &file_icons_core::selection::Selection,
) -> Result<()> {
    let rules = pack.theme.rules();
    let mut paths = BTreeSet::new();
    let enabled: BTreeSet<_> = pack
        .theme
        .targets()
        .into_iter()
        .filter(|target| selection.target_enabled(target))
        .map(|target| target.id)
        .collect();
    for (kind, map) in [
        ("file", &rules.files),
        ("file", &rules.language_files),
        ("extension", &rules.extensions),
        ("extension", &rules.language_extensions),
        ("pattern", &rules.language_patterns),
    ] {
        for (name, path) in map {
            if enabled.contains(&format!("{kind}:{}", name.to_lowercase())) {
                paths.insert(path);
            }
        }
    }
    for path in paths {
        let file = fs::File::open(path).with_context(|| {
            format!(
                "再適用する素材を開けません / Icon asset unavailable: {}",
                path.display()
            )
        })?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() > 0,
            "再適用する素材を利用できません / Icon asset unavailable: {}",
            path.display()
        );
    }
    Ok(())
}

fn prepare(strategy: Strategy, source: &Path) -> Result<Prepared> {
    // 通常の読み込みで未知形式・不正なライブラリ・独立記録を先に拒否する。
    let settings = load()?;
    let inputs = inputs()?;
    let resumed: Option<Pending> = inputs["conflict-operation.json"]
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes))
        .transpose()?;
    if let Some(saved) = &resumed {
        ensure!(saved.version == 1, "unsupported conflict operation");
    }
    let mut original = resumed
        .as_ref()
        .map(|saved| saved.original.clone())
        .unwrap_or_else(|| settings.clone());
    if resumed.is_none()
        && let Some(record) = Recovery::read(&home()?.join("settings.json"))?
    {
        // loadは通常設定と独立記録の適用状態が違うとactiveを空にする。
        // 復旧では、取り込んだjournalと同じ独立記録からパックを識別する。
        original.active = record.applied_pack().map(str::to_owned);
    }
    crate::registration_scope::validate(&original.journal, &original.created_keys)?;
    let mut baseline = Baseline::new();
    let mut observations = Baseline::new();
    let mut previous = BTreeMap::new();
    if let Some(saved) = &resumed {
        for entry in &saved.entries {
            validate_key(&entry.key, &entry.name)?;
            previous.insert(
                (entry.key.clone(), entry.name.clone()),
                entry.planned.clone(),
            );
        }
        for value in &saved.baseline {
            validate_key(&value.key, &value.name)?;
            ensure!(
                baseline
                    .insert((value.key.clone(), value.name.clone()), value.value.clone())
                    .is_none(),
                "duplicate pending baseline"
            );
        }
        ensure!(
            baseline.keys().all(|key| previous.contains_key(key)),
            "invalid pending baseline"
        );
    }
    let old: BTreeMap<_, _> = original
        .journal
        .iter()
        .map(|c| ((c.key.clone(), c.name.clone()), c))
        .collect();
    let keys: BTreeSet<_> = old.keys().chain(baseline.keys()).cloned().collect();
    let mut states = BTreeMap::new();
    for id in keys {
        let current = UserRegistry.read(&id.0, &id.1)?;
        let old_change = old.get(&id);
        let original_value = baseline
            .get(&id)
            .cloned()
            .unwrap_or_else(|| old_change.and_then(|c| c.before.clone()));
        let ours = old_change.is_some_and(|c| current.as_ref() == Some(&c.after))
            || previous.get(&id).is_some_and(|value| *value == current);
        let state = if current == original_value {
            "original"
        } else if ours {
            "applied"
        } else {
            "external"
        };
        baseline.insert(
            id.clone(),
            if ours {
                original_value
            } else {
                current.clone()
            },
        );
        observations.insert(id.clone(), current);
        states.insert(id, state.to_owned());
    }
    crate::validate_baseline(&baseline)?;
    let mut next = settings.clone();
    next.active = None;
    next.active_selection = Default::default();
    next.journal.clear();
    next.created_keys.clear();
    next.fallbacks.clear();
    next.original_handlers.clear();
    next.icon_exclusions.clear();
    let mut desired = baseline.clone();
    let mut pack_info = (None, None, None);
    let mut dll = None;
    if strategy == Strategy::ReapplyTheme {
        let id = original
            .active
            .as_ref()
            .context("再適用するパックの記録がありません / Applied pack is unavailable")?;
        let pack = settings
            .packs
            .iter()
            .find(|pack| &pack.id == id)
            .context("再適用するパックが見つかりません / Pack is unavailable")?;
        let selection = settings.selections.get(id).cloned().unwrap_or_default();
        ensure!(
            selection.any_enabled(&pack.theme),
            "再適用できる対象がありません / No enabled targets"
        );
        validate_assets(pack, &selection)?;
        let destination = crate::deployment::planned_shell(source)?;
        let registration = crate::plan_with_baseline(
            pack,
            &destination,
            &baseline,
            settings.preserve_existing_icons,
            &settings.icon_protection_rules,
        )?;
        next.active = Some(id.clone());
        next.active_selection = selection;
        next.fallbacks = registration.fallbacks;
        next.original_handlers = registration.original_handlers;
        next.icon_exclusions = registration.icon_exclusions;
        next.created_keys = registration.created_keys;
        next.created_keys.extend(original.created_keys.clone());
        next.created_keys.sort();
        next.created_keys.dedup();
        for change in &registration.changes {
            let id = (change.key.clone(), change.name.clone());
            validate_key(&id.0, &id.1)?;
            baseline
                .entry(id.clone())
                .or_insert_with(|| change.before.clone());
            if let std::collections::btree_map::Entry::Vacant(entry) =
                observations.entry(id.clone())
            {
                entry.insert(UserRegistry.read(&id.0, &id.1)?);
                states.insert(id.clone(), "new".into());
            }
            desired.insert(id, Some(change.after.clone()));
        }
        next.journal = registration.changes;
        next.created_keys.retain(|key| {
            next.journal
                .iter()
                .any(|c| c.key == *key || c.key.starts_with(&format!("{key}\\")))
        });
        pack_info = (
            Some(pack.id.clone()),
            Some(pack.label.clone()),
            Some(pack.version.clone()),
        );
        dll = Some(destination);
    }
    crate::registration_scope::validate(&next.journal, &next.created_keys)?;
    let mut affected: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pack in settings
        .packs
        .iter()
        .filter(|p| original.active.as_ref() == Some(&p.id))
    {
        for extension in pack.theme.registration_extensions() {
            affected
                .entry(crate::association(&extension)?.to_lowercase())
                .or_default()
                .push(extension);
        }
    }
    let entries: Vec<_> = observations
        .into_iter()
        .map(|(id, current)| {
            let class =
                id.0.strip_prefix("Software\\Classes\\")
                    .and_then(|k| k.split('\\').next())
                    .unwrap_or_default();
            Entry {
                extensions: affected
                    .get(&class.to_lowercase())
                    .cloned()
                    .unwrap_or_default(),
                original: baseline.get(&id).cloned().flatten(),
                recorded: old
                    .get(&id)
                    .map(|change| change.after.clone())
                    .or_else(|| previous.get(&id).cloned().flatten()),
                planned: desired.get(&id).cloned().flatten(),
                state: states.get(&id).cloned().unwrap_or_default(),
                key: id.0,
                name: id.1,
                current,
            }
        })
        .collect();
    // 設定・記録の生バイト、全観測値、最終設定（パック・版・選択を含む）に確認を結び付ける。
    let input_hashes: BTreeMap<_, _> = inputs
        .iter()
        .map(|(name, value)| {
            (
                name,
                value
                    .as_ref()
                    .map(|bytes| hex::encode(Sha256::digest(bytes))),
            )
        })
        .collect();
    let root = home()?.canonicalize()?;
    let store = backup_store(&root)?;
    let fingerprint = encode(&(1u32, strategy, &input_hashes, &entries, &next, &dll, &store))?;
    let confirmation = hex::encode(Sha256::digest(fingerprint));
    let plan = Plan {
        schema_version: 1,
        backup: store.join(&confirmation),
        confirmation,
        strategy,
        pack_id: pack_info.0,
        pack_label: pack_info.1,
        pack_version: pack_info.2,
        selection: next.active_selection.clone(),
        resuming: resumed.is_some(),
        conflicts: entries.iter().filter(|e| e.state == "external").count(),
        writes: entries.iter().filter(|e| e.current != e.planned).count(),
        overwrites: entries
            .iter()
            .filter(|e| e.current.is_some() && e.planned.is_some() && e.current != e.planned)
            .count(),
        entries,
    };
    Ok(Prepared {
        plan,
        original,
        final_settings: next,
        baseline,
        inputs,
        dll,
    })
}

pub fn preview(strategy: Strategy, source: &Path) -> Result<Plan> {
    Ok(prepare(strategy, source)?.plan)
}

/// Windowsの保存先仮想化はリンクではない。文字列表記ではなく実体を基準にする。
fn regular_directory(path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && metadata.file_attributes()
                & windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT.0
                == 0,
        "backup directory is a link or reparse point: {}",
        path.display()
    );
    Ok(path.canonicalize()?)
}
fn backup_store(root: &Path) -> Result<PathBuf> {
    let path = root.join("conflict-backups");
    if path.try_exists()? {
        regular_directory(&path)
    } else {
        Ok(path)
    }
}
fn backup_directory(requested: &Path) -> Result<PathBuf> {
    let parent = requested.parent().context("backup parent missing")?;
    fs::create_dir_all(parent)?;
    let store = regular_directory(parent)?;
    // 以後は実体側のパスを使い、仮想化前の表記へ戻らない。
    let directory = store.join(requested.file_name().context("backup name missing")?);
    if !directory.try_exists()? {
        fs::create_dir(&directory)?;
    }
    let actual = regular_directory(&directory)?;
    ensure!(
        actual.parent() == Some(store.as_path()),
        "backup directory escaped storage"
    );
    Ok(actual)
}
fn backup(prepared: &mut Prepared) -> Result<()> {
    prepared.plan.backup = backup_directory(&prepared.plan.backup)?;
    let directory = &prepared.plan.backup;
    for (name, data) in &prepared.inputs {
        if let Some(data) = data {
            let path = directory.join(name);
            if let Some(existing) = read_optional(&path)? {
                ensure!(existing == *data, "backup content differs");
            } else {
                atomic(&path, data)?;
            }
        }
    }
    let data = encode(&prepared.plan)?;
    let path = directory.join("plan.json");
    if let Some(existing) = read_optional(&path)? {
        ensure!(existing == data, "backup plan differs");
    } else {
        atomic(&path, &data)?;
    }
    Ok(())
}

fn write_entries(registry: &mut impl Registry, entries: &[Entry]) -> Result<()> {
    for entry in entries {
        let current = registry.read(&entry.key, &entry.name)?;
        if current == entry.planned {
            continue;
        }
        ensure!(current == entry.current, ConfirmationChanged);
        registry.write(&entry.key, &entry.name, entry.planned.as_ref())?;
    }
    for entry in entries {
        ensure!(
            registry.read(&entry.key, &entry.name)? == entry.planned,
            ConfirmationChanged
        );
    }
    Ok(())
}

pub fn execute(strategy: Strategy, confirmation: &str, source: &Path) -> Result<Plan> {
    let mut prepared = prepare(strategy, source)?;
    ensure!(
        prepared.plan.confirmation == confirmation,
        ConfirmationChanged
    );
    if let Some(expected) = &prepared.dll {
        let installed = crate::stage_shell(source)?;
        ensure!(expected.canonicalize()? == installed, ConfirmationChanged);
        // 初回配置時にだけ判明する保存先仮想化を、同一ファイルの実体へ解決する。
        // 確認したDLLの内容と登録範囲は変えず、Explorerが読めるパスを記録する。
        resolve_shell_path(&mut prepared, &installed)?;
    }
    backup(&mut prepared)?;
    ensure!(inputs()? == prepared.inputs, ConfirmationChanged);
    if let Some(id) = &prepared.final_settings.active {
        let pack = prepared
            .final_settings
            .packs
            .iter()
            .find(|p| &p.id == id)
            .context("Pack is unavailable")?;
        validate_assets(pack, &prepared.final_settings.active_selection)?;
    }
    for entry in &prepared.plan.entries {
        ensure!(
            UserRegistry.read(&entry.key, &entry.name)? == entry.current,
            ConfirmationChanged
        );
    }
    let saved = Pending {
        version: 1,
        original: prepared.original,
        baseline: prepared
            .baseline
            .into_iter()
            .map(|((key, name), value)| SavedBaseline { key, name, value })
            .collect(),
        entries: prepared.plan.entries.clone(),
    };
    atomic(&pending_path()?, &encode(&saved)?)?;
    let result = (|| {
        write_entries(&mut UserRegistry, &prepared.plan.entries)?;
        let current_inputs = inputs()?;
        ensure!(
            INPUTS[..3]
                .iter()
                .all(|name| current_inputs[*name] == prepared.inputs[*name]),
            ConfirmationChanged
        );
        crate::save(&prepared.final_settings)?;
        // 完了した設定と全値を確認してから、従来の独立記録と処理記録を取り除く。
        let path = home()?.join("settings.json");
        ensure!(
            encode(&crate::settings_file::load(&path)?)? == encode(&prepared.final_settings)?,
            "recovery settings verification failed"
        );
        match fs::remove_file(crate::recovery::path(&path)) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        if !prepared.final_settings.journal.is_empty() {
            Recovery::save_new(&prepared.final_settings, &path)?;
        }
        for entry in &prepared.plan.entries {
            ensure!(
                UserRegistry.read(&entry.key, &entry.name)? == entry.planned,
                ConfirmationChanged
            );
        }
        crate::cleanup_empty_keys(&saved.original.created_keys)?;
        fs::remove_file(pending_path()?)?;
        Ok(prepared.plan)
    })();
    crate::notify();
    result.context("復旧を完了できません。処理記録を保持しています。再確認して再試行してください / Recovery incomplete; preview and retry")
}

fn resolve_shell_path(prepared: &mut Prepared, installed: &Path) -> Result<()> {
    let key = format!(
        "Software\\Classes\\CLSID\\{}\\InprocServer32",
        crate::CLSID_TEXT
    );
    let expected = crate::string_value(
        prepared
            .dll
            .as_ref()
            .context("Shell path missing")?
            .to_str()
            .context("invalid Shell path")?,
    );
    let actual = crate::string_value(installed.to_str().context("invalid Shell path")?);
    let entry = prepared
        .plan
        .entries
        .iter_mut()
        .find(|entry| entry.key == key && entry.name.is_empty())
        .context("Shell registration missing")?;
    ensure!(
        entry.planned.as_ref() == Some(&expected),
        ConfirmationChanged
    );
    // 実体への解決によって、確認時になかった書き換えを増やさない。
    ensure!(
        entry.current != entry.planned || entry.current.as_ref() == Some(&actual),
        ConfirmationChanged
    );
    let change = prepared
        .final_settings
        .journal
        .iter_mut()
        .find(|change| change.key == key && change.name.is_empty())
        .context("Shell journal missing")?;
    ensure!(change.after == expected, ConfirmationChanged);
    entry.planned = Some(actual.clone());
    change.after = actual;
    prepared.dll = Some(installed.to_owned());
    prepared.plan.writes = prepared
        .plan
        .entries
        .iter()
        .filter(|entry| entry.current != entry.planned)
        .count();
    prepared.plan.overwrites = prepared
        .plan
        .entries
        .iter()
        .filter(|entry| {
            entry.current.is_some() && entry.planned.is_some() && entry.current != entry.planned
        })
        .count();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_process::CommandOutput;
    #[test]
    fn resolved_shell_path_keeps_backup_and_registration_in_sync() {
        let expected = PathBuf::from(r"C:\logical\shell.dll");
        let installed = PathBuf::from(r"C:\physical\shell.dll");
        let before = crate::string_value("previous.dll");
        let value = crate::string_value(expected.to_str().unwrap());
        let key = format!(
            "Software\\Classes\\CLSID\\{}\\InprocServer32",
            crate::CLSID_TEXT
        );
        let change = file_icons_core::registry::Change {
            key: key.clone(),
            name: String::new(),
            before: Some(before.clone()),
            after: value.clone(),
        };
        let settings = Settings {
            journal: vec![change],
            ..Default::default()
        };
        let entry = Entry {
            key,
            name: String::new(),
            extensions: Vec::new(),
            state: "external".into(),
            original: Some(before.clone()),
            recorded: Some(value.clone()),
            current: Some(before.clone()),
            planned: Some(value.clone()),
        };
        let plan = Plan {
            schema_version: 1,
            confirmation: "confirmed".into(),
            strategy: Strategy::ReapplyTheme,
            pack_id: None,
            pack_label: None,
            pack_version: None,
            selection: Default::default(),
            backup: PathBuf::from("backup"),
            resuming: false,
            conflicts: 1,
            writes: 1,
            overwrites: 1,
            entries: vec![entry],
        };
        let mut prepared = Prepared {
            plan,
            original: settings.clone(),
            final_settings: settings,
            baseline: Default::default(),
            inputs: Default::default(),
            dll: Some(expected.clone()),
        };
        resolve_shell_path(&mut prepared, &installed).unwrap();
        let actual = crate::string_value(installed.to_str().unwrap());
        assert_eq!(prepared.plan.entries[0].planned.as_ref(), Some(&actual));
        assert_eq!(prepared.final_settings.journal[0].after, actual);
        assert_eq!(
            prepared.final_settings.journal[0].before.as_ref(),
            Some(&before)
        );
        assert_eq!(prepared.original.journal[0].after, value);
        assert_eq!((prepared.plan.writes, prepared.plan.overwrites), (1, 1));
        prepared.dll = Some(expected);
        prepared.plan.entries[0].planned = Some(value.clone());
        prepared.plan.entries[0].current = Some(value.clone());
        prepared.final_settings.journal[0].after = value;
        assert!(resolve_shell_path(&mut prepared, &installed).is_err());
    }

    #[test]
    fn backup_storage_resolves_real_directories_and_rejects_junctions() {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("conflict-backups");
        let requested = store.join("confirmation");
        let actual = backup_directory(&requested).unwrap();
        assert_eq!(
            actual.parent(),
            Some(store.canonicalize().unwrap().as_path())
        );
        fs::write(actual.join("proof"), b"backup").unwrap();
        assert_eq!(fs::read(requested.join("proof")).unwrap(), b"backup");
        assert_eq!(backup_directory(&requested).unwrap(), actual);
        let external = root.path().join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), b"unchanged").unwrap();
        let linked_store = root.path().join("linked-store");
        let linked_bucket = store.join("linked-bucket");
        for link in [&linked_store, &linked_bucket] {
            let output = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(&external)
                .test_output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                backup_directory(&if link == &linked_store {
                    link.join("new")
                } else {
                    link.clone()
                })
                .is_err()
            );
        }
        assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
    }

    #[test]
    #[ignore = "明示指定した実環境で保存先仮想化だけを確認する。登録と設定は変更しない"]
    fn backup_storage_in_virtualized_appdata() {
        assert_eq!(std::env::var("WFI_BACKUP_PATH_PROBE").as_deref(), Ok("1"));
        let root = home().unwrap().canonicalize().unwrap();
        let parent = root.join("conflict-backups");
        assert_ne!(
            parent.canonicalize().unwrap(),
            parent,
            "仮想化がある環境だけで実行する"
        );
        let requested = parent.join(format!("filesystem-probe-{}", std::process::id()));
        assert!(!requested.exists());
        let actual = backup_directory(&requested).unwrap();
        assert_eq!(
            actual.parent(),
            Some(parent.canonicalize().unwrap().as_path())
        );
        fs::write(actual.join("proof"), b"filesystem-only").unwrap();
        assert_eq!(
            fs::read(requested.join("proof")).unwrap(),
            b"filesystem-only"
        );
        println!(
            "backup directory resolves to an ordinary directory; proof read through both paths"
        );
        fs::remove_file(actual.join("proof")).unwrap();
        fs::remove_dir(actual).unwrap();
    }

    #[derive(Default)]
    struct Memory {
        values: BTreeMap<String, Option<Value>>,
        writes: usize,
        fail: Option<usize>,
        concurrent: bool,
    }
    impl Registry for Memory {
        fn read(&self, key: &str, _: &str) -> Result<Option<Value>> {
            Ok(self.values.get(key).cloned().flatten())
        }
        fn write(&mut self, key: &str, _: &str, value: Option<&Value>) -> Result<()> {
            self.writes += 1;
            ensure!(
                self.fail != Some(self.writes),
                "injected registry write failure"
            );
            self.values.insert(key.into(), value.cloned());
            if self.concurrent && self.writes == 1 {
                self.values
                    .insert("second".into(), Some(crate::string_value("external")));
            }
            Ok(())
        }
    }
    fn entries() -> Vec<Entry> {
        ["first", "second"]
            .into_iter()
            .map(|key| Entry {
                key: key.into(),
                name: String::new(),
                extensions: vec![],
                state: "applied".into(),
                original: None,
                recorded: Some(crate::string_value("old")),
                current: Some(crate::string_value("old")),
                planned: Some(crate::string_value("new")),
            })
            .collect()
    }
    fn memory() -> Memory {
        Memory {
            values: ["first", "second"]
                .into_iter()
                .map(|k| (k.into(), Some(crate::string_value("old"))))
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn partial_writes_resume_and_concurrent_external_values_are_preserved() {
        let mut registry = memory();
        registry.fail = Some(2);
        assert!(write_entries(&mut registry, &entries()).is_err());
        assert_eq!(registry.values["first"], Some(crate::string_value("new")));
        assert_eq!(registry.values["second"], Some(crate::string_value("old")));
        registry.fail = None;
        write_entries(&mut registry, &entries()).unwrap();
        assert!(
            registry
                .values
                .values()
                .all(|v| *v == Some(crate::string_value("new")))
        );
        let mut registry = memory();
        registry.concurrent = true;
        assert!(
            write_entries(&mut registry, &entries())
                .unwrap_err()
                .is::<ConfirmationChanged>()
        );
        assert_eq!(
            registry.values["second"],
            Some(crate::string_value("external"))
        );
        assert_eq!(registry.writes, 1);
    }
    #[test]
    fn recovery_scope_rejects_open_commands_and_unrelated_com_registration() {
        for key in [
            r"Software\Classes\Demo\shell\open\command",
            r"Software\Other\DefaultIcon",
            r"Software\Classes\CLSID\{OTHER}\InprocServer32",
            r"Software\Classes\Demo\UserChoice",
        ] {
            assert!(crate::registration_scope::validate_key(key, "").is_err());
        }
        assert!(
            crate::registration_scope::validate_key(
                r"Software\Classes\Demo\DefaultIcon",
                "command"
            )
            .is_err()
        );
        crate::registration_scope::validate_key(r"Software\Classes\Demo\shellex\IconHandler", "")
            .unwrap();
    }
}
