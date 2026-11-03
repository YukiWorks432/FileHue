//! テーマに依存しない用途別の分類。適用可否の保存形式とは分離する。
use crate::language_catalog::Catalog;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

pub const CATEGORIES: &[(&str, &str, &str)] = &[
    ("language", "開発言語", "Programming languages"),
    ("web", "Web", "Web"),
    ("data", "データ・設定", "Data and configuration"),
    ("documents", "文書", "Documents"),
    ("build", "ビルド・依存管理", "Build and dependencies"),
    (
        "tools",
        "バージョン管理・開発ツール",
        "Version control and tools",
    ),
    ("media", "画像・音声・動画", "Images, audio and video"),
    (
        "archives",
        "圧縮・アーカイブ",
        "Archives and compressed files",
    ),
    ("other", "その他・未分類", "Other and unclassified"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    category: String,
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    language_ids: Vec<String>,
}

// 複数の言語が異なる用途を示す場合は、推測で一方を選ばず未分類に残す。
type Associations = BTreeMap<String, Option<&'static str>>;

struct Classifier {
    files: Associations,
    extensions: Associations,
    patterns: Vec<(String, globset::GlobMatcher, Option<&'static str>)>,
}

fn associate(map: &mut Associations, name: &str, category: &'static str) {
    map.entry(name.to_lowercase())
        .and_modify(|current| {
            if *current != Some(category) {
                *current = None;
            }
        })
        .or_insert(Some(category));
}

impl Classifier {
    fn bundled() -> Self {
        let rules: Vec<Rule> = serde_json::from_str(include_str!("../data/categories.json"))
            .expect("同梱の分類表が不正です");
        let catalog: Catalog = serde_json::from_str(include_str!("../data/languages.json"))
            .expect("同梱の言語定義が不正です");
        let mut language_categories = BTreeMap::new();
        let mut explicit_files = BTreeMap::new();
        let mut explicit_extensions = BTreeMap::new();
        for rule in rules {
            let category = CATEGORIES
                .iter()
                .find(|(id, _, _)| *id == rule.category)
                .expect("分類表のカテゴリが不明です")
                .0;
            for id in rule.language_ids {
                language_categories.insert(id, category);
            }
            for name in rule.filenames {
                explicit_files.insert(name, Some(category));
            }
            for extension in rule.extensions {
                explicit_extensions.insert(extension, Some(category));
            }
        }
        let mut result = Self {
            files: BTreeMap::new(),
            extensions: BTreeMap::new(),
            patterns: Vec::new(),
        };
        let mut patterns = BTreeMap::new();
        for language in catalog.languages {
            let Some(&category) = language_categories.get(&language.id) else {
                continue;
            };
            for name in language.filenames {
                associate(&mut result.files, &name, category);
            }
            for extension in language.extensions {
                associate(
                    &mut result.extensions,
                    extension.trim_start_matches('.'),
                    category,
                );
            }
            for pattern in language.patterns {
                associate(&mut patterns, &pattern, category);
            }
        }
        result.files.extend(explicit_files);
        result.extensions.extend(explicit_extensions);
        result.patterns = patterns
            .into_iter()
            .map(|(pattern, category)| {
                let matcher = globset::Glob::new(&pattern)
                    .expect("同梱の言語パターンが不正です")
                    .compile_matcher();
                (pattern, matcher, category)
            })
            .collect();
        result
    }

    fn extension(&self, name: &str) -> &'static str {
        std::iter::once(name)
            .chain(
                name.match_indices('.')
                    .map(|(offset, _)| &name[offset + 1..]),
            )
            .find_map(|extension| self.extensions.get(extension))
            .copied()
            .flatten()
            .unwrap_or("other")
    }

    fn classify(&self, kind: &str, name: &str) -> &'static str {
        let name = name.to_lowercase();
        let base = name.rsplit('/').next().unwrap_or(&name);
        match kind {
            "file" => {
                if let Some(category) = self.files.get(base) {
                    return category.unwrap_or("other");
                }
                let mut matched = self
                    .patterns
                    .iter()
                    .filter(|(_, matcher, _)| matcher.is_match(&name) || matcher.is_match(base));
                if let Some((_, _, category)) = matched.next() {
                    return if matched.all(|(_, _, other)| other == category) {
                        category.unwrap_or("other")
                    } else {
                        "other"
                    };
                }
                base.find('.')
                    .map(|offset| self.extension(&base[offset + 1..]))
                    .unwrap_or("other")
            }
            "pattern" => {
                if let Some((_, _, category)) = self
                    .patterns
                    .iter()
                    .find(|(pattern, _, _)| *pattern == name)
                {
                    return category.unwrap_or("other");
                }
                // 任意のパターンの意味は推測せず、単純な拡張子指定だけを補う。
                base.strip_prefix("*.")
                    .filter(|extension| !extension.contains(['*', '?', '[', ']', '{', '}']))
                    .map(|extension| self.extension(extension))
                    .unwrap_or("other")
            }
            _ => self.extension(base),
        }
    }
}

pub(crate) fn category(kind: &str, name: &str) -> &'static str {
    static CLASSIFIER: OnceLock<Classifier> = OnceLock::new();
    CLASSIFIER
        .get_or_init(Classifier::bundled)
        .classify(kind, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn bundled_rules_are_unambiguous_and_cover_catalog_languages() {
        let rules: Vec<Rule> =
            serde_json::from_str(include_str!("../data/categories.json")).unwrap();
        let catalog: Catalog =
            serde_json::from_str(include_str!("../data/languages.json")).unwrap();
        let mut files = BTreeSet::new();
        let mut extensions = BTreeSet::new();
        let mut languages = BTreeSet::new();
        let mut categories = BTreeSet::new();
        for rule in rules {
            assert!(CATEGORIES.iter().any(|(id, _, _)| *id == rule.category));
            assert!(categories.insert(rule.category));
            for (names, seen) in [
                (rule.filenames, &mut files),
                (rule.extensions, &mut extensions),
                (rule.language_ids, &mut languages),
            ] {
                for name in names {
                    assert!(!name.is_empty() && name == name.to_lowercase(), "{name}");
                    assert!(seen.insert(name.clone()), "分類指定の重複: {name}");
                }
            }
        }
        for language in catalog.languages {
            assert!(
                languages.contains(&language.id),
                "用途の確認が必要な言語: {}",
                language.id
            );
        }
    }

    #[test]
    fn conflicting_language_hints_remain_unclassified() {
        let mut classifier = Classifier::bundled();
        associate(&mut classifier.extensions, "mixed", "language");
        associate(&mut classifier.extensions, "mixed", "documents");
        associate(&mut classifier.extensions, "mixed", "language");
        assert_eq!(classifier.classify("extension", "mixed"), "other");
    }

    #[test]
    fn longer_extensions_override_general_extensions() {
        let mut classifier = Classifier::bundled();
        classifier
            .extensions
            .insert("build.json".into(), Some("build"));
        assert_eq!(classifier.classify("file", "sample.build.json"), "build");
        assert_eq!(classifier.classify("extension", "src/build.json"), "build");
        assert_eq!(classifier.classify("file", "sample.json"), "data");
    }
}
