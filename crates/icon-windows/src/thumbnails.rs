//! アイコン設定とは独立したサムネイルの選択と復元記録。

use anyhow::{Context, Result, ensure};
use file_icons_core::registry::{Change, Registry, Value};
pub use file_icons_thumbnail_core::format::Format;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
};

const VERSION: u32 = 1;
const LIMIT: u64 = 16 * 1024 * 1024;
const THUMBNAIL_IID: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

pub mod inventory;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub format_version: u32,
    pub selected: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format_version: VERSION,
            selected: BTreeSet::new(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format_version == VERSION,
            "unsupported thumbnail settings version"
        );
        ensure!(
            self.selected.iter().all(|extension| Format::ALL
                .iter()
                .any(|format| format.extension() == extension)),
            "unsupported thumbnail extension"
        );
        Ok(())
    }

    /// 未作成の場合だけ未設定として扱う。
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let Some(bytes) = read(path)? else {
            return Ok(None);
        };
        let settings: Self =
            serde_json::from_slice(&bytes).context("cannot read thumbnail settings")?;
        settings.validate()?;
        Ok(Some(settings))
    }

    /// 呼び出し元の管理ロック内で、読み込んだ設定だけを更新する。
    pub fn save(&self, path: &Path, expected: Option<&Self>) -> Result<()> {
        self.validate()?;
        ensure!(
            Self::load(path)?.as_ref() == expected,
            "thumbnail settings changed"
        );
        let bytes = serde_json::to_vec_pretty(self)?;
        ensure!(bytes.len() as u64 <= LIMIT, "thumbnail settings too large");
        let parent = path.parent().context("thumbnail settings parent missing")?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        ensure!(
            Self::load(path)?.as_ref() == expected,
            "thumbnail settings changed"
        );
        if expected.is_some() {
            file.persist(path)?;
        } else {
            file.persist_noclobber(path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// 上位の共有登録。拡張子だけを扱う場合は存在しない。
    pub class: Option<String>,
    pub members: BTreeSet<String>,
    /// 操作しない関連付けと参照値。登録の直前・復元前に再照合する。
    pub dependencies: Option<Dependencies>,
    /// 登録前にShellが返した提供元。プレビューがなければ `None`。
    pub previous_providers: Option<BTreeMap<String, Option<String>>>,
    /// 拡張子側を先、共有登録を最後に記録する。
    pub changes: Vec<Change>,
    pub created_keys: Vec<String>,
    pub keys_captured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    /// 共有クラスを利用する全拡張子と、その時点の有効なProgID。
    /// 拡張子と有効なProgID。拡張子単独の未関連付けは空文字として記録する。
    pub associations: BTreeMap<String, String>,
    /// 自分では変更しない、関連付けの解決に必要なレジストリ値。
    pub references: Vec<Reference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Scope {
    User,
    Machine,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub scope: Scope,
    pub key: String,
    pub name: String,
    pub value: Option<Value>,
}

/// 毎回、共有先の列挙から参照値までを同じ規則で再取得する。
pub trait DependencyInspector {
    fn inspect(&self, group: &Group) -> Result<Dependencies>;
}

/// ユーザー側のキーの存在確認。
pub trait KeyState {
    fn key_exists(&self, key: &str) -> Result<bool>;
    fn cleanup_created_keys(&mut self, keys: &[String]) -> Result<()>;
}

impl KeyState for crate::UserRegistry {
    fn key_exists(&self, key: &str) -> Result<bool> {
        crate::key_exists(key)
    }
    fn cleanup_created_keys(&mut self, keys: &[String]) -> Result<()> {
        crate::cleanup_empty_keys(keys)
    }
}

/// Shellの実効提供元と関連付けの変更通知を扱う。
pub trait ProviderState {
    fn provider(&self, extension: &str) -> Result<Option<windows::core::GUID>>;
    fn notify(&self) -> Result<()>;
}

impl Group {
    /// 調査時の参照先を固定する。取得できない組は記録しない。
    pub fn capture_dependencies(&mut self, inspector: &impl DependencyInspector) -> Result<()> {
        ensure!(
            self.dependencies.is_none(),
            "thumbnail dependencies already recorded"
        );
        let dependencies = inspector.inspect(self)?;
        validate_dependencies(self, &dependencies)?;
        self.dependencies = Some(dependencies);
        Ok(())
    }

    /// 登録前にShellが実際に選んでいる提供元を記録する。
    pub fn capture_providers(&mut self, provider: &impl ProviderState) -> Result<()> {
        ensure!(
            self.previous_providers.is_none(),
            "thumbnail providers already recorded"
        );
        let dependencies = self
            .dependencies
            .as_ref()
            .context("thumbnail dependencies missing")?;
        let shared = if let Some(class) = &self.class {
            let key = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
            ensure!(
                !dependencies
                    .references
                    .iter()
                    .any(|reference| reference.scope == Scope::Machine
                        && same_registration(&reference.key, &key)
                        && reference.value.is_some()),
                "machine shared provider requires separate validation"
            );
            let parent = self
                .changes
                .iter()
                .find(|change| same_registration(&change.key, &key) && change.name.is_empty())
                .context("shared thumbnail provider missing")?;
            Some(decode_provider(
                parent
                    .before
                    .as_ref()
                    .context("shared thumbnail provider missing")?,
            )?)
        } else {
            None
        };
        let mut result = BTreeMap::new();
        for extension in &self.members {
            let current = provider.provider(extension)?;
            ensure!(
                !current.is_some_and(|guid| Format::ALL
                    .iter()
                    .any(|format| format.clsid() == guid.to_u128())),
                "existing FileHue thumbnail provider has no recovery record"
            );
            if let Some(shared) = shared {
                ensure!(
                    current == Some(shared),
                    "shared thumbnail provider is not effective for all members"
                );
            }
            result.insert(
                extension.to_lowercase(),
                current.map(|guid| format!("{{{guid:?}}}")),
            );
        }
        self.previous_providers = Some(result);
        Ok(())
    }

    /// 存在しなかったキーを、書き込み前に記録する。
    pub fn capture_created_keys(&mut self, registry: &impl KeyState) -> Result<()> {
        ensure!(
            !self.keys_captured && self.created_keys.is_empty(),
            "thumbnail keys already recorded"
        );
        Recovery {
            format_version: VERSION,
            transition: None,
            groups: vec![self.clone()],
        }
        .validate()?;
        let mut missing = BTreeMap::new();
        for change in &self.changes {
            let parts: Vec<_> = change.key.split('\\').collect();
            for end in 3..=parts.len() {
                let key = parts[..end].join("\\");
                if [
                    "software\\classes\\clsid",
                    "software\\classes\\applications",
                    "software\\classes\\systemfileassociations",
                ]
                .contains(&key.to_lowercase().as_str())
                {
                    continue;
                }
                if !registry.key_exists(&key)? {
                    missing.insert(key.to_lowercase(), key);
                }
            }
        }
        let mut captured = self.clone();
        captured.created_keys = missing.into_values().collect();
        captured.keys_captured = true;
        Recovery {
            format_version: VERSION,
            transition: None,
            groups: vec![captured.clone()],
        }
        .validate()?;
        *self = captured;
        Ok(())
    }

    /// 対象拡張子より先に、配置済みDLLのCOM登録を準備する。
    pub fn add_provider(
        &mut self,
        registry: &impl Registry,
        format: Format,
        installed: &Path,
    ) -> Result<()> {
        ensure!(!self.keys_captured, "thumbnail keys already recorded");
        ensure!(installed.is_absolute() && installed.file_name().is_some_and(|name| name.eq_ignore_ascii_case("file_icons_thumbnail_shell.dll")), "invalid thumbnail module path");
        let path = installed
            .to_str()
            .context("invalid thumbnail module path")?;
        ensure!(!path.contains('\0'), "invalid thumbnail module path");
        let member = format!(
            "Software\\Classes\\{}\\shellex\\{THUMBNAIL_IID}",
            format.extension()
        );
        ensure!(
            self.changes
                .iter()
                .any(|change| same_registration(&change.key, &member)
                    && (self.class.is_some() || change.after == provider_value(format))),
            "thumbnail format is not a member of this group"
        );
        let guid = windows::core::GUID::from_u128(format.clsid());
        let key = format!("Software\\Classes\\CLSID\\{{{guid:?}}}\\InprocServer32");
        ensure!(
            !self
                .changes
                .iter()
                .any(|change| same_registration(&change.key, &key)),
            "thumbnail provider already planned"
        );
        let encode = |text: &str| Value {
            kind: 1,
            bytes: text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        let changes = [
            Change {
                before: registry.read(&key, "")?,
                key: key.clone(),
                name: String::new(),
                after: encode(path),
            },
            Change {
                before: registry.read(&key, "ThreadingModel")?,
                key,
                name: "ThreadingModel".into(),
                after: encode("Apartment"),
            },
        ];
        self.changes.splice(0..0, changes);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub format_version: u32,
    pub groups: Vec<Group>,
    /// 共有設定を残したまま、拡張子を一つ切り替える途中状態。
    pub transition: Option<Transition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub group_index: usize,
    pub extension: String,
    pub from: Value,
    pub to: Value,
}

/// 上位の共有設定に妨げられないことを調査した後、拡張子単位の案を作る。
pub fn plan_extension_group(registry: &impl Registry, format: Format) -> Result<Group> {
    let extension = format.extension();
    let key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
    let before = registry.read(&key, "")?;
    Ok(Group {
        class: None,
        members: [extension.into()].into_iter().collect(),
        dependencies: None,
        previous_providers: None,
        changes: vec![Change {
            key,
            name: String::new(),
            before,
            after: provider_value(format),
        }],
        created_keys: Vec::new(),
        keys_captured: false,
    })
}

fn provider_value(format: Format) -> Value {
    let guid = windows::core::GUID::from_u128(format.clsid());
    let bytes = format!("{{{guid:?}}}")
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    Value { kind: 1, bytes }
}

fn parse_provider(text: &str) -> Result<windows::core::GUID> {
    let guid = text
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .context("invalid thumbnail provider")?;
    Ok(windows::core::GUID::try_from(guid)?)
}

fn decode_provider(value: &Value) -> Result<windows::core::GUID> {
    ensure!(
        value.kind == 1 && value.bytes.len() == 78,
        "unsupported thumbnail provider"
    );
    let units: Vec<u16> = value
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    ensure!(units.last() == Some(&0), "invalid thumbnail provider");
    parse_provider(&String::from_utf16(&units[..units.len() - 1])?)
}

/// 関連付けと共有範囲の調査後に、ユーザー側の共有設定を分配する案を作る。
/// この関数は照会だけを行う。関連付け・機械側の参照値・作成予定キーの記録と
/// 変更直前の再照合は呼び出し元の責務であり、この案だけで適用してはならない。
pub fn plan_shared_group(
    registry: &impl Registry,
    class: &str,
    members: BTreeSet<String>,
    selected: &BTreeSet<String>,
) -> Result<Group> {
    ensure!(valid_class(class), "invalid thumbnail group class");
    ensure!(
        !members.is_empty() && members.len() <= 512,
        "invalid thumbnail group"
    );
    ensure!(
        members
            .iter()
            .all(|extension| valid_extension(extension) && *extension == extension.to_lowercase()),
        "invalid thumbnail group extension"
    );
    ensure!(
        !selected.is_empty()
            && selected.iter().all(|extension| members.contains(extension)
                && Format::ALL
                    .iter()
                    .any(|format| format.extension() == extension)),
        "invalid thumbnail selection"
    );
    let parent = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
    let original = registry
        .read(&parent, "")?
        .context("shared thumbnail provider missing")?;
    let guid = decode_provider(&original).context("invalid shared thumbnail provider")?;
    ensure!(guid.to_u128() != 0, "invalid shared thumbnail provider");
    ensure!(
        !Format::ALL
            .iter()
            .any(|format| format.clsid() == guid.to_u128()),
        "shared thumbnail provider already belongs to FileHue"
    );

    let mut changes = Vec::with_capacity(members.len() + 1);
    for extension in &members {
        let key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
        let after = if selected.contains(extension) {
            let format = Format::ALL
                .iter()
                .find(|format| format.extension() == extension)
                .unwrap();
            provider_value(*format)
        } else {
            original.clone()
        };
        changes.push(Change {
            before: registry.read(&key, "")?,
            key,
            name: String::new(),
            after,
        });
    }
    changes.push(Change {
        key: parent,
        name: String::new(),
        before: Some(original),
        after: Value {
            kind: 1,
            bytes: vec![0, 0],
        },
    });
    Ok(Group {
        class: Some(class.into()),
        members,
        dependencies: None,
        previous_providers: None,
        changes,
        created_keys: Vec::new(),
        keys_captured: false,
    })
}

impl Recovery {
    /// 記録済みの登録予定から、FileHueを選んでいる拡張子を求める。
    pub fn selected_extensions(&self) -> Result<BTreeSet<String>> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition before reading selection"
        );
        let mut selected = BTreeSet::new();
        for format in Format::ALL {
            let key = format!(
                "Software\\Classes\\{}\\shellex\\{THUMBNAIL_IID}",
                format.extension()
            );
            if self
                .groups
                .iter()
                .flat_map(|group| &group.changes)
                .any(|change| {
                    change.name.is_empty()
                        && same_registration(&change.key, &key)
                        && change.after == provider_value(format)
                })
            {
                selected.insert(format.extension().into());
            }
        }
        Ok(selected)
    }

    /// 適用中の組が記録どおりに動作しているか、変更せずに照合する。
    pub fn check_applied_group(
        &self,
        index: usize,
        registry: &impl Registry,
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<()> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition first"
        );
        let group = self.groups.get(index).context("thumbnail group missing")?;
        check_dependencies(group, inspector)?;
        for change in &group.changes {
            ensure!(
                registry.read(&change.key, &change.name)?.as_ref() == Some(&change.after),
                "thumbnail group is not fully applied"
            );
        }
        check_target_providers(group, inspector)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format_version == VERSION,
            "unsupported thumbnail recovery version"
        );
        ensure!(
            !self.groups.is_empty() && self.groups.len() <= 512,
            "invalid thumbnail recovery groups"
        );
        let mut seen = BTreeSet::new();
        for group in &self.groups {
            ensure!(
                !group.members.is_empty()
                    && group.members.len() <= 512
                    && !group.changes.is_empty(),
                "invalid thumbnail group"
            );
            let mut members = BTreeSet::new();
            for extension in &group.members {
                ensure!(
                    valid_extension(extension),
                    "invalid thumbnail group extension"
                );
                ensure!(
                    members.insert(extension.to_lowercase()),
                    "duplicate thumbnail group extension"
                );
            }
            if let Some(class) = &group.class {
                ensure!(valid_class(class), "invalid thumbnail group class");
            }
            if let Some(dependencies) = &group.dependencies {
                validate_dependencies(group, dependencies)?;
            }
            if let Some(providers) = &group.previous_providers {
                ensure!(
                    providers.keys().cloned().collect::<BTreeSet<_>>() == members,
                    "invalid thumbnail provider members"
                );
                for value in providers.values().flatten() {
                    parse_provider(value)?;
                }
            }
            let mut parent_seen = false;
            for change in &group.changes {
                let key = change.key.to_lowercase();
                let parent = group.class.as_ref().map(|class| {
                    format!(
                        "software\\classes\\{}\\shellex\\{}",
                        class.to_lowercase(),
                        THUMBNAIL_IID
                    )
                });
                let is_parent = parent.as_ref() == Some(&key);
                let is_member = group.members.iter().any(|extension| {
                    let extension = extension.to_lowercase();
                    key == format!("software\\classes\\{extension}\\shellex\\{THUMBNAIL_IID}")
                        || key == format!("software\\classes\\systemfileassociations\\{extension}\\shellex\\{THUMBNAIL_IID}")
                });
                let is_clsid = Format::ALL.iter().any(|format| {
                    let guid = windows::core::GUID::from_u128(format.clsid());
                    key == format!("software\\classes\\clsid\\{{{guid:?}}}\\inprocserver32")
                        .to_lowercase()
                });
                ensure!(
                    is_parent || is_member || is_clsid,
                    "invalid thumbnail recovery target"
                );
                ensure!(
                    change.name.is_empty() || (is_clsid && change.name == "ThreadingModel"),
                    "invalid thumbnail recovery value"
                );
                ensure!(!parent_seen, "thumbnail parent must follow its members");
                parent_seen = is_parent;
                ensure!(
                    seen.insert((key, change.name.to_lowercase())),
                    "duplicate thumbnail recovery value"
                );
            }
            let mut created = BTreeSet::new();
            for key in &group.created_keys {
                let key = key.to_lowercase();
                ensure!(
                    created.insert(key.clone()),
                    "duplicate thumbnail recovery directory"
                );
                ensure!(
                    key.starts_with("software\\classes\\") && !key.contains(['/', '\0']),
                    "invalid thumbnail recovery directory"
                );
                let member_root = group.members.iter().any(|extension| {
                    let extension = extension.to_lowercase();
                    [
                        format!("software\\classes\\{extension}"),
                        format!("software\\classes\\systemfileassociations\\{extension}"),
                    ]
                    .iter()
                    .any(|root| key == *root || key.starts_with(&format!("{root}\\")))
                });
                let class_root = group.class.as_ref().is_some_and(|class| {
                    let root = format!("software\\classes\\{}", class.to_lowercase());
                    key == root || key.starts_with(&format!("{root}\\"))
                });
                let clsid_root = Format::ALL.iter().any(|format| {
                    let guid = windows::core::GUID::from_u128(format.clsid());
                    let root = format!("software\\classes\\clsid\\{{{guid:?}}}").to_lowercase();
                    key == root || key.starts_with(&format!("{root}\\"))
                });
                ensure!(
                    member_root || class_root || clsid_root,
                    "shared thumbnail directory cannot be removed"
                );
                ensure!(
                    group.changes.iter().any(|change| {
                        let path = change.key.to_lowercase();
                        path == key || path.starts_with(&format!("{key}\\"))
                    }),
                    "unrelated thumbnail recovery directory"
                );
            }
        }
        for group in &self.groups {
            if let Some(dependencies) = &group.dependencies {
                for reference in &dependencies.references {
                    ensure!(
                        reference.scope != Scope::User
                            || !seen.contains(&(
                                reference.key.to_lowercase(),
                                reference.name.to_lowercase()
                            )),
                        "thumbnail reference overlaps another group's change"
                    );
                }
            }
        }
        if let Some(transition) = &self.transition {
            validate_transition(self, transition)?;
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        let Some(bytes) = read(path)? else {
            return Ok(None);
        };
        let recovery: Self =
            serde_json::from_slice(&bytes).context("cannot read thumbnail recovery")?;
        recovery.validate()?;
        Ok(Some(recovery))
    }

    /// 既存の復元記録は、破損していても上書きしない。
    pub fn save_new(&self, path: &Path) -> Result<()> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "new thumbnail recovery cannot contain a transition"
        );
        ensure!(
            self.groups.iter().all(|group| group.dependencies.is_some()
                && group.previous_providers.is_some()
                && group.keys_captured),
            "thumbnail recovery snapshot incomplete"
        );
        let bytes = serde_json::to_vec_pretty(self)?;
        ensure!(bytes.len() as u64 <= LIMIT, "thumbnail recovery too large");
        let parent = path.parent().context("thumbnail recovery parent missing")?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(path)?;
        Ok(())
    }

    /// 管理ロック内で、読み込んだ記録だけを同期して置き換える。
    fn replace(&self, path: &Path, expected: &Self) -> Result<()> {
        self.validate()?;
        let original = read(path)?.context("thumbnail recovery missing")?;
        let stored: Self =
            serde_json::from_slice(&original).context("cannot read thumbnail recovery")?;
        stored.validate()?;
        ensure!(&stored == expected, "thumbnail recovery changed");
        let bytes = serde_json::to_vec_pretty(self)?;
        ensure!(bytes.len() as u64 <= LIMIT, "thumbnail recovery too large");
        let parent = path.parent().context("thumbnail recovery parent missing")?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        ensure!(
            read(path)?.as_ref() == Some(&original),
            "thumbnail recovery changed"
        );
        file.persist(path)?;
        Ok(())
    }

    /// 永続化・書き込み前に、すべての組の依存関係と変更予定値を照合する。
    pub fn preflight_apply(
        &self,
        registry: &(impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<()> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition before applying"
        );
        for group in &self.groups {
            ensure!(group.keys_captured, "thumbnail keys not recorded");
            check_dependencies(group, inspector)?;
            check_previous_providers(group, inspector)?;
            for key in &group.created_keys {
                ensure!(
                    !registry.key_exists(key)?,
                    "thumbnail key appeared before apply"
                );
            }
            for change in &group.changes {
                ensure!(
                    registry.read(&change.key, &change.name)? == change.before,
                    "thumbnail registry changed before apply"
                );
            }
        }
        Ok(())
    }

    /// 先に復元記録を確定し、各値を照合しながら順番に登録する。
    /// 途中で失敗した場合も記録を残し、呼び出し元が復旧できるようにする。
    /// 準備から適用まで、呼び出し元は管理ロックを保持する。
    pub fn apply_new(
        &self,
        path: &Path,
        registry: &mut (impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<()> {
        for group in &self.groups {
            validate_apply_group(group)?;
        }
        self.preflight_apply(registry, inspector)?;
        self.save_new(path)?;
        apply_groups(&self.groups, registry, inspector)
    }

    /// 既存の組を保持し、追加する組の記録を確定してから登録する。
    pub fn append_groups(
        &self,
        path: &Path,
        additional: &Self,
        registry: &mut (impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<Self> {
        self.validate()?;
        ensure!(
            self.transition.is_none() && additional.transition.is_none(),
            "finish thumbnail transition first"
        );
        ensure!(
            Self::load(path)?.as_ref() == Some(self),
            "thumbnail recovery changed"
        );
        for index in 0..self.groups.len() {
            self.check_applied_group(index, registry, inspector)?;
        }
        for group in &additional.groups {
            validate_apply_group(group)?;
        }
        additional.preflight_apply(registry, inspector)?;
        let mut combined = self.clone();
        combined.groups.extend(additional.groups.iter().cloned());
        combined.validate()?;
        combined.replace(path, self)?;
        apply_groups(&additional.groups, registry, inspector)?;
        for index in 0..self.groups.len() {
            combined.check_applied_group(index, registry, inspector)?;
        }
        Ok(combined)
    }

    /// 共有設定を維持したまま一種類を切り替える。管理ロック内で呼び出す。
    pub fn set_shared_member(
        &self,
        path: &Path,
        group_index: usize,
        format: Format,
        enabled: bool,
        registry: &mut (impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<Self> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition first"
        );
        ensure!(
            Self::load(path)?.as_ref() == Some(self),
            "thumbnail recovery changed"
        );
        let group = self
            .groups
            .get(group_index)
            .context("thumbnail group missing")?;
        let class = group
            .class
            .as_ref()
            .context("partial thumbnail transition requires a shared class")?;
        self.check_applied_group(group_index, registry, inspector)?;
        let key = format!(
            "Software\\Classes\\{}\\shellex\\{THUMBNAIL_IID}",
            format.extension()
        );
        let member = group
            .changes
            .iter()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
            .context("thumbnail member registration missing")?;
        let parent_key = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
        let parent = group
            .changes
            .iter()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &parent_key))
            .context("shared thumbnail provider missing")?;
        let to = if enabled {
            provider_value(format)
        } else {
            parent
                .before
                .clone()
                .context("shared thumbnail provider missing")?
        };
        let mut pending = self.clone();
        pending.transition = Some(Transition {
            group_index,
            extension: format.extension().into(),
            from: member.after.clone(),
            to,
        });
        pending.validate()?;
        pending.replace(path, self)?;
        pending.finish_transition(path, registry, inspector)
    }

    /// 中断した切り替えは、値が変更前でも変更後でも同じ記録から完了できる。
    pub fn finish_transition(
        &self,
        path: &Path,
        registry: &mut (impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<Self> {
        self.validate()?;
        ensure!(
            Self::load(path)?.as_ref() == Some(self),
            "thumbnail recovery changed"
        );
        let transition = self
            .transition
            .as_ref()
            .context("thumbnail transition missing")?;
        let group = &self.groups[transition.group_index];
        check_dependencies(group, inspector)?;
        let key = format!(
            "Software\\Classes\\{}\\shellex\\{THUMBNAIL_IID}",
            transition.extension
        );
        for change in &group.changes {
            let current = registry.read(&change.key, &change.name)?;
            if change.name.is_empty() && same_registration(&change.key, &key) {
                ensure!(
                    current.as_ref() == Some(&transition.from)
                        || current.as_ref() == Some(&transition.to),
                    "thumbnail transition changed"
                );
            } else {
                ensure!(
                    current.as_ref() == Some(&change.after),
                    "thumbnail group changed during transition"
                );
            }
        }
        let current = registry.read(&key, "")?;
        if current.as_ref() == Some(&transition.to) {
            inspector
                .notify()
                .context("cannot notify thumbnail transition")?;
        }
        for extension in &group.members {
            let member_key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
            let member = group
                .changes
                .iter()
                .find(|change| {
                    change.name.is_empty() && same_registration(&change.key, &member_key)
                })
                .context("thumbnail member registration missing")?;
            let current = registry
                .read(&member.key, &member.name)?
                .context("thumbnail member value missing")?;
            ensure!(
                inspector.provider(extension)? == Some(decode_provider(&current)?),
                "effective thumbnail provider changed during transition"
            );
        }
        let result = if current.as_ref() == Some(&transition.from) {
            let result = registry.write(&key, "", Some(&transition.to));
            inspector
                .notify()
                .context("cannot notify thumbnail transition")?;
            result
        } else {
            Ok(())
        };
        result?;
        let mut finished = self.clone();
        let group = &mut finished.groups[transition.group_index];
        let member = group
            .changes
            .iter_mut()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
            .context("thumbnail member registration missing")?;
        member.after = transition.to.clone();
        finished.transition = None;
        check_target_providers(&finished.groups[transition.group_index], inspector)?;
        finished.replace(path, self)?;
        Ok(finished)
    }

    /// 参照先と組全体の変更値を照合してから、逆順に復元する。
    /// 実行中に失敗しても、呼び出し元は復元記録を保持する。
    fn restore_group(
        &self,
        index: usize,
        registry: &mut impl Registry,
        inspector: &impl DependencyInspector,
    ) -> Result<()> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition before restoring"
        );
        let group = self.groups.get(index).context("thumbnail group missing")?;
        check_restore_dependencies(group, inspector)?;
        file_icons_core::registry::restore_group(registry, &group.changes)
    }

    /// 値と実効提供元を組全体で照合し、復元後に作成した空キーを片付ける。
    /// 呼び出し元は復元の間、管理ロックを保持する。
    pub fn restore_checked_group(
        &self,
        index: usize,
        registry: &mut (impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<()> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition before restoring"
        );
        let group = self.groups.get(index).context("thumbnail group missing")?;
        ensure!(group.keys_captured, "thumbnail keys not recorded");
        check_restore_dependencies(group, inspector)?;
        for change in &group.changes {
            let current = registry.read(&change.key, &change.name)?;
            ensure!(
                current == change.before || current.as_ref() == Some(&change.after),
                "thumbnail registry changed before restore"
            );
        }
        let parent_is_after = if let Some(class) = &group.class {
            let key = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
            let parent = group
                .changes
                .iter()
                .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
                .context("shared thumbnail provider missing")?;
            let current = registry.read(&parent.key, &parent.name)?;
            current != parent.before && current.as_ref() == Some(&parent.after)
        } else {
            false
        };
        let previous = group
            .previous_providers
            .as_ref()
            .context("thumbnail providers missing")?;
        for extension in &group.members {
            let key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
            let change = group
                .changes
                .iter()
                .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
                .context("thumbnail member registration missing")?;
            let current = registry.read(&change.key, &change.name)?;
            let effective_after = if group.class.is_some() {
                parent_is_after
            } else {
                current != change.before && current.as_ref() == Some(&change.after)
            };
            let expected = if effective_after {
                Some(decode_provider(&change.after)?)
            } else {
                previous
                    .get(&extension.to_lowercase())
                    .context("thumbnail provider missing")?
                    .as_deref()
                    .map(parse_provider)
                    .transpose()?
            };
            ensure!(
                inspector.provider(extension)? == expected,
                "effective thumbnail provider changed before restore"
            );
        }
        let result = (|| -> Result<()> {
            self.restore_group(index, registry, inspector)?;
            registry.cleanup_created_keys(&group.created_keys)?;
            Ok(())
        })();
        inspector
            .notify()
            .context("cannot notify thumbnail registration restore")?;
        result?;
        check_previous_providers(group, inspector)
    }

    /// 復元が完了した組だけを記録から外す。最後の組は記録自体を削除する。
    pub fn remove_restored_group(
        &self,
        path: &Path,
        index: usize,
        registry: &mut (impl Registry + KeyState),
        inspector: &(impl DependencyInspector + ProviderState),
    ) -> Result<Option<Self>> {
        self.validate()?;
        ensure!(
            self.transition.is_none(),
            "finish thumbnail transition before removing recovery"
        );
        let original = read(path)?.context("thumbnail recovery missing")?;
        let stored: Self =
            serde_json::from_slice(&original).context("cannot read thumbnail recovery")?;
        stored.validate()?;
        ensure!(&stored == self, "thumbnail recovery changed");
        let group = self.groups.get(index).context("thumbnail group missing")?;
        check_restore_dependencies(group, inspector)?;
        for change in &group.changes {
            ensure!(
                registry.read(&change.key, &change.name)? == change.before,
                "thumbnail group has not been restored"
            );
        }
        check_previous_providers(group, inspector)?;
        self.restore_checked_group(index, registry, inspector)?;
        if self.groups.len() == 1 {
            ensure!(
                read(path)?.as_ref() == Some(&original),
                "thumbnail recovery changed"
            );
            fs::remove_file(path)?;
            Ok(None)
        } else {
            let mut remaining = self.clone();
            remaining.groups.remove(index);
            remaining.replace(path, self)?;
            Ok(Some(remaining))
        }
    }
}

fn apply_groups(
    groups: &[Group],
    registry: &mut impl Registry,
    inspector: &(impl DependencyInspector + ProviderState),
) -> Result<()> {
    let mut attempted = false;
    let result = (|| -> Result<()> {
        for group in groups {
            check_dependencies(group, inspector)?;
            check_previous_providers(group, inspector)?;
            for change in &group.changes {
                ensure!(
                    registry.read(&change.key, &change.name)? == change.before,
                    "thumbnail registry changed during apply"
                );
                attempted = true;
                registry.write(&change.key, &change.name, Some(&change.after))?;
            }
        }
        Ok(())
    })();
    if attempted {
        inspector
            .notify()
            .context("cannot notify thumbnail registration change")?;
    }
    result?;
    for group in groups {
        check_target_providers(group, inspector)?;
    }
    Ok(())
}

fn validate_apply_group(group: &Group) -> Result<()> {
    let previous = group
        .previous_providers
        .as_ref()
        .context("thumbnail providers missing")?;
    for value in previous.values().flatten() {
        let guid = parse_provider(value)?;
        ensure!(
            !Format::ALL
                .iter()
                .any(|format| format.clsid() == guid.to_u128()),
            "existing FileHue thumbnail provider has no recovery record"
        );
    }
    let original = if let Some(class) = &group.class {
        let key = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
        let parent = group
            .changes
            .iter()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
            .context("shared thumbnail provider missing")?;
        ensure!(
            parent.after.kind == 1 && parent.after.bytes == [0, 0],
            "invalid shared thumbnail replacement"
        );
        let before = parent
            .before
            .as_ref()
            .context("shared thumbnail provider missing")?;
        let guid = decode_provider(before)?;
        ensure!(
            !Format::ALL
                .iter()
                .any(|format| format.clsid() == guid.to_u128()),
            "shared thumbnail provider already belongs to FileHue"
        );
        let dependencies = group
            .dependencies
            .as_ref()
            .context("thumbnail dependencies missing")?;
        let machine = dependencies
            .references
            .iter()
            .find(|reference| {
                reference.scope == Scope::Machine
                    && reference.name.is_empty()
                    && same_registration(&reference.key, &key)
            })
            .context("machine shared provider not inspected")?;
        ensure!(
            machine.value.is_none(),
            "machine shared provider requires separate validation"
        );
        let previous = group
            .previous_providers
            .as_ref()
            .context("thumbnail providers missing")?;
        ensure!(
            previous.values().all(|value| value
                .as_deref()
                .and_then(|text| parse_provider(text).ok())
                == Some(guid)),
            "shared thumbnail providers differ"
        );
        Some(before)
    } else {
        ensure!(
            group.members.len() == 1,
            "invalid extension thumbnail group"
        );
        None
    };
    let mut selected = BTreeSet::new();
    for extension in &group.members {
        let key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
        let change = group
            .changes
            .iter()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
            .context("thumbnail member registration missing")?;
        let guid = decode_provider(&change.after)?;
        if let Some(format) = Format::ALL.iter().find(|format| {
            format.extension().eq_ignore_ascii_case(extension) && format.clsid() == guid.to_u128()
        }) {
            selected.insert(*format);
        } else {
            ensure!(
                original.is_some_and(|before| before == &change.after),
                "invalid thumbnail member provider"
            );
        }
    }
    ensure!(!selected.is_empty(), "no selected thumbnail format");
    for format in Format::ALL {
        let guid = windows::core::GUID::from_u128(format.clsid());
        let key = format!("Software\\Classes\\CLSID\\{{{guid:?}}}\\InprocServer32");
        let entries: Vec<_> = group
            .changes
            .iter()
            .filter(|change| same_registration(&change.key, &key))
            .collect();
        if !selected.contains(&format)
            && !(group.class.is_some() && group.members.contains(format.extension()))
        {
            ensure!(
                entries.is_empty(),
                "unselected thumbnail provider registration"
            );
            continue;
        }
        if entries.is_empty() && !selected.contains(&format) {
            continue;
        }
        ensure!(
            entries.len() == 2,
            "thumbnail provider registration incomplete"
        );
        let path = entries
            .iter()
            .find(|change| change.name.is_empty())
            .context("thumbnail module missing")?;
        let threading = entries
            .iter()
            .find(|change| change.name == "ThreadingModel")
            .context("thumbnail threading model missing")?;
        ensure!(
            decode_registry_string(&threading.after)? == "Apartment",
            "invalid thumbnail threading model"
        );
        let module = decode_registry_string(&path.after)?;
        let module = Path::new(&module);
        ensure!(module.is_absolute() && module.file_name().is_some_and(|name| name.eq_ignore_ascii_case("file_icons_thumbnail_shell.dll")), "invalid thumbnail module path");
    }
    Ok(())
}

fn validate_transition(record: &Recovery, transition: &Transition) -> Result<()> {
    let group = record
        .groups
        .get(transition.group_index)
        .context("thumbnail transition group missing")?;
    let class = group
        .class
        .as_ref()
        .context("partial thumbnail transition requires a shared class")?;
    ensure!(
        group.members.contains(&transition.extension)
            && group.keys_captured
            && group.dependencies.is_some()
            && group.previous_providers.is_some(),
        "invalid thumbnail transition member"
    );
    let format = Format::ALL
        .iter()
        .copied()
        .find(|format| format.extension() == transition.extension)
        .context("unsupported thumbnail transition")?;
    let parent_key = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
    let parent = group
        .changes
        .iter()
        .find(|change| change.name.is_empty() && same_registration(&change.key, &parent_key))
        .context("shared thumbnail provider missing")?;
    let original = parent
        .before
        .as_ref()
        .context("shared thumbnail provider missing")?;
    decode_provider(original)?;
    ensure!(
        parent.after.kind == 1 && parent.after.bytes == [0, 0],
        "invalid shared thumbnail replacement"
    );
    let key = format!(
        "Software\\Classes\\{}\\shellex\\{THUMBNAIL_IID}",
        transition.extension
    );
    let member = group
        .changes
        .iter()
        .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
        .context("thumbnail member registration missing")?;
    let owned = provider_value(format);
    ensure!(
        member.after == transition.from
            && transition.from != transition.to
            && ((transition.from == owned && transition.to == *original)
                || (transition.from == *original && transition.to == owned)),
        "invalid thumbnail transition values"
    );
    let mut selected = 0;
    for extension in &group.members {
        let Some(format) = Format::ALL
            .iter()
            .copied()
            .find(|format| format.extension() == extension)
        else {
            continue;
        };
        let key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
        let change = group
            .changes
            .iter()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
            .context("thumbnail member registration missing")?;
        let value = if extension == &transition.extension {
            &transition.to
        } else {
            &change.after
        };
        if value == &provider_value(format) {
            selected += 1;
        }
    }
    ensure!(
        selected > 0,
        "restore shared group when disabling its final format"
    );
    let guid = windows::core::GUID::from_u128(format.clsid());
    let clsid = format!("Software\\Classes\\CLSID\\{{{guid:?}}}\\InprocServer32");
    ensure!(
        group
            .changes
            .iter()
            .filter(|change| same_registration(&change.key, &clsid)
                && (change.name.is_empty() || change.name == "ThreadingModel"))
            .count()
            == 2,
        "thumbnail provider registration missing"
    );
    Ok(())
}

fn decode_registry_string(value: &Value) -> Result<String> {
    ensure!(
        value.kind == 1
            && value.bytes.len() >= 2
            && value.bytes.len() <= 65536
            && value.bytes.len().is_multiple_of(2),
        "invalid registry string"
    );
    let units: Vec<u16> = value
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    ensure!(
        units.last() == Some(&0) && !units[..units.len() - 1].contains(&0),
        "invalid registry string"
    );
    Ok(String::from_utf16(&units[..units.len() - 1])?)
}

fn check_previous_providers(group: &Group, provider: &impl ProviderState) -> Result<()> {
    let expected = group
        .previous_providers
        .as_ref()
        .context("thumbnail providers missing")?;
    for (extension, value) in expected {
        let expected = value.as_deref().map(parse_provider).transpose()?;
        ensure!(
            provider.provider(extension)? == expected,
            "effective thumbnail provider changed"
        );
    }
    Ok(())
}

fn check_target_providers(group: &Group, provider: &impl ProviderState) -> Result<()> {
    for extension in &group.members {
        let key = format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
        let change = group
            .changes
            .iter()
            .find(|change| change.name.is_empty() && same_registration(&change.key, &key))
            .context("thumbnail member registration missing")?;
        let expected = decode_provider(&change.after)?;
        ensure!(
            provider.provider(extension)? == Some(expected),
            "effective thumbnail provider differs from plan"
        );
    }
    Ok(())
}

fn check_dependencies(group: &Group, inspector: &impl DependencyInspector) -> Result<()> {
    let expected = group
        .dependencies
        .as_ref()
        .context("thumbnail dependencies missing")?;
    let current = inspector.inspect(group)?;
    validate_dependencies(group, &current)?;
    ensure!(&current == expected, "thumbnail dependencies changed");
    Ok(())
}

/// 拡張子だけの解除では、同じ実効関連付けを明示する追加値を保持して戻せる。
/// 適用時と共有クラスの解除では、引き続き参照値の完全一致を要求する。
fn check_restore_dependencies(group: &Group, inspector: &impl DependencyInspector) -> Result<()> {
    let expected = group
        .dependencies
        .as_ref()
        .context("thumbnail dependencies missing")?;
    let current = inspector.inspect(group)?;
    validate_dependencies(group, &current)?;
    if &current == expected {
        return Ok(());
    }
    ensure!(
        group.class.is_none()
            && current.associations == expected.associations
            && current.references.len() == expected.references.len(),
        "thumbnail dependencies changed"
    );
    for (before, now) in expected.references.iter().zip(&current.references) {
        if before == now {
            continue;
        }
        ensure!(
            before.scope == Scope::User
                && now.scope == Scope::User
                && before.name.is_empty()
                && now.name.is_empty()
                && same_registration(&before.key, &now.key)
                && before.value.is_none(),
            "thumbnail dependencies changed"
        );
        let extension = group
            .members
            .iter()
            .find(|extension| {
                same_registration(&now.key, &format!("Software\\Classes\\{extension}"))
            })
            .context("thumbnail dependencies changed")?;
        let value = now
            .value
            .as_ref()
            .context("thumbnail dependencies changed")?;
        let class = decode_registry_string(value)?;
        ensure!(
            !class.is_empty() && same_registration(&class, &current.associations[extension]),
            "thumbnail dependencies changed"
        );
    }
    Ok(())
}

fn validate_dependencies(group: &Group, dependencies: &Dependencies) -> Result<()> {
    let members: BTreeSet<_> = group
        .members
        .iter()
        .map(|value| value.to_lowercase())
        .collect();
    let observed: BTreeSet<_> = dependencies.associations.keys().cloned().collect();
    ensure!(
        members == observed
            && dependencies
                .associations
                .values()
                .all(|class| valid_class(class) || (group.class.is_none() && class.is_empty())),
        "invalid thumbnail associations"
    );
    ensure!(
        dependencies.references.len() <= 4096,
        "too many thumbnail references"
    );
    let mut previous = None;
    for reference in &dependencies.references {
        let key = reference.key.to_lowercase();
        ensure!(
            key.starts_with("software\\classes\\")
                && !key.contains(['/', '\0'])
                && reference.name.is_empty(),
            "invalid thumbnail reference"
        );
        ensure!(
            !group
                .changes
                .iter()
                .any(|change| reference.scope == Scope::User
                    && same_registration(&change.key, &reference.key)
                    && change.name.eq_ignore_ascii_case(&reference.name)),
            "thumbnail reference overlaps a changed value"
        );
        let identity = (reference.scope, key);
        ensure!(
            previous.as_ref().is_none_or(|prior| prior < &identity),
            "duplicate or unsorted thumbnail reference"
        );
        previous = Some(identity);
    }
    Ok(())
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= LIMIT, "thumbnail document too large");
    Ok(Some(bytes))
}

fn valid_extension(extension: &str) -> bool {
    extension.len() > 1
        && extension.len() <= 64
        && extension.starts_with('.')
        && !extension.eq_ignore_ascii_case(".exe")
        && !extension.eq_ignore_ascii_case(".lnk")
        && extension[1..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

// 関連付けの識別はアイコン側と同じ小文字化を使い、登録・復元用の綴りは保持する。
fn same_registration(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

fn valid_class(class: &str) -> bool {
    if class.is_empty()
        || class.len() > 255
        || class.starts_with('.')
        || class.contains(['/', '\0'])
        || class.chars().any(char::is_control)
    {
        return false;
    }
    let parts: Vec<&str> = class.split('\\').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return false;
    }
    let simple = parts.len() == 1
        && ![
            "*",
            "unknown",
            "folder",
            "directory",
            "drive",
            "allfilesystemobjects",
            "clsid",
            "systemfileassociations",
            "applications",
            "exefile",
            "lnkfile",
        ]
        .iter()
        .any(|name| class.eq_ignore_ascii_case(name));
    simple || super::application_association(class)
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Default)]
    struct State {
        values: BTreeMap<(String, String), Value>,
        writes: usize,
        fail_at: Option<usize>,
    }
    struct RegistrySim(Rc<RefCell<State>>);
    impl Registry for RegistrySim {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            Ok(self
                .0
                .borrow()
                .values
                .get(&(key.to_lowercase(), name.to_lowercase()))
                .cloned())
        }
        fn write(&mut self, key: &str, name: &str, value: Option<&Value>) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state.writes += 1;
            ensure!(
                state.fail_at != Some(state.writes),
                "simulated write failure"
            );
            let id = (key.to_lowercase(), name.to_lowercase());
            if let Some(value) = value {
                state.values.insert(id, value.clone());
            } else {
                state.values.remove(&id);
            }
            Ok(())
        }
    }
    impl KeyState for RegistrySim {
        fn key_exists(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn cleanup_created_keys(&mut self, _: &[String]) -> Result<()> {
            Ok(())
        }
    }
    struct InspectorSim {
        state: Rc<RefCell<State>>,
        notifications: Cell<usize>,
        reject_applied: Cell<bool>,
    }
    impl DependencyInspector for InspectorSim {
        fn inspect(&self, group: &Group) -> Result<Dependencies> {
            Ok(Dependencies {
                associations: group
                    .members
                    .iter()
                    .map(|extension| (extension.clone(), "Example.Vector".into()))
                    .collect(),
                references: Vec::new(),
            })
        }
    }
    impl ProviderState for InspectorSim {
        fn provider(&self, extension: &str) -> Result<Option<windows::core::GUID>> {
            let key = format!("software\\classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
            let state = self.state.borrow();
            let value = state.values.get(&(key, String::new()));
            if value.is_some() && self.reject_applied.get() {
                return Ok(None);
            }
            value.map(decode_provider).transpose()
        }
        fn notify(&self) -> Result<()> {
            self.notifications.set(self.notifications.get() + 1);
            Ok(())
        }
    }
    fn prepared() -> (Recovery, RegistrySim, InspectorSim) {
        let state = Rc::new(RefCell::new(State::default()));
        let registry = RegistrySim(state.clone());
        let inspector = InspectorSim {
            state,
            notifications: Cell::new(0),
            reject_applied: Cell::new(false),
        };
        let mut group = plan_extension_group(&registry, Format::Svg).unwrap();
        group
            .add_provider(
                &registry,
                Format::Svg,
                Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
            )
            .unwrap();
        group.capture_dependencies(&inspector).unwrap();
        group.capture_providers(&inspector).unwrap();
        group.capture_created_keys(&registry).unwrap();
        (
            Recovery {
                format_version: VERSION,
                transition: None,
                groups: vec![group],
            },
            registry,
            inspector,
        )
    }

    #[test]
    fn direct_restore_preserves_only_a_redundant_new_default_association() {
        struct Referenced<'a> {
            inner: &'a InspectorSim,
            association: RefCell<String>,
        }
        impl DependencyInspector for Referenced<'_> {
            fn inspect(&self, group: &Group) -> Result<Dependencies> {
                let mut dependencies = self.inner.inspect(group)?;
                dependencies
                    .associations
                    .insert(".svg".into(), self.association.borrow().clone());
                for key in [
                    "software\\classes\\.svg",
                    "software\\classes\\example.vector\\curver",
                ] {
                    dependencies.references.push(Reference {
                        scope: Scope::User,
                        key: key.into(),
                        name: String::new(),
                        value: self
                            .inner
                            .state
                            .borrow()
                            .values
                            .get(&(key.into(), String::new()))
                            .cloned(),
                    });
                }
                Ok(dependencies)
            }
        }
        impl ProviderState for Referenced<'_> {
            fn provider(&self, extension: &str) -> Result<Option<windows::core::GUID>> {
                self.inner.provider(extension)
            }
            fn notify(&self) -> Result<()> {
                self.inner.notify()
            }
        }
        for scenario in [
            "same",
            "different",
            "new_default",
            "other_reference",
            "malformed",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("recovery.json");
            let (mut record, mut registry, inner) = prepared();
            let inspector = Referenced {
                inner: &inner,
                association: RefCell::new(
                    if scenario == "new_default" {
                        ""
                    } else {
                        "Example.Vector"
                    }
                    .into(),
                ),
            };
            record.groups[0].dependencies = Some(inspector.inspect(&record.groups[0]).unwrap());
            record.apply_new(&path, &mut registry, &inspector).unwrap();
            let class = if scenario == "different" {
                "Another.Application"
            } else {
                "Example.Vector"
            };
            let added = Value {
                kind: 1,
                bytes: class
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            };
            registry
                .write("Software\\Classes\\.svg", "", Some(&added))
                .unwrap();
            if scenario == "new_default" {
                *inspector.association.borrow_mut() = class.into();
            }
            if scenario == "other_reference" {
                registry
                    .write(
                        "Software\\Classes\\Example.Vector\\CurVer",
                        "",
                        Some(&added),
                    )
                    .unwrap();
            }
            if scenario == "malformed" {
                registry
                    .write(
                        "Software\\Classes\\.svg",
                        "",
                        Some(&Value {
                            kind: 3,
                            bytes: vec![1, 2],
                        }),
                    )
                    .unwrap();
            }
            let before = registry.0.borrow().values.clone();
            let writes = registry.0.borrow().writes;
            let journal = fs::read(&path).unwrap();
            // 適用状態の確認は引き続き厳密で、変更後の新しい適用を許可しない。
            assert!(
                record
                    .check_applied_group(0, &registry, &inspector)
                    .is_err()
            );
            let result = record.restore_checked_group(0, &mut registry, &inspector);
            if scenario == "same" {
                result.unwrap();
                assert!(
                    record
                        .remove_restored_group(&path, 0, &mut registry, &inspector)
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    registry.read("Software\\Classes\\.svg", "").unwrap(),
                    Some(added)
                );
                assert_eq!(inspector.provider(".svg").unwrap(), None);
            } else {
                assert!(result.is_err(), "{scenario}");
                assert_eq!(registry.0.borrow().writes, writes);
                assert_eq!(registry.0.borrow().values, before);
                assert_eq!(fs::read(&path).unwrap(), journal);
            }
        }
    }

    #[test]
    fn failed_write_keeps_durable_record_and_can_be_restored() {
        for failed_write in 1..=3 {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("recovery.json");
            let (record, mut registry, inspector) = prepared();
            registry.0.borrow_mut().fail_at = Some(failed_write);
            assert!(record.apply_new(&path, &mut registry, &inspector).is_err());
            assert_eq!(inspector.notifications.get(), 1);
            assert_eq!(registry.0.borrow().values.len(), failed_write - 1);
            let saved = Recovery::load(&path).unwrap().unwrap();
            registry.0.borrow_mut().fail_at = None;
            saved
                .restore_checked_group(0, &mut registry, &inspector)
                .unwrap();
            assert!(registry.0.borrow().values.is_empty());
            assert!(path.exists());
        }
    }

    #[test]
    fn failed_journal_and_unexpected_effective_provider_do_not_lose_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (record, mut registry, inspector) = prepared();
        fs::write(&path, b"previous record").unwrap();
        assert!(record.apply_new(&path, &mut registry, &inspector).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"previous record");
        assert!(registry.0.borrow().values.is_empty());
        fs::remove_file(&path).unwrap();
        inspector.reject_applied.set(true);
        assert!(record.apply_new(&path, &mut registry, &inspector).is_err());
        assert!(Recovery::load(&path).unwrap().is_some());
        assert!(!registry.0.borrow().values.is_empty());
        let saved = Recovery::load(&path).unwrap().unwrap();
        assert!(
            saved
                .restore_checked_group(0, &mut registry, &inspector)
                .is_err()
        );
        inspector.reject_applied.set(false);
        saved
            .restore_checked_group(0, &mut registry, &inspector)
            .unwrap();
        assert!(registry.0.borrow().values.is_empty());
    }

    #[test]
    fn successful_apply_records_changes_and_checks_effective_provider() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (record, mut registry, inspector) = prepared();
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        assert_eq!(
            record.selected_extensions().unwrap(),
            [".svg".into()].into_iter().collect()
        );
        assert_eq!(inspector.notifications.get(), 1);
        assert_eq!(
            inspector.provider(".svg").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Svg.clsid()))
        );
        assert!(Recovery::load(&path).unwrap().is_some());
    }

    #[test]
    fn appended_group_can_be_restored_after_partial_write_without_touching_existing_group() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (record, mut registry, inspector) = prepared();
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        let mut added = plan_extension_group(&registry, Format::Pdf).unwrap();
        added
            .add_provider(
                &registry,
                Format::Pdf,
                Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
            )
            .unwrap();
        added.capture_dependencies(&inspector).unwrap();
        added.capture_providers(&inspector).unwrap();
        added.capture_created_keys(&registry).unwrap();
        let addition = Recovery {
            format_version: VERSION,
            groups: vec![added],
            transition: None,
        };
        let next_failure = registry.0.borrow().writes + 2;
        registry.0.borrow_mut().fail_at = Some(next_failure);
        assert!(
            record
                .append_groups(&path, &addition, &mut registry, &inspector)
                .is_err()
        );
        let pending = Recovery::load(&path).unwrap().unwrap();
        assert_eq!(pending.groups.len(), 2);
        assert!(
            pending
                .check_applied_group(1, &registry, &inspector)
                .is_err()
        );
        assert_eq!(
            inspector.provider(".svg").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Svg.clsid()))
        );
        registry.0.borrow_mut().fail_at = None;
        pending
            .restore_checked_group(1, &mut registry, &inspector)
            .unwrap();
        let remaining = pending
            .remove_restored_group(&path, 1, &mut registry, &inspector)
            .unwrap()
            .unwrap();
        assert_eq!(
            remaining.selected_extensions().unwrap(),
            [".svg".into()].into_iter().collect()
        );
        assert_eq!(inspector.provider(".pdf").unwrap(), None);
        assert_eq!(
            inspector.provider(".svg").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Svg.clsid()))
        );
    }

    #[test]
    fn appended_group_preserves_both_selected_providers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (record, mut registry, inspector) = prepared();
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        let mut group = plan_extension_group(&registry, Format::Pdf).unwrap();
        group
            .add_provider(
                &registry,
                Format::Pdf,
                Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
            )
            .unwrap();
        group.capture_dependencies(&inspector).unwrap();
        group.capture_providers(&inspector).unwrap();
        group.capture_created_keys(&registry).unwrap();
        let addition = Recovery {
            format_version: VERSION,
            groups: vec![group],
            transition: None,
        };
        let combined = record
            .append_groups(&path, &addition, &mut registry, &inspector)
            .unwrap();
        assert_eq!(
            combined.selected_extensions().unwrap(),
            [".pdf".into(), ".svg".into()].into_iter().collect()
        );
        assert_eq!(Recovery::load(&path).unwrap(), Some(combined));
        assert_eq!(
            inspector.provider(".svg").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Svg.clsid()))
        );
        assert_eq!(
            inspector.provider(".pdf").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Pdf.clsid()))
        );
    }

    #[test]
    fn recovery_is_removed_only_after_values_and_provider_are_restored() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (record, mut registry, inspector) = prepared();
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        assert!(
            record
                .remove_restored_group(&path, 0, &mut registry, &inspector)
                .is_err()
        );
        assert!(path.exists());
        record
            .restore_checked_group(0, &mut registry, &inspector)
            .unwrap();
        assert!(
            record
                .remove_restored_group(&path, 0, &mut registry, &inspector)
                .unwrap()
                .is_none()
        );
        assert!(!path.exists());
    }

    #[test]
    fn malformed_registration_is_rejected_before_journal_or_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (mut record, mut registry, inspector) = prepared();
        record.groups[0]
            .changes
            .retain(|change| change.name != "ThreadingModel");
        assert!(record.apply_new(&path, &mut registry, &inspector).is_err());
        assert!(!path.exists());
        assert!(registry.0.borrow().values.is_empty());
        assert_eq!(inspector.notifications.get(), 0);
    }

    #[test]
    fn newly_created_key_stops_apply_before_recovery_is_saved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let (mut record, mut registry, inspector) = prepared();
        record.groups[0]
            .created_keys
            .push(format!("Software\\Classes\\.svg\\shellex\\{THUMBNAIL_IID}"));
        assert!(record.apply_new(&path, &mut registry, &inspector).is_err());
        assert!(!path.exists());
        assert!(registry.0.borrow().values.is_empty());
    }

    #[test]
    fn interrupted_restore_can_resume_from_each_write_boundary() {
        for offset in 1..=3 {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("recovery.json");
            let (record, mut registry, inspector) = prepared();
            record.apply_new(&path, &mut registry, &inspector).unwrap();
            registry.0.borrow_mut().fail_at = Some(3 + offset);
            assert!(
                record
                    .restore_checked_group(0, &mut registry, &inspector)
                    .is_err()
            );
            assert!(path.exists());
            registry.0.borrow_mut().fail_at = None;
            Recovery::load(&path)
                .unwrap()
                .unwrap()
                .restore_checked_group(0, &mut registry, &inspector)
                .unwrap();
            assert!(registry.0.borrow().values.is_empty());
        }
    }

    #[test]
    fn shared_transaction_preserves_unselected_and_stops_on_conflict() {
        struct SharedInspector(Rc<RefCell<State>>);
        impl DependencyInspector for SharedInspector {
            fn inspect(&self, _: &Group) -> Result<Dependencies> {
                Ok(Dependencies {
                    associations: [".ai", ".ait", ".eps"]
                        .into_iter()
                        .map(|ext| (ext.into(), "Example.Vector".into()))
                        .collect(),
                    references: vec![Reference {
                        scope: Scope::Machine,
                        key: format!("Software\\Classes\\Example.Vector\\shellex\\{THUMBNAIL_IID}"),
                        name: String::new(),
                        value: None,
                    }],
                })
            }
        }
        impl ProviderState for SharedInspector {
            fn provider(&self, extension: &str) -> Result<Option<windows::core::GUID>> {
                let state = self.0.borrow();
                let parent = format!("software\\classes\\example.vector\\shellex\\{THUMBNAIL_IID}");
                let shared = state
                    .values
                    .get(&(parent, String::new()))
                    .context("missing shared provider")?;
                if shared.bytes != [0, 0] {
                    return Ok(Some(decode_provider(shared)?));
                }
                let key = format!("software\\classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
                state
                    .values
                    .get(&(key, String::new()))
                    .map(decode_provider)
                    .transpose()
            }
            fn notify(&self) -> Result<()> {
                Ok(())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let state = Rc::new(RefCell::new(State::default()));
        let mut registry = RegistrySim(state.clone());
        let inspector = SharedInspector(state);
        let parent = format!("Software\\Classes\\Example.Vector\\shellex\\{THUMBNAIL_IID}");
        let ait = format!("Software\\Classes\\.ait\\shellex\\{THUMBNAIL_IID}");
        let old_provider = windows::core::GUID::from_u128(0x11111111_2222_4333_8444_555555555555);
        let original = Value {
            kind: 1,
            bytes: "{11111111-2222-4333-8444-555555555555}"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        let hidden = Value {
            kind: 3,
            bytes: vec![3, 9, 0, 255],
        };
        registry.write(&parent, "", Some(&original)).unwrap();
        registry.write(&ait, "", Some(&hidden)).unwrap();
        let members = [".ai".into(), ".ait".into(), ".eps".into()]
            .into_iter()
            .collect();
        let selected = [".ai".into(), ".eps".into()].into_iter().collect();
        let mut group = plan_shared_group(&registry, "Example.Vector", members, &selected).unwrap();
        let module = Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll");
        group.add_provider(&registry, Format::Ai, module).unwrap();
        group.add_provider(&registry, Format::Eps, module).unwrap();
        group.capture_dependencies(&inspector).unwrap();
        group.capture_providers(&inspector).unwrap();
        group.capture_created_keys(&registry).unwrap();
        let record = Recovery {
            format_version: VERSION,
            transition: None,
            groups: vec![group],
        };
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        assert_eq!(
            inspector.provider(".ai").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Ai.clsid()))
        );
        assert_eq!(inspector.provider(".ait").unwrap(), Some(old_provider));
        assert_eq!(
            inspector.provider(".eps").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Eps.clsid()))
        );
        assert_eq!(
            record.selected_extensions().unwrap(),
            [".ai".into(), ".eps".into()].into_iter().collect()
        );
        let stale_record = record.clone();
        let next_write = registry.0.borrow().writes + 1;
        registry.0.borrow_mut().fail_at = Some(next_write);
        assert!(
            record
                .set_shared_member(&path, 0, Format::Ai, false, &mut registry, &inspector)
                .is_err()
        );
        let pending = Recovery::load(&path).unwrap().unwrap();
        assert!(pending.transition.is_some());
        assert!(pending.selected_extensions().is_err());
        assert_eq!(
            inspector.provider(".ai").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Ai.clsid()))
        );
        assert!(
            pending
                .restore_checked_group(0, &mut registry, &inspector)
                .is_err()
        );
        registry.0.borrow_mut().fail_at = None;
        let record = pending
            .finish_transition(&path, &mut registry, &inspector)
            .unwrap();
        assert!(record.transition.is_none());
        assert_eq!(
            record.selected_extensions().unwrap(),
            [".eps".into()].into_iter().collect()
        );
        assert_eq!(inspector.provider(".ai").unwrap(), Some(old_provider));
        assert_eq!(inspector.provider(".ait").unwrap(), Some(old_provider));
        assert_eq!(
            inspector.provider(".eps").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Eps.clsid()))
        );
        let current_bytes = fs::read(&path).unwrap();
        let current_values = registry.0.borrow().values.clone();
        assert!(
            stale_record
                .set_shared_member(&path, 0, Format::Ai, false, &mut registry, &inspector)
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), current_bytes);
        assert_eq!(registry.0.borrow().values, current_values);
        fs::write(&path, b"damaged recovery").unwrap();
        assert!(
            record
                .set_shared_member(&path, 0, Format::Ai, true, &mut registry, &inspector)
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"damaged recovery");
        assert_eq!(registry.0.borrow().values, current_values);
        fs::write(&path, current_bytes).unwrap();
        assert!(
            record
                .set_shared_member(&path, 0, Format::Eps, false, &mut registry, &inspector)
                .is_err()
        );
        assert_eq!(Recovery::load(&path).unwrap().unwrap(), record);

        // 値を書いた後、記録を確定する前に中断した状態から再開する。
        let ai = format!("Software\\Classes\\.ai\\shellex\\{THUMBNAIL_IID}");
        let mut pending = record.clone();
        pending.transition = Some(Transition {
            group_index: 0,
            extension: ".ai".into(),
            from: original.clone(),
            to: provider_value(Format::Ai),
        });
        pending.replace(&path, &record).unwrap();
        registry
            .write(&ai, "", Some(&provider_value(Format::Ai)))
            .unwrap();
        let record = Recovery::load(&path)
            .unwrap()
            .unwrap()
            .finish_transition(&path, &mut registry, &inspector)
            .unwrap();
        assert!(record.transition.is_none());
        assert_eq!(
            inspector.provider(".ai").unwrap(),
            Some(windows::core::GUID::from_u128(Format::Ai.clsid()))
        );
        let external = Value {
            kind: 3,
            bytes: vec![4, 5, 6],
        };
        registry.write(&ait, "", Some(&external)).unwrap();
        let before = registry.0.borrow().values.clone();
        assert!(
            record
                .restore_checked_group(0, &mut registry, &inspector)
                .is_err()
        );
        assert_eq!(registry.0.borrow().values, before);
        registry.write(&ait, "", Some(&original)).unwrap();
        record
            .restore_checked_group(0, &mut registry, &inspector)
            .unwrap();
        assert_eq!(registry.read(&parent, "").unwrap(), Some(original));
        assert_eq!(registry.read(&ait, "").unwrap(), Some(hidden));
        assert_eq!(inspector.provider(".ai").unwrap(), Some(old_provider));
        assert!(path.exists());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Memory(BTreeMap<(String, String), Value>);
    impl Registry for Memory {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            Ok(self
                .0
                .get(&(key.to_lowercase(), name.to_lowercase()))
                .cloned())
        }
        fn write(&mut self, key: &str, name: &str, value: Option<&Value>) -> Result<()> {
            let key = (key.to_lowercase(), name.to_lowercase());
            if let Some(value) = value {
                self.0.insert(key, value.clone());
            } else {
                self.0.remove(&key);
            }
            Ok(())
        }
    }
    impl KeyState for Memory {
        fn key_exists(&self, _: &str) -> Result<bool> {
            Ok(false)
        }
        fn cleanup_created_keys(&mut self, _: &[String]) -> Result<()> {
            Ok(())
        }
    }

    fn string_value(value: &str) -> Value {
        Value {
            kind: 1,
            bytes: value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }

    #[test]
    fn captures_only_missing_owned_keys_before_registration() {
        struct Keys(BTreeSet<String>);
        impl KeyState for Keys {
            fn key_exists(&self, key: &str) -> Result<bool> {
                Ok(self.0.contains(&key.to_lowercase()))
            }
            fn cleanup_created_keys(&mut self, _: &[String]) -> Result<()> {
                unreachable!()
            }
        }
        let mut group = plan_extension_group(&Memory::default(), Format::Svg).unwrap();
        let clsid_key =
            "Software\\Classes\\CLSID\\{C62D2AC2-0FD5-4EC7-BB81-FD245220DDA3}\\InprocServer32";
        group.changes.insert(
            0,
            Change {
                key: clsid_key.into(),
                name: String::new(),
                before: None,
                after: string_value("C:\\example.dll"),
            },
        );
        let keys = Keys(
            [
                "software\\classes\\.svg".into(),
                "software\\classes\\.svg\\shellex".into(),
            ]
            .into_iter()
            .collect(),
        );
        group.capture_created_keys(&keys).unwrap();
        let found: BTreeSet<_> = group
            .created_keys
            .iter()
            .map(|key| key.to_lowercase())
            .collect();
        assert_eq!(
            found,
            [
                "software\\classes\\.svg\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}".into(),
                "software\\classes\\clsid\\{c62d2ac2-0fd5-4ec7-bb81-fd245220dda3}".into(),
                "software\\classes\\clsid\\{c62d2ac2-0fd5-4ec7-bb81-fd245220dda3}\\inprocserver32"
                    .into(),
            ]
            .into_iter()
            .collect()
        );
        assert!(group.capture_created_keys(&keys).is_err());
    }

    #[test]
    fn provider_registration_precedes_extension_and_restores_existing_values() {
        let mut registry = Memory::default();
        let key =
            "Software\\Classes\\CLSID\\{C62D2AC2-0FD5-4EC7-BB81-FD245220DDA3}\\InprocServer32";
        let original = Value {
            kind: 3,
            bytes: vec![9, 8, 0, 7],
        };
        registry.write(key, "", Some(&original)).unwrap();
        let mut group = plan_extension_group(&registry, Format::Svg).unwrap();
        let path = Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll");
        group.add_provider(&registry, Format::Svg, path).unwrap();
        assert_eq!(group.changes.len(), 3);
        assert_eq!(group.changes[0].key, key);
        assert_eq!(
            group.changes[0].after,
            string_value("C:\\FileHue\\file_icons_thumbnail_shell.dll")
        );
        assert_eq!(group.changes[1].name, "ThreadingModel");
        assert_eq!(group.changes[1].after, string_value("Apartment"));
        assert!(group.changes[2].key.contains("\\.svg\\shellex\\"));
        assert!(group.add_provider(&registry, Format::Svg, path).is_err());
        assert!(group.add_provider(&registry, Format::Pdf, path).is_err());
        file_icons_core::registry::apply(&mut registry, &group.changes, |_| Ok(())).unwrap();
        file_icons_core::registry::restore_group(&mut registry, &group.changes).unwrap();
        assert_eq!(registry.read(key, "").unwrap(), Some(original));
        assert_eq!(registry.read(key, "ThreadingModel").unwrap(), None);
    }

    #[test]
    fn extension_plan_only_changes_requested_thumbnail_and_restores_raw_value() {
        let mut registry = Memory::default();
        let svg = format!("Software\\Classes\\.svg\\shellex\\{THUMBNAIL_IID}");
        let pdf = format!("Software\\Classes\\.pdf\\shellex\\{THUMBNAIL_IID}");
        let prior = Value {
            kind: 3,
            bytes: vec![11, 42, 0, 255],
        };
        let other = string_value("{11111111-2222-3333-4444-555555555555}");
        registry.write(&svg, "", Some(&prior)).unwrap();
        registry.write(&pdf, "", Some(&other)).unwrap();
        let group = plan_extension_group(&registry, Format::Svg).unwrap();
        let record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group],
        };
        record.validate().unwrap();
        let changes = &record.groups[0].changes;
        file_icons_core::registry::apply(&mut registry, changes, |_| Ok(())).unwrap();
        assert_eq!(
            registry.read(&svg, "").unwrap(),
            Some(string_value("{C62D2AC2-0FD5-4EC7-BB81-FD245220DDA3}"))
        );
        assert_eq!(registry.read(&pdf, "").unwrap(), Some(other.clone()));
        file_icons_core::registry::restore_group(&mut registry, changes).unwrap();
        assert_eq!(registry.read(&svg, "").unwrap(), Some(prior));
        assert_eq!(registry.read(&pdf, "").unwrap(), Some(other));
    }

    #[test]
    fn shared_plan_preserves_unselected_provider_and_restores_original_values() {
        let mut registry = Memory::default();
        let parent = format!("Software\\Classes\\Example.Vector\\shellex\\{THUMBNAIL_IID}");
        let key =
            |extension: &str| format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}");
        let original = string_value("{11111111-2222-3333-4444-555555555555}");
        let existing = Value {
            kind: 3,
            bytes: vec![7, 0, 255],
        };
        let icon_key = "Software\\Classes\\Example.Vector\\DefaultIcon";
        let icon = string_value("existing.ico");
        registry.write(&parent, "", Some(&original)).unwrap();
        registry.write(&key(".ait"), "", Some(&existing)).unwrap();
        registry.write(icon_key, "", Some(&icon)).unwrap();
        let members = [".ai".into(), ".ait".into(), ".eps".into()]
            .into_iter()
            .collect();
        let selected = [".ai".into(), ".eps".into()].into_iter().collect();
        let group = plan_shared_group(&registry, "Example.Vector", members, &selected).unwrap();
        let record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group],
        };
        record.validate().unwrap();
        let changes = &record.groups[0].changes;
        assert_eq!(registry.read(&key(".ai"), "").unwrap(), None);
        assert_eq!(registry.read(&parent, "").unwrap(), Some(original.clone()));
        file_icons_core::registry::apply(&mut registry, changes, |_| Ok(())).unwrap();
        assert_eq!(
            registry.read(&key(".ai"), "").unwrap(),
            Some(string_value("{3044D562-79BA-450F-8846-0E3A75F4F27D}"))
        );
        assert_eq!(
            registry.read(&key(".eps"), "").unwrap(),
            Some(string_value("{BD42852C-4CB6-49A7-86EF-613BECFD07AD}"))
        );
        assert_eq!(
            registry.read(&key(".ait"), "").unwrap(),
            Some(original.clone())
        );
        assert_eq!(registry.read(&parent, "").unwrap(), Some(string_value("")));
        assert_eq!(registry.read(icon_key, "").unwrap(), Some(icon.clone()));
        file_icons_core::registry::restore_group(&mut registry, changes).unwrap();
        assert_eq!(registry.read(&key(".ai"), "").unwrap(), None);
        assert_eq!(registry.read(&key(".eps"), "").unwrap(), None);
        assert_eq!(registry.read(&key(".ait"), "").unwrap(), Some(existing));
        assert_eq!(registry.read(&parent, "").unwrap(), Some(original));
        assert_eq!(registry.read(icon_key, "").unwrap(), Some(icon));
    }

    #[test]
    fn shared_plan_rejects_unsafe_members_and_unusable_existing_provider() {
        let mut registry = Memory::default();
        let parent = format!("Software\\Classes\\Example.Vector\\shellex\\{THUMBNAIL_IID}");
        let selected = [".ai".into()].into_iter().collect();
        let members = || [".ai".into(), ".ait".into()].into_iter().collect();
        assert!(plan_shared_group(&registry, "Example.Vector", members(), &selected).is_err());
        for value in [
            string_value(""),
            string_value("{00000000-0000-0000-0000-000000000000}"),
            Value {
                kind: 3,
                bytes: vec![1, 2],
            },
        ] {
            registry.write(&parent, "", Some(&value)).unwrap();
            assert!(plan_shared_group(&registry, "Example.Vector", members(), &selected).is_err());
            assert_eq!(registry.read(&parent, "").unwrap(), Some(value));
        }
        let original = string_value("{11111111-2222-3333-4444-555555555555}");
        registry.write(&parent, "", Some(&original)).unwrap();
        let excluded = [".ai".into(), ".exe".into()].into_iter().collect();
        assert!(plan_shared_group(&registry, "Example.Vector", excluded, &selected).is_err());
        let unrelated = [".psd".into()].into_iter().collect();
        assert!(plan_shared_group(&registry, "Example.Vector", members(), &unrelated).is_err());
        assert_eq!(registry.read(&parent, "").unwrap(), Some(original));
    }

    #[test]
    fn existing_filehue_provider_cannot_be_captured_as_the_restore_target() {
        struct OwnProvider;
        impl ProviderState for OwnProvider {
            fn provider(&self, _: &str) -> Result<Option<windows::core::GUID>> {
                Ok(Some(windows::core::GUID::from_u128(Format::Svg.clsid())))
            }
            fn notify(&self) -> Result<()> {
                Ok(())
            }
        }
        let registry = Memory::default();
        let mut group = plan_extension_group(&registry, Format::Svg).unwrap();
        group.dependencies = Some(Dependencies {
            associations: [(".svg".into(), "Example.Svg".into())]
                .into_iter()
                .collect(),
            references: vec![],
        });
        assert!(group.capture_providers(&OwnProvider).is_err());
        assert!(group.previous_providers.is_none());
    }

    #[test]
    fn shared_plan_records_effective_providers_and_rejects_unsafe_machine_overlay() {
        struct Providers(BTreeMap<String, Option<windows::core::GUID>>);
        impl ProviderState for Providers {
            fn provider(&self, extension: &str) -> Result<Option<windows::core::GUID>> {
                Ok(*self.0.get(extension).context("missing provider")?)
            }
            fn notify(&self) -> Result<()> {
                Ok(())
            }
        }
        let mut registry = Memory::default();
        let parent = format!("Software\\Classes\\Example.Vector\\shellex\\{THUMBNAIL_IID}");
        let old = string_value("{11111111-2222-4333-8444-555555555555}");
        registry.write(&parent, "", Some(&old)).unwrap();
        let members = [".ai".into(), ".ait".into()].into_iter().collect();
        let selected = [".ai".into()].into_iter().collect();
        let mut group = plan_shared_group(&registry, "Example.Vector", members, &selected).unwrap();
        group.dependencies = Some(Dependencies {
            associations: [
                (".ai".into(), "Example.Vector".into()),
                (".ait".into(), "Example.Vector".into()),
            ]
            .into_iter()
            .collect(),
            references: vec![Reference {
                scope: Scope::Machine,
                key: parent,
                name: String::new(),
                value: None,
            }],
        });
        let guid = windows::core::GUID::from_u128(0x11111111_2222_4333_8444_555555555555);
        let expected = Providers(
            [(".ai".into(), Some(guid)), (".ait".into(), Some(guid))]
                .into_iter()
                .collect(),
        );
        let different = Providers(
            [(".ai".into(), Some(guid)), (".ait".into(), None)]
                .into_iter()
                .collect(),
        );
        assert!(group.capture_providers(&different).is_err());
        assert!(group.previous_providers.is_none());
        group.dependencies.as_mut().unwrap().references[0].value = Some(old);
        assert!(group.capture_providers(&expected).is_err());
        group.dependencies.as_mut().unwrap().references[0].value = None;
        group.capture_providers(&expected).unwrap();
        assert_eq!(
            group.previous_providers.as_ref().unwrap()[".ait"].as_deref(),
            Some("{11111111-2222-4333-8444-555555555555}")
        );
    }

    fn group() -> Group {
        Group {
            class: Some("Example.Vector".into()),
            members: [".ai".into(), ".ait".into()].into_iter().collect(),
            dependencies: Some(Dependencies {
                associations: [
                    (".ai".into(), "Example.Vector".into()),
                    (".ait".into(), "Example.Vector".into()),
                ]
                .into_iter()
                .collect(),
                references: vec![Reference {
                    scope: Scope::Machine,
                    key: "Software\\Classes\\Example.Vector\\CurVer".into(),
                    name: String::new(),
                    value: Some(string_value("Example.Vector.1")),
                }],
            }),
            previous_providers: Some(
                [(".ai".into(), None), (".ait".into(), None)]
                    .into_iter()
                    .collect(),
            ),
            changes: vec![
                Change {
                    key: format!("Software\\Classes\\.ait\\shellex\\{THUMBNAIL_IID}"),
                    name: String::new(),
                    before: Some(Value {
                        kind: 3,
                        bytes: vec![0, 255, 42],
                    }),
                    after: Value {
                        kind: 1,
                        bytes: vec![65, 0, 0, 0],
                    },
                },
                Change {
                    key: format!("Software\\Classes\\Example.Vector\\shellex\\{THUMBNAIL_IID}"),
                    name: String::new(),
                    before: Some(Value {
                        kind: 2,
                        bytes: vec![66, 0, 0, 0],
                    }),
                    after: Value {
                        kind: 1,
                        bytes: vec![0, 0],
                    },
                },
            ],
            created_keys: Vec::new(),
            keys_captured: true,
        }
    }

    #[test]
    fn restore_checks_associations_and_unowned_references_before_any_write() {
        struct Inspector(std::cell::RefCell<Dependencies>);
        impl DependencyInspector for Inspector {
            fn inspect(&self, _: &Group) -> Result<Dependencies> {
                Ok(self.0.borrow().clone())
            }
        }
        let mut registry = Memory::default();
        let group = group();
        for change in &group.changes {
            registry
                .write(&change.key, &change.name, Some(&change.after))
                .unwrap();
        }
        let original = group.dependencies.clone().unwrap();
        let inspector = Inspector(std::cell::RefCell::new(original.clone()));
        let record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group],
        };
        let applied = registry.0.clone();

        inspector.0.borrow_mut().references[0].value = Some(string_value("Example.Vector.2"));
        assert!(record.restore_group(0, &mut registry, &inspector).is_err());
        assert_eq!(registry.0, applied);
        *inspector.0.borrow_mut() = original.clone();
        inspector
            .0
            .borrow_mut()
            .associations
            .insert(".eps".into(), "Example.Vector".into());
        assert!(record.restore_group(0, &mut registry, &inspector).is_err());
        assert_eq!(registry.0, applied);

        *inspector.0.borrow_mut() = original;
        record.restore_group(0, &mut registry, &inspector).unwrap();
        for change in &record.groups[0].changes {
            assert_eq!(
                registry.read(&change.key, &change.name).unwrap(),
                change.before
            );
        }
    }

    #[test]
    fn apply_preflight_detects_stale_dependencies_and_registry_values() {
        struct Inspector(std::cell::RefCell<Dependencies>);
        impl DependencyInspector for Inspector {
            fn inspect(&self, _: &Group) -> Result<Dependencies> {
                Ok(self.0.borrow().clone())
            }
        }
        impl ProviderState for Inspector {
            fn provider(&self, _: &str) -> Result<Option<windows::core::GUID>> {
                Ok(None)
            }
            fn notify(&self) -> Result<()> {
                Ok(())
            }
        }
        let mut group = group();
        let expected = group.dependencies.take().unwrap();
        let inspector = Inspector(std::cell::RefCell::new(expected.clone()));
        group.capture_dependencies(&inspector).unwrap();
        assert!(group.capture_dependencies(&inspector).is_err());
        let record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group],
        };
        let mut registry = Memory::default();
        for change in &record.groups[0].changes {
            registry
                .write(&change.key, &change.name, change.before.as_ref())
                .unwrap();
        }
        record.preflight_apply(&registry, &inspector).unwrap();
        inspector
            .0
            .borrow_mut()
            .associations
            .insert(".ai".into(), "Other.Vector".into());
        assert!(record.preflight_apply(&registry, &inspector).is_err());
        *inspector.0.borrow_mut() = expected;
        let first = &record.groups[0].changes[0];
        registry
            .write(&first.key, &first.name, Some(&string_value("third party")))
            .unwrap();
        assert!(record.preflight_apply(&registry, &inspector).is_err());
    }

    #[test]
    fn incomplete_and_self_referential_dependencies_cannot_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.json");
        let mut planned = group();
        planned.dependencies = None;
        let mut record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![planned],
        };
        assert!(record.save_new(&path).is_err());
        assert!(!path.exists());
        record.groups[0] = group();
        record.groups[0].keys_captured = false;
        assert!(record.save_new(&path).is_err());
        assert!(!path.exists());
        record.groups[0] = group();
        let change = &record.groups[0].changes[0];
        let reference = Reference {
            scope: Scope::User,
            key: change.key.clone(),
            name: change.name.clone(),
            value: change.before.clone(),
        };
        record.groups[0]
            .dependencies
            .as_mut()
            .unwrap()
            .references
            .insert(0, reference);
        assert!(record.save_new(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn references_cannot_overlap_another_group_owned_value() {
        let mut first = group();
        let second = plan_extension_group(&Memory::default(), Format::Svg).unwrap();
        first.dependencies.as_mut().unwrap().references.insert(
            0,
            Reference {
                scope: Scope::User,
                key: second.changes[0].key.clone(),
                name: String::new(),
                value: None,
            },
        );
        let record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![first, second],
        };
        assert!(record.validate().is_err());
    }

    #[test]
    fn recovery_roundtrips_original_bytes_and_never_overwrites_existing_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thumbnails.recovery.json");
        let record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group()],
        };
        record.save_new(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let loaded = Recovery::load(&path).unwrap().unwrap();
        assert_eq!(
            loaded.groups[0].changes[0].before.as_ref().unwrap().bytes,
            [0, 255, 42]
        );
        assert!(record.save_new(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::write(&path, b"{damaged").unwrap();
        assert!(Recovery::load(&path).is_err());
        assert!(record.save_new(&path).is_err());
        assert_eq!(fs::read(path).unwrap(), b"{damaged");
    }

    #[test]
    fn rejects_outside_scope_excluded_members_and_wrong_order() {
        let mut record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group()],
        };
        record.validate().unwrap();
        record.groups[0].changes[0].key =
            "Software\\Classes\\Example.Vector\\shell\\open\\command".into();
        assert!(record.validate().is_err());
        record.groups[0] = group();
        record.groups[0].members.insert(".exe".into());
        assert!(record.validate().is_err());
        record.groups[0] = group();
        record.groups[0].changes.reverse();
        assert!(record.validate().is_err());
        record.groups[0] = group();
        record.groups[0].members.insert(".AI".into());
        assert!(record.validate().is_err());
        for class in ["Applications", "exefile", "LnkFile"] {
            record.groups[0] = group();
            record.groups[0].class = Some(class.into());
            record.groups[0].changes[1].key =
                format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
            assert!(record.validate().is_err());
        }
    }

    #[test]
    fn recovery_allows_only_own_thumbnail_clsid_and_nonshared_created_keys() {
        let mut record = Recovery {
            format_version: 1,
            transition: None,
            groups: vec![group()],
        };
        record.groups[0].changes.insert(0, Change {
            key: "Software\\Classes\\CLSID\\{3044D562-79BA-450F-8846-0E3A75F4F27D}\\InprocServer32".into(),
            name: "ThreadingModel".into(), before: None, after: Value { kind: 1, bytes: vec![0, 0] },
        });
        record.groups[0]
            .created_keys
            .push("Software\\Classes\\CLSID\\{3044D562-79BA-450F-8846-0E3A75F4F27D}".into());
        record.validate().unwrap();
        record.groups[0]
            .created_keys
            .push("Software\\Classes\\CLSID".into());
        assert!(record.validate().is_err());
        record.groups[0].created_keys.clear();
        record.groups[0].changes[0].key =
            "Software\\Classes\\CLSID\\{00000000-0000-0000-0000-000000000000}\\InprocServer32"
                .into();
        assert!(record.validate().is_err());
    }

    #[test]
    fn selection_rejects_unknown_extensions_and_broken_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thumbnails.json");
        assert!(Settings::load(&path).unwrap().is_none());
        fs::write(
            &path,
            br#"{ "format_version": 1, "selected": [".ai", ".psd"] }"#,
        )
        .unwrap();
        assert_eq!(Settings::load(&path).unwrap().unwrap().selected.len(), 2);
        fs::write(&path, br#"{ "format_version": 1, "selected": [".exe"] }"#).unwrap();
        assert!(Settings::load(&path).is_err());
        fs::write(&path, br#"{ "format_version": 2, "selected": [] }"#).unwrap();
        assert!(Settings::load(&path).is_err());
    }

    #[test]
    fn settings_save_preserves_conflicting_and_damaged_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thumbnails.json");
        let initial = Settings::default();
        initial.save(&path, None).unwrap();
        let mut changed = initial.clone();
        changed.selected.insert(".psd".into());
        changed.save(&path, Some(&initial)).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), Some(changed.clone()));
        assert!(initial.save(&path, Some(&initial)).is_err());
        assert_eq!(Settings::load(&path).unwrap(), Some(changed));
        fs::write(&path, b"broken").unwrap();
        assert!(initial.save(&path, None).is_err());
        assert_eq!(fs::read(path).unwrap(), b"broken");
    }
}
