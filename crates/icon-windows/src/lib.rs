//! ユーザー単位の状態保存と、復元可能なShell登録。

#[cfg(test)]
#[path = "../../../test-support/process.rs"]
mod test_process;
use anyhow::{Context, Result, ensure};
use file_icons_core::{
    ImportedPack,
    registry::{self, Change, Registry, Value},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use windows::{
    Win32::UI::Shell::*,
    core::{GUID, PCWSTR, PWSTR},
};
use winreg::{RegKey, RegValue, enums::*};

mod activation;
pub use activation::{
    PreviousPackRestored, activate, activate_options, activate_protection, activate_selection,
};

mod packs;
pub use packs::{PackCleanupPending, cleanup_packs, remove_pack};

mod shell_cleanup;
pub use shell_cleanup::{ShellCleanupPending, cleanup_staged_shell};

mod deployment;
pub use deployment::stage_shell;

pub mod conflict;
mod icon_policy;
pub mod native_preview;
mod package_icons;
mod protection;
pub use icon_policy::{ExistingIcon, IconProtectionRules, ProtectionRule};
pub use protection::{inspect_existing_icons, inspect_pack_icons, inspect_retained_icons};
mod recovery;
mod registration_scope;
mod settings_file;
pub use protection::{IconExclusion, ProtectionReason, protection_report};
use recovery::Recovery;
pub use settings_file::UnsupportedSettingsFormat;

pub(crate) const STATE_LIMIT: usize = 16 * 1024 * 1024;

pub const CLSID: GUID = GUID::from_u128(0x730b15a9_834e_4c12_b64e_4a49e81a3f30);
pub const CLSID_TEXT: &str = "{730B15A9-834E-4C12-B64E-4A49E81A3F30}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconLocation {
    pub path: String,
    pub index: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "protection::enabled_by_default")]
    pub preserve_existing_icons: bool,
    #[serde(default)]
    pub icon_protection_rules: IconProtectionRules,
    /// 適用時に除外した種類。旧DLLから呼ばれてもテーマ画像を返さない。
    #[serde(default)]
    pub icon_exclusions: Vec<IconExclusion>,
    pub language: String,
    #[serde(default)]
    pub selections: BTreeMap<String, file_icons_core::selection::Selection>,
    #[serde(default)]
    pub active_selection: file_icons_core::selection::Selection,
    pub packs: Vec<ImportedPack>,
    #[serde(default)]
    pub pending_pack_removals: Vec<PathBuf>,
    pub active: Option<String>,
    pub fallbacks: BTreeMap<String, IconLocation>,
    /// 通常表示へ委譲する、登録前のハンドラー。
    #[serde(default)]
    pub original_handlers: BTreeMap<String, String>,
    /// 登録前に永続化し、完全に復元するまで保持する。
    pub journal: Vec<Change>,
    #[serde(default)]
    pub created_keys: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preserve_existing_icons: true,
            icon_protection_rules: Default::default(),
            icon_exclusions: Vec::new(),
            language: String::new(),
            selections: BTreeMap::new(),
            active_selection: Default::default(),
            packs: Vec::new(),
            pending_pack_removals: Vec::new(),
            active: None,
            fallbacks: BTreeMap::new(),
            original_handlers: BTreeMap::new(),
            journal: Vec::new(),
            created_keys: Vec::new(),
        }
    }
}

impl Settings {
    pub fn fallback(&self, class: &str) -> Option<&IconLocation> {
        class_value(&self.fallbacks, class)
    }
    pub fn original_handler(&self, class: &str) -> Option<&str> {
        class_value(&self.original_handlers, class).map(String::as_str)
    }
}

fn class_value<'a, T>(values: &'a BTreeMap<String, T>, class: &str) -> Option<&'a T> {
    let identity = class.to_lowercase();
    values
        .get(class)
        .or_else(|| values.get(&identity))
        .or_else(|| {
            // 旧形式の記録は元の綴りを保持している。
            values
                .iter()
                .find(|(key, _)| key.to_lowercase() == identity)
                .map(|(_, value)| value)
        })
}

type Stamp = Option<(SystemTime, u64)>;

/// 同じ設定は再解析しない。破損・消失時は最後の復元情報で通常表示を選ぶ。
#[derive(Default)]
pub struct SettingsCache {
    current: Option<(PathBuf, (Stamp, Stamp), Arc<Settings>)>,
    retry_after: Option<Instant>,
}

impl SettingsCache {
    pub fn get(&mut self, path: &Path) -> Result<Arc<Settings>> {
        let stamp = (file_stamp(path), file_stamp(&recovery::path(path)));
        if let Some((previous_path, previous_stamp, settings)) = &self.current
            && previous_path == path
            && previous_stamp == &stamp
            && self
                .retry_after
                .is_none_or(|deadline| Instant::now() < deadline)
        {
            return Ok(settings.clone());
        }
        let loaded = if stamp.0.is_none()
            && self
                .current
                .as_ref()
                .is_some_and(|(previous_path, _, _)| previous_path == path)
        {
            Err(anyhow::anyhow!("settings disappeared"))
        } else {
            settings_file::runtime(path).and_then(|mut settings| {
                if let Some(record) = Recovery::read(path)? {
                    record.merge(&mut settings);
                }
                Ok(settings)
            })
        };
        // 退避した結果は正常な読み込みと区別し、同じファイルでも再試行する。
        self.retry_after = loaded
            .is_err()
            .then(|| Instant::now() + Duration::from_secs(1));
        let settings = match loaded {
            Ok(settings) => settings,
            Err(error) => {
                if let Ok(Some(record)) = Recovery::read(path) {
                    record.fallback()
                } else {
                    let previous = self
                        .current
                        .as_ref()
                        .filter(|(previous_path, _, _)| previous_path == path)
                        .map(|(_, _, previous)| (**previous).clone())
                        .or_else(|| settings_file::runtime(path).ok())
                        .ok_or(error)?;
                    let mut fallback = previous;
                    fallback.active = None;
                    fallback
                }
            }
        };
        let settings = Arc::new(settings);
        self.current = Some((path.into(), stamp, settings.clone()));
        Ok(settings)
    }
}

fn file_stamp(path: &Path) -> Stamp {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| Some((metadata.modified().ok()?, metadata.len())))
}

pub fn home() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?)
            .join("WindowsFileIconsDev"),
    )
}

pub fn load() -> Result<Settings> {
    load_from(&home()?.join("settings.json"))
}
pub fn load_from(path: &Path) -> Result<Settings> {
    let mut settings = settings_file::load(path)?;
    if let Some(record) = Recovery::read(path)? {
        record.merge(&mut settings);
    }
    Ok(settings)
}

/// 設定を上書きせず、登録前の表示・復元情報だけを読む。
pub fn load_recovery() -> Result<Option<Settings>> {
    let path = home()?.join("settings.json");
    Ok(Recovery::read(&path)?.map(|record| {
        let mut settings = record.fallback();
        // 一覧が健全なら、登録情報だけの破損で取得済みパックまで失わない。
        let _ = settings_file::attach_library(&mut settings, &path);
        settings
    }))
}

pub fn save(settings: &Settings) -> Result<()> {
    save_to(settings, &home()?.join("settings.json"))
}
pub fn save_to(settings: &Settings, path: &Path) -> Result<()> {
    settings_file::save(settings, path)
}

pub struct UserRegistry;

fn key_exists(path: &str) -> Result<bool> {
    match RegKey::predef(HKEY_CURRENT_USER).open_subkey(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// 適用前に存在しなかったキーだけを、空であることを確認して深い順に除去する。
fn cleanup_empty_keys(paths: &[String]) -> Result<()> {
    let mut paths = paths.to_vec();
    paths.sort_by_key(|path| std::cmp::Reverse(path.split('\\').count()));
    paths.dedup();
    let root = RegKey::predef(HKEY_CURRENT_USER);
    for path in paths {
        ensure!(
            path.starts_with("Software\\Classes\\"),
            "invalid cleanup target"
        );
        let key = match root.open_subkey(&path) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let empty = key.enum_keys().next().transpose()?.is_none()
            && key.enum_values().next().transpose()?.is_none();
        drop(key);
        if empty {
            match root.delete_subkey(&path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

impl Registry for UserRegistry {
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
        match RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(key)
            .and_then(|key| key.get_raw_value(name))
        {
            Ok(value) => Ok(Some(Value {
                kind: value.vtype as u32,
                bytes: value.bytes.into_owned(),
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn write(&mut self, key: &str, name: &str, value: Option<&Value>) -> Result<()> {
        ensure!(
            key.starts_with("Software\\Classes\\"),
            "registry target is outside Classes"
        );
        let root = RegKey::predef(HKEY_CURRENT_USER);
        if let Some(value) = value {
            let kind = match value.kind {
                0 => REG_NONE,
                1 => REG_SZ,
                2 => REG_EXPAND_SZ,
                3 => REG_BINARY,
                4 => REG_DWORD,
                5 => REG_DWORD_BIG_ENDIAN,
                6 => REG_LINK,
                7 => REG_MULTI_SZ,
                8 => REG_RESOURCE_LIST,
                9 => REG_FULL_RESOURCE_DESCRIPTOR,
                10 => REG_RESOURCE_REQUIREMENTS_LIST,
                11 => REG_QWORD,
                _ => anyhow::bail!("unsupported registry value type"),
            };
            root.create_subkey(key)?.0.set_raw_value(
                name,
                &RegValue {
                    vtype: kind,
                    bytes: (&value.bytes[..]).into(),
                },
            )?;
        } else {
            match root
                .open_subkey_with_flags(key, KEY_WRITE)
                .and_then(|key| key.delete_value(name))
            {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn association(extension: &str) -> Result<String> {
    let result = query_association(extension)?;
    ensure!(
        !result.contains(['\\', '/', '\0']) || application_association(&result),
        "invalid ProgID for extension .{extension}: {result}"
    );
    Ok(result)
}

// Windowsのアプリ登録は複数の種類に共有される。照会には使うが、登録は保護する。
fn application_association(class: &str) -> bool {
    let Some((root, executable)) = class.split_once('\\') else {
        return false;
    };
    root.eq_ignore_ascii_case("Applications")
        && executable.len() > 4
        && executable.to_ascii_lowercase().ends_with(".exe")
        && !executable
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
}

// WindowsはApplications\\Illustrator.exeのような関連付けも返す。
// 復旧一覧の照会には利用できるが、書き込み先のクラスにはそのまま使わない。
pub(crate) fn query_association(extension: &str) -> Result<String> {
    if extension.is_empty() {
        return Ok("Unknown".into());
    }
    ensure!(!extension.contains(['\\', '/', '\0']), "invalid extension");
    let bare_extension = extension;
    let extension = wide(&format!(".{extension}"));
    let mut size = 0;
    let result = unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_PROGID,
            PCWSTR(extension.as_ptr()),
            PCWSTR::null(),
            None,
            &mut size,
        )
    };
    if result
        == windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_NO_ASSOCIATION.0)
        && size == 0
    {
        return Ok(format!(".{bare_extension}"));
    }
    ensure!(
        size > 1 && size < 32768,
        "no effective file type for extension: {result:?} (size={size})"
    );
    let mut buffer = vec![0; size as usize];
    unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_PROGID,
            PCWSTR(extension.as_ptr()),
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size,
        )
        .ok()?
    };
    let result = String::from_utf16(
        &buffer[..buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len())],
    )?;
    ensure!(
        !result.is_empty(),
        "empty association for extension .{bare_extension}"
    );
    if result.eq_ignore_ascii_case("Unknown") {
        Ok(format!(".{bare_extension}"))
    } else {
        Ok(result)
    }
}

fn string_value(text: &str) -> Value {
    Value {
        kind: 1,
        bytes: wide(text)
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect(),
    }
}

fn class_string(key: &str) -> Result<Option<String>> {
    match RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(key)
        .and_then(|key| key.get_value(""))
    {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn location_from_value(value: &str) -> Result<IconLocation> {
    if value.is_empty() {
        return Ok(IconLocation {
            path: format!("{}\\System32\\shell32.dll", std::env::var("SystemRoot")?),
            index: 0,
        });
    }
    let (path, index) = value
        .rsplit_once(',')
        .and_then(|(path, index)| index.trim().parse().ok().map(|index| (path, index)))
        .unwrap_or((value, 0));
    let path = path.trim().trim_matches('"');
    let input = wide(path);
    let mut output = vec![0u16; 32768];
    let length = unsafe {
        windows::Win32::System::Environment::ExpandEnvironmentStringsW(
            PCWSTR(input.as_ptr()),
            Some(&mut output),
        )
    };
    ensure!(
        length > 0 && length as usize <= output.len(),
        "invalid default icon path"
    );
    Ok(IconLocation {
        path: String::from_utf16(&output[..length as usize - 1])?,
        index,
    })
}

/// 変更予定と元の表示を保存し、対象外は従来のハンドラーへ委譲する。
pub struct RegistrationPlan {
    pub icon_exclusions: Vec<IconExclusion>,
    pub original_handlers: BTreeMap<String, String>,
    pub changes: Vec<Change>,
    pub fallbacks: BTreeMap<String, IconLocation>,
    pub created_keys: Vec<String>,
}

pub fn handler_clsid(value: &str) -> Result<GUID> {
    let value = value.trim();
    let value = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(value);
    Ok(GUID::try_from(value)?)
}

/// 別拡張子が同じクラスを共有していても、保護対象の登録を上書きしない。
pub fn protected_class(progid: &str) -> Result<bool> {
    if application_association(progid)
        || progid.eq_ignore_ascii_case("lnkfile")
        || progid.eq_ignore_ascii_case("exefile")
    {
        return Ok(true);
    }
    for extension in ["lnk", "exe"] {
        if progid.eq_ignore_ascii_case(&association(extension)?) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn plan(pack: &ImportedPack, dll: &Path) -> Result<RegistrationPlan> {
    plan_with_protection(pack, dll, true)
}

pub fn plan_with_protection(
    pack: &ImportedPack,
    dll: &Path,
    enabled: bool,
) -> Result<RegistrationPlan> {
    ensure!(dll.is_file(), "Shell DLL is missing");
    plan_with_baseline(
        pack,
        &dll.canonicalize()?,
        &BTreeMap::new(),
        enabled,
        &IconProtectionRules::default(),
    )
}

pub(crate) type Baseline = BTreeMap<(String, String), Option<Value>>;

fn baseline_entry<'a>(
    key: &str,
    name: &str,
    baseline: &'a Baseline,
) -> Option<(&'a (String, String), &'a Option<Value>)> {
    let identity = (key.to_lowercase(), name.to_lowercase());
    baseline
        .get_key_value(&(key.into(), name.into()))
        .or_else(|| {
            baseline
                .iter()
                .find(|((key, name), _)| (key.to_lowercase(), name.to_lowercase()) == identity)
        })
}

pub(crate) fn validate_baseline(baseline: &Baseline) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for (key, name) in baseline.keys() {
        registration_scope::validate_key(key, name)?;
        ensure!(
            seen.insert((key.to_lowercase(), name.to_lowercase())),
            "duplicate registration baseline"
        );
    }
    Ok(())
}

fn baseline_string(key: &str, baseline: &Baseline) -> Result<Option<String>> {
    let full = format!("Software\\Classes\\{key}");
    let Some((_, value)) = baseline_entry(&full, "", baseline) else {
        return class_string(key);
    };
    if let Some(value) = value {
        ensure!(
            matches!(value.kind, 1 | 2) && value.bytes.len() % 2 == 0,
            "invalid icon registration string"
        );
        let words = value
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|w| *w != 0)
            .collect::<Vec<_>>();
        return Ok(Some(String::from_utf16(&words)?));
    }
    match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(full)
        .and_then(|k| k.get_value(""))
    {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn plan_with_baseline(
    pack: &ImportedPack,
    dll: &Path,
    baseline: &Baseline,
    preserve_existing_icons: bool,
    rules: &IconProtectionRules,
) -> Result<RegistrationPlan> {
    validate_baseline(baseline)?;
    let registry = UserRegistry;
    let mut targets = BTreeMap::new();
    let mut fallbacks = BTreeMap::new();
    let mut original_handlers = BTreeMap::new();
    let scan = protection::RegistrationScan::read(pack, baseline)?;
    let icon_exclusions =
        protection::report_with_scan(pack, baseline, preserve_existing_icons, rules, &scan)?;
    let mut classes: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for extension in pack.theme.registration_extensions() {
        // 判定と書き込み先を同じ調査結果へ結び付け、登録用には元の綴りを保つ。
        let class = scan.association(&extension);
        classes
            .entry(class.to_lowercase())
            .or_insert_with(|| (class.to_owned(), Vec::new()))
            .1
            .push(extension);
    }
    if pack.theme.needs_unknown_class() {
        classes
            .entry("unknown".into())
            .or_insert_with(|| ("Unknown".into(), Vec::new()))
            .1
            .push(String::new());
    }
    let mut registered_classes = std::collections::BTreeSet::new();
    for (identity, (progid, extensions)) in classes {
        if protected_class(&progid)?
            || extensions.iter().all(|extension| {
                icon_exclusions
                    .iter()
                    .any(|item| item.extension == *extension)
            })
        {
            continue;
        }
        registered_classes.insert(identity.clone());
        let handler_key = format!("{progid}\\shellex\\IconHandler");
        if let Some(existing) = baseline_string(&handler_key, baseline)?
            && !existing.is_empty()
        {
            let original = handler_clsid(&existing)
                .with_context(|| format!("invalid existing icon handler: {progid}"))?;
            ensure!(
                original != CLSID,
                "our handler is already registered: {progid}"
            );
            original_handlers.insert(identity.clone(), existing);
        }
        fallbacks.insert(
            identity,
            location_from_value(
                &baseline_string(&format!("{progid}\\DefaultIcon"), baseline)?.unwrap_or_default(),
            )?,
        );
        targets.insert(
            format!("Software\\Classes\\{handler_key}"),
            CLSID_TEXT.to_owned(),
        );
        targets.insert(
            format!("Software\\Classes\\{progid}\\DefaultIcon"),
            "%1".to_owned(),
        );
    }
    if targets.is_empty() {
        return Ok(RegistrationPlan {
            icon_exclusions,
            original_handlers,
            changes: Vec::new(),
            fallbacks,
            created_keys: Vec::new(),
        });
    }
    protection::extension_originals(
        &scan,
        baseline,
        &registered_classes,
        &mut fallbacks,
        &mut original_handlers,
    )?;
    let clsid_key = format!("Software\\Classes\\CLSID\\{CLSID_TEXT}\\InprocServer32");
    targets.insert(clsid_key.clone(), dll.to_string_lossy().into_owned());
    let mut changes = targets
        .into_iter()
        .map(|(key, value)| {
            let previous = baseline_entry(&key, "", baseline);
            Ok(Change {
                before: match previous {
                    Some((_, value)) => value.clone(),
                    None => registry.read(&key, "")?,
                },
                key: previous.map(|(id, _)| id.0.clone()).unwrap_or(key),
                name: String::new(),
                after: string_value(&value),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    changes.push(Change {
        before: match baseline.get(&(clsid_key.clone(), "ThreadingModel".into())) {
            Some(value) => value.clone(),
            None => registry.read(&clsid_key, "ThreadingModel")?,
        },
        key: clsid_key,
        name: "ThreadingModel".into(),
        after: string_value("Apartment"),
    });
    let mut created_keys = std::collections::BTreeSet::new();
    for change in &changes {
        let parts = change.key.split('\\').collect::<Vec<_>>();
        for length in 3..=parts.len() {
            let path = parts[..length].join("\\");
            if !key_exists(&path)? {
                created_keys.insert(path);
            }
        }
    }
    let created_keys = created_keys.into_iter().collect::<Vec<_>>();
    registration_scope::validate(&changes, &created_keys)?;
    Ok(RegistrationPlan {
        icon_exclusions,
        original_handlers,
        changes,
        fallbacks,
        created_keys,
    })
}

pub fn notify() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

pub fn apply(settings: &mut Settings, pack_id: &str, dll: &Path) -> Result<()> {
    ensure!(
        settings.journal.is_empty(),
        "restore current registration before applying another pack"
    );
    ensure!(
        !recovery::path(&home()?.join("settings.json")).try_exists()?,
        "finish pending recovery before applying a pack"
    );
    let pack = settings
        .packs
        .iter()
        .find(|pack| pack.id == pack_id)
        .context("pack not found")?;
    let installed = stage_shell(dll)?;
    let plan = plan_with_baseline(
        pack,
        &installed,
        &BTreeMap::new(),
        settings.preserve_existing_icons,
        &settings.icon_protection_rules,
    )?;
    let mut next = settings.clone();
    next.active = Some(pack_id.into());
    next.active_selection = settings
        .selections
        .get(pack_id)
        .cloned()
        .unwrap_or_default();
    next.fallbacks = plan.fallbacks;
    next.original_handlers = plan.original_handlers;
    next.journal = plan.changes.clone();
    next.created_keys = plan.created_keys;
    next.icon_exclusions = plan.icon_exclusions;
    if plan.changes.is_empty() {
        save(&next)?;
        *settings = next;
        notify();
        return Ok(());
    }
    // 復元記録の保存が成功するまではレジストリを変更しない。
    let result = registry::apply(&mut UserRegistry, &plan.changes, |_| {
        Recovery::save_new(&next, &home()?.join("settings.json"))?;
        save(&next)
    });
    notify();
    *settings = load()?;
    result
}

pub fn restore(settings: &mut Settings) -> Result<()> {
    restore_with(settings, save)
}

fn restore_with(
    settings: &mut Settings,
    persist: impl FnOnce(&Settings) -> Result<()>,
) -> Result<()> {
    let path = home()?.join("settings.json");
    if let Some(record) = Recovery::read(&path)? {
        record.merge(settings);
    }
    registration_scope::validate(&settings.journal, &settings.created_keys)?;
    let result = (|| {
        registry::restore(&mut UserRegistry, &settings.journal)?;
        cleanup_empty_keys(&settings.created_keys)?;
        let mut next = settings.clone();
        next.journal.clear();
        next.created_keys.clear();
        next.active = None;
        next.active_selection = Default::default();
        next.fallbacks.clear();
        next.original_handlers.clear();
        next.icon_exclusions.clear();
        persist(&next)?;
        // 設定保存に失敗しても、再試行用の復元記録を残す順序にする。
        match fs::remove_file(recovery::path(&path)) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        *settings = next;
        Ok(())
    })();
    // 一部だけ復元できた場合や設定保存に失敗した場合も、表示を更新する。
    notify();
    result
}

/// 壊れた設定は別ファイルへ退避し、独立した記録から登録を解除する。
pub fn restore_registration() -> Result<()> {
    match load() {
        Ok(mut settings) => restore(&mut settings),
        Err(error) => {
            let mut recovery = load_recovery()?.context(error)?;
            let backup = settings_file::backup_damaged(&home()?.join("settings.json"))?;
            restore_with(&mut recovery, |settings| backup.save(settings))
        }
    }
}
