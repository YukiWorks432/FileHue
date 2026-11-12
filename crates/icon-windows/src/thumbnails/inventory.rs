//! 関連付けとCurVerをたどり、復元時に再照合する参照先を列挙する。

use super::{
    Dependencies, DependencyInspector, Group, KeyState, ProviderState, Recovery, Reference, Scope,
    THUMBNAIL_IID, decode_provider, plan_extension_group, plan_shared_group, same_registration,
    valid_class, valid_extension,
};
use anyhow::{Context, Result, ensure};
use file_icons_core::registry::{Registry, Value};
use file_icons_thumbnail_core::format::Format;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

mod windows;
pub use windows::WindowsSource;

/// 照会失敗を「関連付けなし」と扱わず、呼び出し元へ返す。
pub trait Source {
    fn extensions(&self) -> Result<Vec<String>>;
    fn association(&self, extension: &str) -> Result<Option<String>>;
    fn curver(&self, class: &str) -> Result<Option<String>>;
    fn read(&self, scope: Scope, key: &str) -> Result<Option<Value>>;
}

pub struct Inspector<S>(pub S);

/// 現在の関連付けを調べ、選択された種類の初回登録計画を作る。
pub fn plan_selection<S: Source>(
    registry: &(impl Registry + KeyState),
    inspector: &Inspector<S>,
    selected: &BTreeSet<String>,
    module: &Path,
) -> Result<Recovery>
where
    Inspector<S>: ProviderState,
{
    ensure!(
        !selected.is_empty()
            && selected.iter().all(|extension| Format::ALL
                .iter()
                .any(|format| format.extension() == extension)),
        "invalid thumbnail selection"
    );
    let mut direct = Vec::new();
    let mut shared: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for format in Format::ALL
        .into_iter()
        .filter(|format| selected.contains(format.extension()))
    {
        let effective = inspector.provider(format.extension())?;
        match classify(&inspector.0, format.extension(), effective)? {
            Binding::Extension => direct.push(format),
            Binding::SharedUser { class } => {
                shared
                    .entry(class.to_lowercase())
                    .or_insert_with(|| (class, BTreeSet::new()))
                    .1
                    .insert(format.extension().into());
            }
            Binding::SharedMachine { class } => anyhow::bail!(
                "machine shared thumbnail provider requires separate validation: {class}"
            ),
        }
    }
    let mut groups = Vec::new();
    for format in direct {
        let mut group = plan_extension_group(registry, format)?;
        group.add_provider(registry, format, module)?;
        group.capture_dependencies(inspector)?;
        group.capture_providers(inspector)?;
        group.capture_created_keys(registry)?;
        groups.push(group);
    }
    for (_, (class, selection)) in shared {
        let first = selection
            .iter()
            .next()
            .context("empty shared thumbnail selection")?
            .clone();
        let provisional = plan_shared_group(
            registry,
            &class,
            [first.clone()].into_iter().collect(),
            &[first].into_iter().collect(),
        )?;
        let discovered = inspector.inspect(&provisional)?;
        let members: BTreeSet<_> = discovered.associations.keys().cloned().collect();
        ensure!(
            selection.is_subset(&members),
            "shared thumbnail membership changed"
        );
        let mut group = plan_shared_group(registry, &class, members, &selection)?;
        for format in Format::ALL {
            if group.members.contains(format.extension()) {
                group.add_provider(registry, format, module)?;
            }
        }
        group.capture_dependencies(inspector)?;
        group.capture_providers(inspector)?;
        group.capture_created_keys(registry)?;
        groups.push(group);
    }
    let record = Recovery {
        format_version: super::VERSION,
        groups,
        transition: None,
    };
    record.preflight_apply(registry, inspector)?;
    Ok(record)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Binding {
    Extension,
    SharedUser { class: String },
    SharedMachine { class: String },
}

/// Shellの提供元とProgIDの参照先を突き合わせ、変更する登録の候補を分類する。
pub fn classify(
    source: &impl Source,
    extension: &str,
    effective: Option<::windows::core::GUID>,
) -> Result<Binding> {
    ensure!(valid_extension(extension), "invalid thumbnail extension");
    let Some(association) = source.association(extension)? else {
        ensure!(
            effective.is_none(),
            "thumbnail provider has no identifiable association"
        );
        return Ok(Binding::Extension);
    };
    let chain = class_chain(source, association)?;
    let mut matches = Vec::new();
    for class in chain {
        let key = format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}");
        let user = source.read(Scope::User, &key)?;
        let machine = source.read(Scope::Machine, &key)?;
        let candidate = |value: &Option<Value>| -> Result<Option<::windows::core::GUID>> {
            match value {
                None => Ok(None),
                Some(value) if value.kind == 1 && value.bytes == [0, 0] => Ok(None),
                Some(value) => Ok(Some(decode_provider(value)?)),
            }
        };
        let user_provider = candidate(&user)?;
        let machine_provider = candidate(&machine)?;
        if effective.is_some()
            && (user_provider == effective
                || (user_provider.is_none() && machine_provider == effective))
        {
            ensure!(valid_class(&class), "unsupported shared association");
            matches.push(if machine.is_some() {
                Binding::SharedMachine { class }
            } else {
                Binding::SharedUser { class }
            });
        }
    }
    ensure!(
        matches.len() <= 1,
        "ambiguous thumbnail provider registration"
    );
    Ok(matches.pop().unwrap_or(Binding::Extension))
}

fn class_chain(source: &impl Source, association: String) -> Result<Vec<String>> {
    let mut chain = Vec::new();
    let mut next = Some(association);
    while let Some(class) = next {
        ensure!(
            !class.is_empty()
                && class.len() <= 255
                && !class.contains(['/', '\0'])
                && !class.chars().any(char::is_control),
            "invalid association class"
        );
        ensure!(
            chain.len() < 16
                && !chain
                    .iter()
                    .any(|prior: &String| same_registration(prior, &class)),
            "cyclic or too deep CurVer"
        );
        next = source.curver(&class)?;
        chain.push(class);
    }
    Ok(chain)
}

impl<S: Source> DependencyInspector for Inspector<S> {
    fn inspect(&self, group: &Group) -> Result<Dependencies> {
        let members: BTreeSet<_> = group
            .members
            .iter()
            .map(|extension| extension.to_lowercase())
            .collect();
        let extensions = if group.class.is_some() {
            self.0.extensions()?
        } else {
            group.members.iter().cloned().collect()
        };
        ensure!(extensions.len() <= 65536, "too many registered extensions");
        let mut extensions: BTreeSet<_> = extensions
            .into_iter()
            .map(|extension| extension.to_lowercase())
            .collect();
        extensions.extend(members.iter().cloned());
        let mut associations = BTreeMap::new();
        let mut referenced = BTreeSet::new();
        for extension in extensions {
            ensure!(
                extension.starts_with('.')
                    && !extension.contains(['\\', '/', '\0'])
                    && extension.len() <= 255,
                "invalid registered extension"
            );
            let association = self.0.association(&extension)?;
            if association.is_none() && (group.class.is_some() || !members.contains(&extension)) {
                ensure!(
                    !members.contains(&extension),
                    "shared thumbnail association unavailable"
                );
                continue;
            }
            let chain = match &association {
                Some(class) => class_chain(&self.0, class.clone())?,
                None => Vec::new(),
            };
            let relevant = match &group.class {
                Some(target) => chain.iter().any(|class| same_registration(class, target)),
                None => members.contains(&extension),
            };
            if !relevant {
                continue;
            }
            ensure!(
                valid_extension(&extension) && chain.iter().all(|class| valid_class(class)),
                "unsupported shared association"
            );
            // 空文字は有効なProgIDではなく、拡張子に関連付けがない状態を表す。
            associations.insert(extension.clone(), association.unwrap_or_default());
            ensure!(associations.len() <= 512, "too many shared associations");
            for class in chain {
                referenced.insert(format!("Software\\Classes\\{class}\\CurVer").to_lowercase());
                referenced.insert(
                    format!("Software\\Classes\\{class}\\shellex\\{THUMBNAIL_IID}").to_lowercase(),
                );
            }
            referenced.insert(format!("Software\\Classes\\{extension}").to_lowercase());
            referenced.insert(
                format!("Software\\Classes\\{extension}\\shellex\\{THUMBNAIL_IID}").to_lowercase(),
            );
            referenced.insert(format!("Software\\Classes\\SystemFileAssociations\\{extension}\\shellex\\{THUMBNAIL_IID}").to_lowercase());
        }
        let mut references = Vec::new();
        for scope in [Scope::User, Scope::Machine] {
            for key in &referenced {
                if scope == Scope::User
                    && group
                        .changes
                        .iter()
                        .any(|change| change.name.is_empty() && same_registration(&change.key, key))
                {
                    continue;
                }
                references.push(Reference {
                    scope,
                    key: key.clone(),
                    name: String::new(),
                    value: self.0.read(scope, key)?,
                });
            }
        }
        references.sort_by_key(|reference| (reference.scope, reference.key.to_lowercase()));
        Ok(Dependencies {
            associations,
            references,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use file_icons_core::registry::Change;

    #[derive(Default)]
    struct Fake {
        associations: BTreeMap<String, String>,
        curvers: BTreeMap<String, String>,
        values: BTreeMap<(Scope, String), Value>,
        providers: BTreeMap<String, Option<::windows::core::GUID>>,
        failures: BTreeSet<String>,
    }
    impl ProviderState for Inspector<Fake> {
        fn provider(&self, extension: &str) -> Result<Option<::windows::core::GUID>> {
            Ok(*self
                .0
                .providers
                .get(extension)
                .context("provider missing")?)
        }
        fn notify(&self) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct RegistryFake(BTreeMap<(String, String), Value>);
    impl Registry for RegistryFake {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            Ok(self
                .0
                .get(&(key.to_lowercase(), name.to_lowercase()))
                .cloned())
        }
        fn write(&mut self, _: &str, _: &str, _: Option<&Value>) -> Result<()> {
            unreachable!()
        }
    }
    impl KeyState for RegistryFake {
        fn key_exists(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn cleanup_created_keys(&mut self, _: &[String]) -> Result<()> {
            unreachable!()
        }
    }
    impl Source for Fake {
        fn extensions(&self) -> Result<Vec<String>> {
            Ok(self.associations.keys().cloned().collect())
        }
        fn association(&self, extension: &str) -> Result<Option<String>> {
            ensure!(
                !self.failures.contains(extension),
                "association query failed"
            );
            Ok(self.associations.get(extension).cloned())
        }
        fn curver(&self, class: &str) -> Result<Option<String>> {
            Ok(self.curvers.get(&class.to_lowercase()).cloned())
        }
        fn read(&self, scope: Scope, key: &str) -> Result<Option<Value>> {
            Ok(self.values.get(&(scope, key.to_lowercase())).cloned())
        }
    }

    fn group() -> Group {
        Group {
            class: Some("Example.Vector.1".into()),
            members: [".ai".into(), ".ait".into()].into_iter().collect(),
            dependencies: None,
            previous_providers: None,
            changes: vec![Change {
                key: format!("Software\\Classes\\Example.Vector.1\\shellex\\{THUMBNAIL_IID}"),
                name: String::new(),
                before: None,
                after: Value {
                    kind: 1,
                    bytes: vec![0, 0],
                },
            }],
            created_keys: Vec::new(),
            keys_captured: false,
        }
    }

    fn fixture() -> Fake {
        let mut source = Fake {
            associations: [
                (".ai".into(), "Example.Vector".into()),
                (".ait".into(), "Example.Vector.1".into()),
                (".eps".into(), "Other.File".into()),
            ]
            .into_iter()
            .collect(),
            ..Fake::default()
        };
        source
            .curvers
            .insert("example.vector".into(), "Example.Vector.1".into());
        source.values.insert(
            (
                Scope::Machine,
                format!("software\\classes\\example.vector.1\\shellex\\{THUMBNAIL_IID}"),
            ),
            Value {
                kind: 3,
                bytes: vec![1, 7, 255],
            },
        );
        source
    }

    #[test]
    fn discovers_curver_aliases_and_records_both_registry_scopes() {
        let inspector = Inspector(fixture());
        let snapshot = inspector.inspect(&group()).unwrap();
        assert_eq!(
            snapshot.associations,
            [
                (".ai".into(), "Example.Vector".into()),
                (".ait".into(), "Example.Vector.1".into())
            ]
            .into_iter()
            .collect()
        );
        let parent = format!("software\\classes\\example.vector.1\\shellex\\{THUMBNAIL_IID}");
        assert!(
            snapshot
                .references
                .iter()
                .any(|entry| entry.scope == Scope::Machine
                    && entry.key == parent
                    && entry.value.as_ref().unwrap().bytes == [1, 7, 255])
        );
        assert!(
            !snapshot
                .references
                .iter()
                .any(|entry| entry.scope == Scope::User && entry.key == parent)
        );
        assert!(
            snapshot
                .references
                .iter()
                .any(|entry| entry.scope == Scope::User
                    && entry.key == "software\\classes\\example.vector\\curver")
        );
        assert!(
            snapshot
                .references
                .iter()
                .any(|entry| entry.scope == Scope::Machine
                    && entry.key == "software\\classes\\example.vector\\curver")
        );
        super::super::validate_dependencies(&group(), &snapshot).unwrap();
    }

    #[test]
    fn unassociated_extension_is_planned_without_changing_default_app() {
        let mut source = Fake::default();
        source.providers.insert(".psb".into(), None);
        let inspector = Inspector(source);
        let registry = RegistryFake::default();
        let selected = [".psb".into()].into_iter().collect();
        let plan = plan_selection(
            &registry,
            &inspector,
            &selected,
            Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
        )
        .unwrap();
        let group = &plan.groups[0];
        assert!(group.class.is_none());
        assert_eq!(
            group.dependencies.as_ref().unwrap().associations[".psb"],
            ""
        );
        assert!(
            group
                .changes
                .iter()
                .all(|change| !same_registration(&change.key, "Software\\Classes\\.psb"))
        );
        let unrelated = ::windows::core::GUID::from_u128(0x11111111_2222_4333_8444_555555555555);
        assert!(classify(&inspector.0, ".psb", Some(unrelated)).is_err());
        let mut changed = inspector.0;
        changed
            .associations
            .insert(".psb".into(), "Example.Document".into());
        assert_ne!(
            Inspector(changed).inspect(group).unwrap(),
            *group.dependencies.as_ref().unwrap()
        );
    }

    #[test]
    fn discovery_rejects_excluded_members_cycles_and_lookup_failures() {
        let mut source = fixture();
        source
            .associations
            .insert(".exe".into(), "Example.Vector".into());
        assert!(Inspector(source).inspect(&group()).is_err());
        let mut source = fixture();
        source
            .curvers
            .insert("example.vector.1".into(), "Example.Vector".into());
        assert!(Inspector(source).inspect(&group()).is_err());
        let mut source = fixture();
        source.failures.insert(".eps".into());
        assert!(Inspector(source).inspect(&group()).is_err());
    }

    #[test]
    fn classifies_effective_shared_provider_and_rejects_ambiguity() {
        let mut source = Fake::default();
        source
            .associations
            .insert(".svg".into(), "Example.Vector".into());
        source
            .curvers
            .insert("example.vector".into(), "Example.Vector.1".into());
        let key = format!("software\\classes\\example.vector.1\\shellex\\{THUMBNAIL_IID}");
        let value = Value {
            kind: 1,
            bytes: "{11111111-2222-4333-8444-555555555555}"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        let guid = ::windows::core::GUID::from_u128(0x11111111_2222_4333_8444_555555555555);
        source
            .values
            .insert((Scope::User, key.clone()), value.clone());
        assert_eq!(
            classify(&source, ".svg", Some(guid)).unwrap(),
            Binding::SharedUser {
                class: "Example.Vector.1".into()
            }
        );
        assert_eq!(classify(&source, ".svg", None).unwrap(), Binding::Extension);
        source
            .values
            .insert((Scope::Machine, key.clone()), value.clone());
        assert_eq!(
            classify(&source, ".svg", Some(guid)).unwrap(),
            Binding::SharedMachine {
                class: "Example.Vector.1".into()
            }
        );
        source.values.remove(&(Scope::Machine, key));
        let alias = format!("software\\classes\\example.vector\\shellex\\{THUMBNAIL_IID}");
        source.values.insert((Scope::User, alias.clone()), value);
        assert!(classify(&source, ".svg", Some(guid)).is_err());
        source.values.insert(
            (Scope::User, alias),
            Value {
                kind: 3,
                bytes: vec![1],
            },
        );
        assert!(classify(&source, ".svg", Some(guid)).is_err());
    }

    #[test]
    fn selection_plan_groups_shared_types_and_preserves_other_members() {
        let mut source = Fake::default();
        let mut registry = RegistryFake::default();
        let old = ::windows::core::GUID::from_u128(0x11111111_2222_4333_8444_555555555555);
        let value = Value {
            kind: 1,
            bytes: "{11111111-2222-4333-8444-555555555555}"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        let key = format!("software\\classes\\example.vector\\shellex\\{THUMBNAIL_IID}");
        source
            .values
            .insert((Scope::User, key.clone()), value.clone());
        registry.0.insert((key, String::new()), value.clone());
        for extension in [".ai", ".ait", ".eps"] {
            source
                .associations
                .insert(extension.into(), "Example.Vector".into());
            source.providers.insert(extension.into(), Some(old));
        }
        source
            .associations
            .insert(".svg".into(), "Applications\\画像編集.exe".into());
        source
            .associations
            .insert(".unrelated".into(), "Applications\\素材管理.exe".into());
        source.providers.insert(".svg".into(), None);
        let selected = [".ai".into(), ".eps".into(), ".svg".into()]
            .into_iter()
            .collect();
        let plan = plan_selection(
            &registry,
            &Inspector(source),
            &selected,
            Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
        )
        .unwrap();
        assert_eq!(plan.groups.len(), 2);
        let group = plan
            .groups
            .iter()
            .find(|group| group.class.is_some())
            .unwrap();
        assert_eq!(
            group.members,
            [".ai".into(), ".ait".into(), ".eps".into()]
                .into_iter()
                .collect()
        );
        let ait = group
            .changes
            .iter()
            .find(|change| change.key.to_lowercase().contains("\\.ait\\shellex\\"))
            .unwrap();
        assert_eq!(ait.after, value);
        assert!(registry.0.len() == 1);
    }

    #[test]
    fn selection_plan_refuses_unverified_machine_shared_provider() {
        let mut source = Fake::default();
        let registry = RegistryFake::default();
        let guid = ::windows::core::GUID::from_u128(0x11111111_2222_4333_8444_555555555555);
        let value = Value {
            kind: 1,
            bytes: "{11111111-2222-4333-8444-555555555555}"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        source
            .associations
            .insert(".ai".into(), "Example.Vector".into());
        source.providers.insert(".ai".into(), Some(guid));
        source.values.insert(
            (
                Scope::Machine,
                format!("software\\classes\\example.vector\\shellex\\{THUMBNAIL_IID}"),
            ),
            value,
        );
        let selected = [".ai".into()].into_iter().collect();
        assert!(
            plan_selection(
                &registry,
                &Inspector(source),
                &selected,
                Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll")
            )
            .is_err()
        );
        assert!(registry.0.is_empty());
    }
}
