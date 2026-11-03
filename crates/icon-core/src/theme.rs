mod image_limits;
mod json_limits;
mod notices;

use crate::language_catalog::{Catalog, Language};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

const MAX_JSON: u64 = 16 * 1024 * 1024;
const MAX_ASSET: u64 = 32 * 1024 * 1024;
const CONVERSION_REVISION: u32 = 3;
const SIZES: [u32; 7] = [16, 20, 24, 32, 48, 128, 256];

/// パスを調べ直さず、既存の名前・直近の親だけを両判定で共有する。
pub(crate) fn matches_pattern(
    matcher: &globset::GlobMatcher,
    pattern: &str,
    parent: Option<&str>,
    name: &str,
) -> bool {
    if pattern.contains('/')
        && let Some(parent) = parent
    {
        matcher.is_match(format!("{parent}/{name}"))
    } else {
        matcher.is_match(name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeChoice {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedPack {
    pub id: String,
    pub version: String,
    pub label: String,
    pub theme: CompiledTheme,
    /// 素材と一緒に保存した出典・権利表示の所在。
    pub provenance: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ImportWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportWarning {
    pub rule_type: String,
    pub rule: String,
    pub icon_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeRules {
    pub files: BTreeMap<String, PathBuf>,
    pub extensions: BTreeMap<String, PathBuf>,
    pub language_files: BTreeMap<String, PathBuf>,
    pub language_extensions: BTreeMap<String, PathBuf>,
    pub language_patterns: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CompiledTheme {
    #[serde(flatten)]
    rules: ThemeRules,
    #[serde(skip)]
    pattern_cache: OnceLock<Vec<(globset::GlobMatcher, String)>>,
}
impl<'de> Deserialize<'de> for CompiledTheme {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Self::new(ThemeRules::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl CompiledTheme {
    pub fn new(mut rules: ThemeRules) -> Result<Self> {
        for mapping in [
            &mut rules.files,
            &mut rules.extensions,
            &mut rules.language_files,
            &mut rules.language_extensions,
        ] {
            let mut normalized = BTreeMap::new();
            for (name, icon) in std::mem::take(mapping) {
                ensure!(
                    name.split('/').count() <= 2
                        && name
                            .split('/')
                            .all(|part| !part.is_empty() && part != "." && part != "..")
                        && !name.contains(['\\', '\0']),
                    "invalid rule name: {name}"
                );
                let name = name.to_lowercase();
                if let Some(previous) = normalized.insert(name.clone(), icon.clone()) {
                    ensure!(
                        previous == icon,
                        "conflicting case-insensitive rule: {name}"
                    );
                }
            }
            *mapping = normalized;
        }
        let mut patterns = BTreeMap::new();
        for (pattern, icon) in std::mem::take(&mut rules.language_patterns) {
            let pattern = pattern.to_lowercase();
            globset::GlobBuilder::new(&pattern)
                .case_insensitive(true)
                .build()?;
            if let Some(previous) = patterns.insert(pattern.clone(), icon.clone()) {
                ensure!(
                    previous == icon,
                    "conflicting case-insensitive pattern: {pattern}"
                );
            }
        }
        rules.language_patterns = patterns;
        Ok(Self {
            rules,
            pattern_cache: OnceLock::new(),
        })
    }
    pub fn rules(&self) -> &ThemeRules {
        &self.rules
    }
    pub fn replace_rules(&mut self, rules: ThemeRules) -> Result<()> {
        *self = Self::new(rules)?;
        Ok(())
    }

    pub fn needs_unknown_class(&self) -> bool {
        self.rules
            .files
            .keys()
            .chain(self.rules.language_files.keys())
            .any(|name| {
                let name = name.rsplit('/').next().unwrap_or(name);
                Path::new(name).extension().is_none()
            })
            || self
                .rules
                .language_patterns
                .keys()
                .any(|pattern| !crate::selection::protected_path(pattern))
    }

    /// ファイル名優先。VS Code互換の直接の親フォルダー条件を扱う。
    /// 規則も入力もUnicode小文字化し、複合拡張子は長いものを優先する。
    pub fn resolve(&self, path: &str) -> Option<&Path> {
        self.resolve_with_reason(path).map(|(icon, _)| icon)
    }

    /// CLIの診断にも、Explorerと同じ優先順位と存在確認を使う。
    pub fn resolve_with_reason(&self, path: &str) -> Option<(&Path, &'static str)> {
        if crate::selection::protected_path(path) {
            return None;
        }
        let path = path.replace('\\', "/").to_lowercase();
        let mut parts = path.rsplit('/');
        let name = parts.next()?;
        let parent = parts.next().filter(|parent| !parent.is_empty());
        let named = parent
            .and_then(|parent| self.rules.files.get(&format!("{parent}/{name}")))
            .or_else(|| self.rules.files.get(name));
        if let Some(icon) = named {
            return icon.is_file().then_some((icon.as_path(), "file_name"));
        }
        if let Some(parent) = parent {
            for (offset, _) in name.match_indices('.') {
                if let Some(icon) = self
                    .rules
                    .extensions
                    .get(&format!("{parent}/{}", &name[offset + 1..]))
                {
                    return icon.is_file().then_some((icon.as_path(), "file_extension"));
                }
            }
        }
        for (offset, _) in name.match_indices('.') {
            if let Some(icon) = self.rules.extensions.get(&name[offset + 1..]) {
                return icon.is_file().then_some((icon.as_path(), "file_extension"));
            }
        }
        if let Some(icon) = self.rules.language_files.get(name) {
            return icon
                .is_file()
                .then_some((icon.as_path(), "language_file_name"));
        }
        let patterns = self.pattern_cache.get_or_init(|| {
            self.rules
                .language_patterns
                .keys()
                .filter_map(|pattern| {
                    globset::GlobBuilder::new(pattern)
                        .case_insensitive(true)
                        .build()
                        .ok()
                        .map(|glob| (glob.compile_matcher(), pattern.clone()))
                })
                .collect()
        });
        for (matcher, key) in patterns {
            if matches_pattern(matcher, key, parent, name) {
                let icon = self.rules.language_patterns.get(key)?;
                return icon
                    .is_file()
                    .then_some((icon.as_path(), "language_pattern"));
            }
        }
        for (offset, _) in name.match_indices('.') {
            if let Some(icon) = self.rules.language_extensions.get(&name[offset + 1..]) {
                return icon
                    .is_file()
                    .then_some((icon.as_path(), "language_extension"));
            }
        }
        None
    }

    pub fn registration_extensions(&self) -> Vec<String> {
        let mut values = self
            .rules
            .extensions
            .keys()
            .filter_map(|ext| ext.rsplit('/').next()?.rsplit('.').next())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.extend(
            self.rules
                .files
                .keys()
                .filter_map(|name| name.rsplit('/').next()?.rsplit_once('.'))
                .map(|(_, ext)| ext.to_owned()),
        );
        values.extend(
            self.rules
                .language_extensions
                .keys()
                .filter_map(|ext| ext.rsplit('.').next())
                .map(str::to_owned),
        );
        values.extend(
            self.rules
                .language_files
                .keys()
                .filter_map(|name| name.rsplit_once('.'))
                .map(|(_, ext)| ext.to_owned()),
        );
        values.retain(|ext| !crate::selection::protected_path(&format!("sample.{ext}")));
        values.sort();
        values.dedup();
        values
    }
}

#[derive(Deserialize)]
struct Package {
    name: String,
    publisher: Option<String>,
    version: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    contributes: Contributions,
}

#[derive(Deserialize)]
struct Contributions {
    #[serde(rename = "iconThemes", default)]
    icon_themes: Vec<ThemeChoice>,
    #[serde(default)]
    languages: Vec<Language>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Definition {
    #[serde(default)]
    icon_definitions: BTreeMap<String, Icon>,
    #[serde(default)]
    file_names: BTreeMap<String, String>,
    #[serde(default)]
    file_extensions: BTreeMap<String, String>,
    #[serde(default)]
    fonts: Vec<serde_json::Value>,
    #[serde(default)]
    language_ids: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Icon {
    icon_path: Option<PathBuf>,
}

fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("open: {}", path.display()))?
        .take(limit + 1)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= limit,
        "input too large: {}",
        path.display()
    );
    Ok(data)
}

fn json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = bounded(path, MAX_JSON)?;
    decode_json(path, &bytes)
}

fn decode_json<T: serde::de::DeserializeOwned>(path: &Path, bytes: &[u8]) -> Result<T> {
    let text = std::str::from_utf8(bytes)?.trim_start_matches('\u{feff}');
    json_limits::check(text).with_context(|| format!("invalid JSON: {}", path.display()))?;
    json5::from_str(text).with_context(|| format!("invalid JSON: {}", path.display()))
}

fn contained(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("missing: {}", path.display()))?;
    ensure!(
        path.starts_with(root),
        "path escapes extension: {}",
        path.display()
    );
    Ok(path)
}

fn validate_choices(choices: &[ThemeChoice]) -> Result<()> {
    ensure!(!choices.is_empty(), "no file icon themes");
    let mut ids = std::collections::BTreeSet::new();
    for choice in choices {
        ensure!(!choice.id.trim().is_empty(), "empty theme id");
        ensure!(ids.insert(&choice.id), "duplicate theme id: {}", choice.id);
    }
    Ok(())
}

pub fn discover(root: &Path) -> Result<Vec<ThemeChoice>> {
    let package: Package = json(&root.join("package.json"))?;
    validate_choices(&package.contributes.icon_themes)?;
    Ok(package.contributes.icon_themes)
}

/// 展開済みVS Code拡張を取り込む。拡張のプログラムは実行しない。
/// 失敗時は参照可能なパックを返さず、呼び出し側の選択状態も変更しない。
pub fn import(root: &Path, theme_id: &str, cache_root: &Path) -> Result<ImportedPack> {
    import_controlled(root, theme_id, cache_root, None, || Ok(()))
}

/// 取得元の記録を素材と一緒に保持し、変換の合間に中断を確認する。
pub fn import_controlled(
    root: &Path,
    theme_id: &str,
    cache_root: &Path,
    origin: Option<&serde_json::Value>,
    check: impl FnMut() -> Result<()>,
) -> Result<ImportedPack> {
    import_observed(root, theme_id, cache_root, origin, check, |_, _| {})
}

/// 使用する画像定義の総数と、検証・変換を書き終えた件数を通知する。
/// 同じ画像を参照する複数の規則は一件と数え、既存ICOも再検証する。
pub fn import_observed(
    root: &Path,
    theme_id: &str,
    cache_root: &Path,
    origin: Option<&serde_json::Value>,
    mut check: impl FnMut() -> Result<()>,
    mut converted: impl FnMut(usize, usize),
) -> Result<ImportedPack> {
    check()?;
    let root = root.canonicalize()?;
    let package_document: serde_json::Value = json(&root.join("package.json"))?;
    let package: Package = serde_json::from_value(package_document.clone())?;
    validate_choices(&package.contributes.icon_themes)?;
    let choice = package
        .contributes
        .icon_themes
        .iter()
        .find(|choice| choice.id == theme_id)
        .context("unknown theme id")?;
    let theme_path = contained(&root, &root.join(&choice.path))?;
    let bytes = bounded(&theme_path, MAX_JSON)?;
    let mut definition: Definition = decode_json(&theme_path, &bytes)?;
    ensure!(
        definition.fonts.is_empty(),
        "font icons are not supported yet"
    );
    let mut warnings = Vec::new();
    for (rule_type, mapping) in [
        ("fileNames", &mut definition.file_names),
        ("fileExtensions", &mut definition.file_extensions),
        ("languageIds", &mut definition.language_ids),
    ] {
        check()?;
        mapping.retain(|rule, icon_id| {
            if definition.icon_definitions.contains_key(icon_id) {
                true
            } else {
                warnings.push(ImportWarning {
                    rule_type: rule_type.into(),
                    rule: rule.clone(),
                    icon_id: icon_id.clone(),
                });
                false
            }
        });
    }
    let pack_id = format!(
        "{}.{}",
        package.publisher.as_deref().unwrap_or("local"),
        package.name
    );
    let mut languages: Catalog = serde_json::from_str(include_str!("../data/languages.json"))?;
    // 組み込み定義にない、初期要件の言語対応は本製品の規則として補う。
    for (id, extensions) in [
        ("toml", vec![".toml"]),
        ("yaml", vec![".yaml", ".yml"]),
        ("zig", vec![".zig"]),
    ] {
        languages.languages.push(Language {
            id: id.into(),
            extensions: extensions.into_iter().map(str::to_owned).collect(),
            filenames: Vec::new(),
            patterns: Vec::new(),
        });
    }
    languages.languages.extend(package.contributes.languages);
    let mut language_files = BTreeMap::new();
    let mut language_extensions = BTreeMap::new();
    let mut language_patterns: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for language in languages.languages {
        if let Some(icon) = definition.language_ids.get(&language.id) {
            for name in language.filenames {
                language_files.insert(name, icon.clone());
            }
            for ext in language.extensions {
                language_extensions.insert(ext.trim_start_matches('.').into(), icon.clone());
            }
            for pattern in language.patterns {
                globset::GlobBuilder::new(&pattern)
                    .case_insensitive(true)
                    .build()?;
                // 同じ原文キーも失わず、素材の内容を確定してから衝突を判定する。
                language_patterns
                    .entry(pattern.to_lowercase())
                    .or_default()
                    .push(icon.clone());
            }
        }
    }
    // 素材の内容を先に確定する。全素材を同時にメモリーへ保持しない。
    let used = definition
        .file_names
        .values()
        .chain(definition.file_extensions.values())
        .chain(language_files.values())
        .chain(language_extensions.values())
        .chain(language_patterns.values().flatten())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut sources = BTreeMap::new();
    let mut asset_hashes = BTreeMap::new();
    ensure!(used.len() <= 4096, "too many distinct icons");
    for id in used {
        check()?;
        let icon = definition
            .icon_definitions
            .get(&id)
            .context("undefined icon id")?;
        let relative = icon.icon_path.as_ref().context("icon has no image path")?;
        let source = contained(&root, &theme_path.parent().unwrap().join(relative))?;
        let data = bounded(&source, MAX_ASSET)?;
        asset_hashes.insert(id.clone(), hex::encode(Sha256::digest(&data)));
        sources.insert(id, source);
    }
    let licenses = notices::collect(&root, &package_document, &mut check)?;
    let mut provenance_document = serde_json::json!({
        "conversion_revision": CONVERSION_REVISION, "download": origin, "package": package_document, "theme_id": theme_id, "licenses": licenses,
        "source": origin.map(|value| value["url"].clone()).unwrap_or_else(|| serde_json::json!(root)), "assets": asset_hashes,
    });
    if !warnings.is_empty() {
        provenance_document["warnings"] = serde_json::to_value(&warnings)?;
    }
    let provenance_bytes = crate::bounded_json::encode(&provenance_document, MAX_JSON as usize)?;
    ensure!(
        provenance_bytes.len() as u64 <= MAX_JSON,
        "provenance too large"
    );
    // 変換形式の版、定義、補完規則、素材、出典を区切りのある形式で識別する。
    let rules = serde_json::to_vec(&(&language_files, &language_extensions, &language_patterns))?;
    let mut identity = Sha256::new();
    for part in [
        CONVERSION_REVISION.to_le_bytes().as_slice(),
        bytes.as_slice(),
        provenance_bytes.as_slice(),
        rules.as_slice(),
    ] {
        identity.update((part.len() as u64).to_le_bytes());
        identity.update(part);
    }
    let destination = cache_root.join(hex::encode(identity.finalize()));
    fs::create_dir_all(&destination)?;
    crate::storage::verify_directory(cache_root, &destination)?;
    let mut icons: BTreeMap<String, PathBuf> = BTreeMap::new();
    let total = sources.len();
    converted(0, total);
    let mut generated_bytes = 0usize;
    let mut compile = |mapping: BTreeMap<String, String>| -> Result<BTreeMap<String, PathBuf>> {
        let mut output = BTreeMap::new();
        for (name, icon_id) in mapping {
            check()?;
            let segments = name.split('/').collect::<Vec<_>>();
            ensure!(
                segments.len() <= 2
                    && segments
                        .iter()
                        .all(|part| !part.is_empty() && *part != "." && *part != "..")
                    && !name.contains(['\\', '\0']),
                "invalid rule name: {name}"
            );
            let path = if let Some(path) = icons.get(&icon_id) {
                path.clone()
            } else {
                ensure!(icons.len() < 4096, "too many distinct icons");
                let source = &sources[&icon_id];
                let asset_hash = &asset_hashes[&icon_id];
                let path = destination.join(format!("{asset_hash}.ico"));
                if path.try_exists()? {
                    let cached = crate::storage::read_file(&path, MAX_ASSET)?;
                    image_limits::validate_icon(&cached)?;
                }
                let data = bounded(source, MAX_ASSET)?;
                ensure!(
                    hex::encode(Sha256::digest(&data)) == *asset_hash,
                    "icon changed during import: {}",
                    source.display()
                );
                let mut expected = Vec::new();
                convert(source, &data)?.write(&mut expected)?;
                generated_bytes += expected.len();
                ensure!(
                    generated_bytes <= 128 * 1024 * 1024,
                    "generated icons too large"
                );
                if path.try_exists()? {
                    crate::storage::verify_file(&path, &expected)?;
                } else {
                    let mut file = tempfile::NamedTempFile::new_in(&destination)?;
                    file.write_all(&expected)?;
                    file.as_file().sync_all()?;
                    file.persist_noclobber(&path).map_err(|error| error.error)?;
                }
                icons.insert(icon_id.clone(), path.clone());
                converted(icons.len(), total);
                path
            };
            let name = name.to_lowercase();
            if let Some(existing) = output.get(&name) {
                ensure!(
                    existing == &path,
                    "conflicting case-insensitive rule: {name}"
                );
            } else {
                output.insert(name, path);
            }
        }
        Ok(output)
    };
    let mut theme = ThemeRules {
        files: compile(definition.file_names)?,
        extensions: compile(definition.file_extensions)?,
        language_files: compile(language_files)?,
        language_extensions: compile(language_extensions)?,
        ..Default::default()
    };
    // パターンにはワイルドカードが許されるため、画像の変換だけを共通処理へ渡す。
    for (pattern, icons) in language_patterns {
        for icon in icons {
            let converted = compile(BTreeMap::from([("pattern-asset".into(), icon)]))?;
            let path = converted["pattern-asset"].clone();
            if let Some(previous) = theme
                .language_patterns
                .insert(pattern.clone(), path.clone())
            {
                ensure!(
                    previous == path,
                    "conflicting case-insensitive pattern: {pattern}"
                );
            }
        }
    }
    ensure!(
        !theme.files.is_empty()
            || !theme.extensions.is_empty()
            || !theme.language_files.is_empty()
            || !theme.language_extensions.is_empty()
            || !theme.language_patterns.is_empty(),
        "theme has no supported rules"
    );
    let theme = CompiledTheme::new(theme)?;
    check()?;
    let provenance = destination.join("provenance.json");
    let mut record = tempfile::NamedTempFile::new_in(&destination)?;
    record.write_all(&provenance_bytes)?;
    record.as_file().sync_all()?;
    if let Err(error) = record.persist_noclobber(&provenance) {
        if error.error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
        ensure!(
            crate::storage::read_file(&provenance, MAX_JSON)? == provenance_bytes,
            "stored provenance is damaged"
        );
    }
    Ok(ImportedPack {
        id: format!("{pack_id}/{theme_id}"),
        version: package.version,
        label: match package.display_name {
            Some(name) if package.contributes.icon_themes.len() > 1 => {
                format!("{name} · {}", choice.label)
            }
            Some(name) => name,
            None => choice.label.clone(),
        },
        theme,
        provenance,
        warnings,
    })
}

fn convert(source: &Path, data: &[u8]) -> Result<ico::IconDir> {
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    if source
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"))
    {
        return encode_rasters(&image_limits::icon(data)?);
    }
    if source
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        let tree = image_limits::svg(data)?;
        for size in SIZES {
            let mut pixmap =
                resvg::tiny_skia::Pixmap::new(size, size).context("invalid raster size")?;
            let scale = (size as f32 / tree.size().width()).min(size as f32 / tree.size().height());
            let transform = resvg::tiny_skia::Transform::from_translate(
                (size as f32 - tree.size().width() * scale) / 2.,
                (size as f32 - tree.size().height() * scale) / 2.,
            )
            .pre_scale(scale, scale);
            resvg::render(&tree, transform, &mut pixmap.as_mut());
            let rgba = pixmap
                .pixels()
                .iter()
                .flat_map(|pixel| {
                    let p = pixel.demultiply();
                    [p.red(), p.green(), p.blue(), p.alpha()]
                })
                .collect();
            dir.add_entry(ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
                size, size, rgba,
            ))?);
        }
    } else {
        return encode_rasters(&[image_limits::raster(data)?]);
    }
    if dir.entries().is_empty() {
        bail!("no icon images");
    }
    Ok(dir)
}

fn encode_rasters(images: &[image::DynamicImage]) -> Result<ico::IconDir> {
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in SIZES {
        let area =
            |image: &&image::DynamicImage| u64::from(image.width()) * u64::from(image.height());
        let image = images
            .iter()
            .filter(|image| image.width() >= size && image.height() >= size)
            .min_by_key(area)
            .or_else(|| images.iter().max_by_key(area))
            .context("no ICO image")?;
        let rgba = image
            .resize(size, size, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let mut canvas = image::RgbaImage::new(size, size);
        image::imageops::overlay(
            &mut canvas,
            &rgba,
            ((size - rgba.width()) / 2) as i64,
            ((size - rgba.height()) / 2) as i64,
        );
        dir.add_entry(ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
            size,
            size,
            canvas.into_raw(),
        ))?);
    }
    Ok(dir)
}

#[cfg(test)]
mod performance {
    use super::*;

    #[test]
    #[ignore = "just perf-icons <未作成のJSON> から明示実行する"]
    fn ico_performance() {
        // 多サイズの通常入力と、許可上限付近の入力を区別して測る。
        let mut cases = Vec::new();
        for (name, sizes) in [
            ("ico_standard", SIZES.to_vec()),
            ("ico_large", vec![512; 32]),
        ] {
            let mut source = ico::IconDir::new(ico::ResourceType::Icon);
            for size in sizes {
                source.add_entry(
                    ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
                        size,
                        size,
                        vec![255; (size * size * 4) as usize],
                    ))
                    .unwrap(),
                );
            }
            let mut input = Vec::new();
            source.write(&mut input).unwrap();
            cases.push(crate::performance::measure(name, || {
                let converted = convert(Path::new("fixture.ico"), &input).unwrap();
                let mut output = Vec::new();
                converted.write(&mut output).unwrap();
                let digest = Sha256::digest(&output);
                u64::from_le_bytes(digest[..8].try_into().unwrap())
            }));
        }
        let _ = crate::performance::live_bytes();
        crate::performance::write(
            cases,
            serde_json::json!({"output_sizes":SIZES,"large_input":[32,512,512],"allocation_tracking":true}),
        );
    }
}
