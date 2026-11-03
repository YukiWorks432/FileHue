//! パックの元の規則を変更せず、利用者の対象選択を重ねる。
use crate::CompiledTheme;
pub use crate::categories::CATEGORIES;
use crate::categories::category;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

/// Windowsがファイルごとに選ぶアイコンを、テーマの規則より優先して保護する。
pub fn protected_path(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("exe")
    })
}
fn protected_id(id: &str) -> bool {
    id.split_once(':').is_some_and(|(kind, name)| match kind {
        "extension" => name.rsplit(['/', '.']).next().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("exe")
        }),
        "file" | "pattern" => protected_path(name),
        _ => false,
    })
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Selection {
    overrides: BTreeMap<String, bool>,
    #[serde(skip)]
    patterns: OnceLock<Vec<(globset::GlobMatcher, String, bool)>>,
}
impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        self.overrides == other.overrides
    }
}
impl Eq for Selection {}
impl<'de> Deserialize<'de> for Selection {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Data {
            overrides: BTreeMap<String, bool>,
        }
        let data = Data::deserialize(deserializer)?;
        let mut selection = Self::default();
        for (key, value) in data.overrides {
            // 保護指定でも、壊れた識別子やパターンを受け入れてはならない。
            selection
                .set(&key, None)
                .map_err(serde::de::Error::custom)?;
            // 旧版の有効指定を読み込んでも、保護対象を再び有効化しない。
            if protected_id(&key.to_lowercase()) {
                continue;
            }
            selection
                .set(&key, Some(value))
                .map_err(serde::de::Error::custom)?;
        }
        Ok(selection)
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub category: &'static str,
}
/// 入力ごとに正規化し、対象の走査中は文字列を確保しない検索条件。
pub struct TargetQuery {
    text: String,
}
impl TargetQuery {
    pub fn new(query: &str) -> Self {
        Self {
            text: query.to_lowercase(),
        }
    }
    pub fn matches(&self, target: &Target) -> bool {
        target.name.contains(&self.text)
            || (target.kind == "extension"
                && self
                    .text
                    .strip_prefix('.')
                    .is_some_and(|query| target.name.starts_with(query)))
    }
}
impl Target {
    pub fn protected(&self) -> bool {
        protected_id(&self.id)
    }
    pub fn matches(&self, query: &str) -> bool {
        TargetQuery::new(query).matches(self)
    }
}
impl CompiledTheme {
    pub fn targets(&self) -> Vec<Target> {
        let rules = self.rules();
        let mut keys = BTreeSet::new();
        for name in rules.files.keys().chain(rules.language_files.keys()) {
            keys.insert(format!("file:{name}"));
            if let Some((_, extension)) = name.rsplit('/').next().unwrap_or(name).split_once('.')
                && !extension.is_empty()
            {
                keys.insert(format!("extension:{extension}"));
            }
        }
        for extension in rules
            .extensions
            .keys()
            .chain(rules.language_extensions.keys())
        {
            keys.insert(format!("extension:{extension}"));
        }
        for pattern in rules.language_patterns.keys() {
            keys.insert(format!("pattern:{}", pattern.to_lowercase()));
        }
        keys.into_iter()
            .map(|id| {
                let (kind, name) = id.split_once(':').unwrap();
                Target {
                    name: name.into(),
                    kind: match kind {
                        "file" => "file",
                        "pattern" => "pattern",
                        _ => "extension",
                    },
                    category: category(kind, name),
                    id,
                }
            })
            .collect()
    }
}
impl Selection {
    pub fn set(&mut self, id: &str, value: Option<bool>) -> Result<()> {
        let id = id.to_lowercase();
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid target id"))?;
        ensure!(
            !name.is_empty()
                && name.len() <= 1024
                && !name.contains(['\\', '\0'])
                && name
                    .split('/')
                    .all(|p| !p.is_empty() && p != "." && p != "..")
                && (if kind == "pattern" {
                    name.strip_prefix("**/").unwrap_or(name)
                } else {
                    name
                })
                .split('/')
                .count()
                    <= 2,
            "invalid target id"
        );
        ensure!(
            matches!(kind, "extension" | "file" | "pattern"),
            "invalid target kind"
        );
        if kind == "pattern" {
            globset::GlobBuilder::new(name)
                .case_insensitive(true)
                .build()?;
        }
        ensure!(
            value != Some(true) || !protected_id(&id),
            "protected file type"
        );
        if protected_id(&id) {
            self.overrides.remove(&id);
            self.patterns.take();
            return Ok(());
        }
        if let Some(value) = value {
            self.overrides.insert(id, value);
        } else {
            self.overrides.remove(&id);
        }
        self.patterns.take();
        Ok(())
    }
    pub fn explicit(&self, id: &str) -> Option<bool> {
        self.overrides.get(id).copied()
    }
    pub fn enabled(&self, path: &str) -> bool {
        if protected_path(path) {
            return false;
        }
        let path = path.replace('\\', "/").to_lowercase();
        let mut parts = path.rsplit('/');
        let name = parts.next().unwrap_or_default();
        let parent = parts.next();
        if let Some(parent) = parent
            && let Some(value) = self.explicit(&format!("file:{parent}/{name}"))
        {
            return value;
        }
        if let Some(value) = self.explicit(&format!("file:{name}")) {
            return value;
        }
        let patterns = self.patterns.get_or_init(|| {
            self.overrides
                .iter()
                .filter_map(|(id, value)| {
                    id.strip_prefix("pattern:").and_then(|pattern| {
                        globset::GlobBuilder::new(pattern)
                            .case_insensitive(true)
                            .build()
                            .ok()
                            .map(|glob| (glob.compile_matcher(), pattern.to_owned(), *value))
                    })
                })
                .collect()
        });
        for (matcher, pattern, value) in patterns {
            if crate::theme::matches_pattern(matcher, pattern, parent, name) {
                return *value;
            }
        }
        self.extension_enabled(parent, name)
    }
    fn extension_enabled(&self, parent: Option<&str>, name: &str) -> bool {
        if let Some(parent) = parent {
            for (offset, _) in name.match_indices('.') {
                let ext = &name[offset + 1..];
                if let Some(value) = self.explicit(&format!("extension:{parent}/{ext}")) {
                    return value;
                }
            }
        }
        for (offset, _) in name.match_indices('.') {
            let ext = &name[offset + 1..];
            if let Some(value) = self.explicit(&format!("extension:{ext}")) {
                return value;
            }
        }
        true
    }
    pub fn target_enabled(&self, target: &Target) -> bool {
        if target.protected() {
            return false;
        }
        self.explicit(&target.id)
            .unwrap_or_else(|| match target.kind {
                "extension" => {
                    let (parent, extension) = target
                        .name
                        .rsplit_once('/')
                        .map(|(parent, extension)| (Some(parent), extension))
                        .unwrap_or((None, target.name.as_str()));
                    self.extension_enabled(parent, &format!(".{extension}"))
                }
                "file" => self.enabled(&target.name),
                _ => true,
            })
    }
    pub fn any_enabled(&self, theme: &CompiledTheme) -> bool {
        let rules = theme.rules();
        theme
            .targets()
            .iter()
            .filter(|t| {
                t.kind != "extension"
                    || rules.extensions.contains_key(&t.name)
                    || rules.language_extensions.contains_key(&t.name)
            })
            .any(|t| self.target_enabled(t))
    }
    pub fn validate_for(&self, theme: &CompiledTheme) -> Result<()> {
        let supported: BTreeSet<_> = theme
            .targets()
            .into_iter()
            .map(|target| target.id)
            .collect();
        ensure!(
            self.overrides.keys().all(|id| supported.contains(id)),
            "unknown selection target"
        );
        Ok(())
    }
    pub fn retain_supported(&mut self, theme: &CompiledTheme) {
        let targets: BTreeSet<_> = theme
            .targets()
            .into_iter()
            .map(|target| target.id)
            .collect();
        self.overrides.retain(|key, _| targets.contains(key));
        self.patterns.take();
    }
    pub fn copy_to(
        &self,
        source: &CompiledTheme,
        destination: &CompiledTheme,
        destination_selection: &Self,
    ) -> (Self, Vec<String>) {
        let allowed: BTreeSet<_> = destination
            .targets()
            .into_iter()
            .map(|target| target.id)
            .collect();
        let mut result = destination_selection.clone();
        result.overrides.retain(|id, _| allowed.contains(id));
        result.patterns.take();
        let mut unmatched = Vec::new();
        for target in source.targets() {
            if target.protected() {
                result.overrides.remove(&target.id);
            } else if allowed.contains(&target.id) {
                // 両方のSelectionは入力境界で検証済み。対象の列挙・パターン検証を繰り返さない。
                if let Some(value) = self.explicit(&target.id) {
                    result.overrides.insert(target.id, value);
                } else {
                    result.overrides.remove(&target.id);
                }
            } else {
                unmatched.push(target.id);
            }
        }
        (result, unmatched)
    }
    pub fn changes_from(&self, other: &Self) -> usize {
        self.overrides
            .keys()
            .chain(other.overrides.keys())
            .collect::<BTreeSet<_>>()
            .iter()
            .filter(|k| self.overrides.get(**k) != other.overrides.get(**k))
            .count()
    }
    pub fn resolve<'a>(
        &self,
        theme: &'a CompiledTheme,
        path: &str,
    ) -> Option<(&'a std::path::Path, &'static str)> {
        if self.enabled(path) {
            theme.resolve_with_reason(path)
        } else {
            None
        }
    }
}
