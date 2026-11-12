//! GUIとCLIの管理操作。同じ排他を保持したまま保存・登録処理を呼び出す。
use anyhow::{Context, Result, ensure};
pub use file_icons_catalog::Progress;
use file_icons_core::{ImportedPack, registry::Registry};
use file_icons_windows::{Settings, home, load, save};
use std::{collections::BTreeSet, fs, path::Path, sync::Arc};
pub type DownloadSource = fn(&str, &Path, Arc<Progress>) -> Result<Vec<ImportedPack>>;

#[derive(Debug)]
pub enum Failure {
    Busy,
    NotFound,
    Precondition,
    Conflict,
    Recovery,
}

fn restore_thumbnail_records(
    path: &Path,
    mut record: file_icons_windows::thumbnails::Recovery,
    registry: &mut (impl Registry + file_icons_windows::thumbnails::KeyState),
    inspector: &(
         impl file_icons_windows::thumbnails::DependencyInspector
         + file_icons_windows::thumbnails::ProviderState
     ),
    before_final_removal: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut failures = Vec::new();
    if record.transition.is_some() {
        match record.finish_transition(path, registry, inspector) {
            Ok(finished) => record = finished,
            Err(error) => failures.push(format!("transition: {error:#}")),
        }
    }
    let mut index = 0;
    while index < record.groups.len() {
        if record
            .transition
            .as_ref()
            .is_some_and(|pending| pending.group_index == index)
        {
            index += 1;
            continue;
        }
        if let Err(error) = record.restore_checked_group(index, registry, inspector) {
            failures.push(format!("group {index}: {error:#}"));
            index += 1;
            continue;
        }
        if record.groups.len() == 1 {
            before_final_removal()?;
        }
        match record.remove_restored_group(path, index, registry, inspector)? {
            Some(remaining) => record = remaining,
            None => break,
        }
    }
    ensure!(
        failures.is_empty(),
        "thumbnail restore conflicts: {}",
        failures.join("; ")
    );
    Ok(())
}

fn clear_thumbnail_settings(path: &Path) -> Result<()> {
    use file_icons_windows::thumbnails::Settings as ThumbnailSettings;
    let existing = ThumbnailSettings::load(path)?;
    if let Some(settings) = existing
        .as_ref()
        .filter(|settings| !settings.selected.is_empty())
    {
        ThumbnailSettings::default().save(path, Some(settings))?;
    }
    Ok(())
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Failure {}

pub struct Manager {
    _lock: fs::File,
}

#[derive(Debug)]
pub struct ThumbnailStatus {
    pub settings: file_icons_windows::thumbnails::Settings,
    pub recovery: Option<file_icons_windows::thumbnails::Recovery>,
    pub missing_recovery: bool,
    /// 復元記録にある各組を照合した結果。エラーがあれば適用完了とは扱わない。
    pub group_errors: Vec<Option<String>>,
}

/// 適用前に確認する選択差分と、影響する登録の調査結果。
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct ThumbnailSelectionPlan {
    pub selected: BTreeSet<String>,
    pub current: BTreeSet<String>,
    pub additions: BTreeSet<String>,
    pub removals: BTreeSet<String>,
    pub existing_groups: Option<file_icons_windows::thumbnails::Recovery>,
    pub new_groups: Option<file_icons_windows::thumbnails::Recovery>,
    pub recheck_on_execution: bool,
}

impl Manager {
    pub fn acquire() -> Result<Self> {
        let root = home()?;
        fs::create_dir_all(&root)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("manager.lock"))?;
        lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => anyhow::Error::new(Failure::Busy),
            fs::TryLockError::Error(error) => error.into(),
        })?;
        Ok(Self { _lock: lock })
    }
    pub fn settings(&self) -> Result<Settings> {
        ensure!(!file_icons_windows::conflict::pending()?, Failure::Recovery);
        load().context(Failure::Recovery)
    }

    /// アイコン設定とは別に、サムネイルの選択と復元記録を読む。
    pub fn thumbnail_status(&self) -> Result<ThumbnailStatus> {
        use file_icons_windows::thumbnails::inventory::{Inspector, WindowsSource};
        let root = home()?;
        let recovery =
            file_icons_windows::thumbnails::Recovery::load(&root.join("thumbnail-recovery.json"))?;
        let mut group_errors = Vec::new();
        if let Some(record) = &recovery {
            let inspector = Inspector(WindowsSource::new()?);
            let registry = file_icons_windows::UserRegistry;
            for index in 0..record.groups.len() {
                group_errors.push(
                    record
                        .check_applied_group(index, &registry, &inspector)
                        .err()
                        .map(|error| format!("{error:#}")),
                );
            }
        }
        let settings =
            file_icons_windows::thumbnails::Settings::load(&root.join("thumbnail-settings.json"))?
                .unwrap_or_default();
        let missing_recovery = recovery.is_none() && !settings.selected.is_empty();
        Ok(ThumbnailStatus {
            settings,
            recovery,
            missing_recovery,
            group_errors,
        })
    }

    /// 選択設定が破損していても、独立した復元記録を取得する。
    pub fn thumbnail_recovery(&self) -> Result<Option<file_icons_windows::thumbnails::Recovery>> {
        file_icons_windows::thumbnails::Recovery::load(&home()?.join("thumbnail-recovery.json"))
    }

    /// 指定した拡張子の集合へ変更する。途中失敗時は適用できた組の記録を保持する。
    pub fn set_thumbnail_selection(&self, selected: BTreeSet<String>) -> Result<()> {
        let reviewed = self.preview_thumbnail_selection(selected)?;
        self.apply_reviewed_thumbnail_selection(reviewed)
    }

    /// 確認済みの登録先と以前の値をそのまま使用し、適用前に再照合する。
    pub fn apply_reviewed_thumbnail_selection(
        &self,
        reviewed: ThumbnailSelectionPlan,
    ) -> Result<()> {
        use file_icons_windows::thumbnails::{
            Format, Settings as ThumbnailSettings,
            inventory::{Inspector, WindowsSource},
        };
        ensure!(
            self.preview_thumbnail_selection(reviewed.selected.clone())? == reviewed,
            "thumbnail registration changed after review"
        );
        let root = home()?;
        let path = root.join("thumbnail-recovery.json");
        let settings_path = root.join("thumbnail-settings.json");
        if let Some(additional) = &reviewed.new_groups {
            let source = std::env::current_exe()?
                .parent()
                .context("executable parent missing")?
                .join("file_icons_thumbnail_shell.dll");
            let destination = file_icons_windows::planned_thumbnail_shell(&source)?;
            let installed = file_icons_windows::stage_thumbnail_shell(&source)?;
            ensure!(
                destination.canonicalize()? == installed,
                "thumbnail module differs from plan"
            );
            let inspector = Inspector(WindowsSource::new()?);
            let mut registry = file_icons_windows::UserRegistry;
            let applied = apply_reviewed_groups(
                &path,
                reviewed.existing_groups.as_ref(),
                additional,
                &mut registry,
                &inspector,
            )?;
            let settings = ThumbnailSettings {
                selected: applied.selected_extensions()?,
                ..ThumbnailSettings::default()
            };
            let previous = ThumbnailSettings::load(&settings_path)?;
            if previous.as_ref() != Some(&settings) {
                settings.save(&settings_path, previous.as_ref())?;
            }
        }
        for format in Format::ALL {
            if reviewed.additions.contains(format.extension())
                && reviewed.existing_groups.as_ref().is_some_and(|record| {
                    record
                        .groups
                        .iter()
                        .any(|group| group.members.contains(format.extension()))
                })
            {
                self.set_shared_thumbnail(format, true)?;
            }
        }
        for format in Format::ALL {
            if reviewed.removals.contains(format.extension()) {
                self.disable_thumbnail(format)?;
            }
        }
        let target = ThumbnailSettings {
            selected: reviewed.selected,
            ..ThumbnailSettings::default()
        };
        let previous = ThumbnailSettings::load(&settings_path)?;
        if previous.as_ref() != Some(&target) && (previous.is_some() || !target.selected.is_empty())
        {
            target.save(&settings_path, previous.as_ref())?;
        }
        Ok(())
    }

    /// 選択の変更予定を、登録や設定を変更せず調査する。
    pub fn preview_thumbnail_selection(
        &self,
        selected: BTreeSet<String>,
    ) -> Result<ThumbnailSelectionPlan> {
        use file_icons_windows::thumbnails::{
            Settings as ThumbnailSettings,
            inventory::{Inspector, WindowsSource, plan_selection},
        };
        ThumbnailSettings {
            selected: selected.clone(),
            ..ThumbnailSettings::default()
        }
        .validate()?;
        let status = self.thumbnail_status()?;
        ensure!(
            !status.missing_recovery && status.group_errors.iter().all(Option::is_none),
            "thumbnail registration requires recovery"
        );
        let current = status
            .recovery
            .as_ref()
            .map(|record| record.selected_extensions())
            .transpose()?
            .unwrap_or_default();
        let additions: BTreeSet<_> = selected.difference(&current).cloned().collect();
        let removals: BTreeSet<_> = current.difference(&selected).cloned().collect();
        let independent: BTreeSet<_> = additions
            .iter()
            .filter(|extension| {
                !status.recovery.as_ref().is_some_and(|record| {
                    record
                        .groups
                        .iter()
                        .any(|group| group.members.contains(*extension))
                })
            })
            .cloned()
            .collect();
        let new_groups = if independent.is_empty() {
            None
        } else {
            let source = std::env::current_exe()?
                .parent()
                .context("executable parent missing")?
                .join("file_icons_thumbnail_shell.dll");
            let destination = file_icons_windows::planned_thumbnail_shell(&source)?;
            let registry = file_icons_windows::UserRegistry;
            let inspector = Inspector(WindowsSource::new()?);
            Some(plan_selection(
                &registry,
                &inspector,
                &independent,
                &destination,
            )?)
        };
        Ok(ThumbnailSelectionPlan {
            selected,
            current,
            additions,
            removals,
            existing_groups: status.recovery,
            new_groups,
            recheck_on_execution: true,
        })
    }

    /// 共有ProgIDの残りを維持して、一種類のサムネイルを切り替える。
    pub fn set_shared_thumbnail(
        &self,
        format: file_icons_windows::thumbnails::Format,
        enabled: bool,
    ) -> Result<()> {
        use file_icons_windows::thumbnails::{
            Recovery, Settings as ThumbnailSettings,
            inventory::{Inspector, WindowsSource},
        };
        let root = home()?;
        let path = root.join("thumbnail-recovery.json");
        let settings_path = root.join("thumbnail-settings.json");
        ThumbnailSettings::load(&settings_path)?;
        let mut record = Recovery::load(&path)?.context("thumbnail recovery missing")?;
        let inspector = Inspector(WindowsSource::new()?);
        let mut registry = file_icons_windows::UserRegistry;
        if record.transition.is_some() {
            record = record.finish_transition(&path, &mut registry, &inspector)?;
        }
        let selected = set_shared_thumbnail_record(
            &path,
            record,
            format,
            enabled,
            &mut registry,
            &inspector,
            &mut || clear_thumbnail_settings(&settings_path),
        )?;
        let settings = ThumbnailSettings {
            selected,
            ..ThumbnailSettings::default()
        };
        let previous = ThumbnailSettings::load(&settings_path)?;
        if previous.as_ref() != Some(&settings) {
            settings.save(&settings_path, previous.as_ref())?;
        }
        Ok(())
    }

    /// 指定した種類だけを復元し、他の種類の登録を維持する。
    pub fn disable_thumbnail(&self, format: file_icons_windows::thumbnails::Format) -> Result<()> {
        use file_icons_windows::thumbnails::{
            Recovery, Settings as ThumbnailSettings,
            inventory::{Inspector, WindowsSource},
        };
        let root = home()?;
        let path = root.join("thumbnail-recovery.json");
        let settings_path = root.join("thumbnail-settings.json");
        ThumbnailSettings::load(&settings_path)?;
        let mut record = Recovery::load(&path)?.context("thumbnail recovery missing")?;
        let inspector = Inspector(WindowsSource::new()?);
        let mut registry = file_icons_windows::UserRegistry;
        if record.transition.is_some() {
            record = record.finish_transition(&path, &mut registry, &inspector)?;
        }
        let selected = disable_thumbnail_record(
            &path,
            record,
            format,
            &mut registry,
            &inspector,
            &mut || clear_thumbnail_settings(&settings_path),
        )?;
        let settings = ThumbnailSettings {
            selected,
            ..ThumbnailSettings::default()
        };
        let previous = ThumbnailSettings::load(&settings_path)?;
        if previous.as_ref() != Some(&settings) {
            settings.save(&settings_path, previous.as_ref())?;
        }
        Ok(())
    }

    /// 復元可能な組を個別に戻し、競合する組の記録は保持する。
    pub fn restore_thumbnails(&self) -> Result<()> {
        use file_icons_windows::thumbnails::{
            Recovery, Settings as ThumbnailSettings,
            inventory::{Inspector, WindowsSource},
        };
        let root = home()?;
        let path = root.join("thumbnail-recovery.json");
        let settings_path = root.join("thumbnail-settings.json");
        let record = Recovery::load(&path)?;
        if record.is_none() {
            ensure!(
                ThumbnailSettings::load(&settings_path)?
                    .is_none_or(|settings| settings.selected.is_empty()),
                "thumbnail recovery missing for saved selection"
            );
        }
        if let Some(record) = record {
            let inspector = Inspector(WindowsSource::new()?);
            let mut registry = file_icons_windows::UserRegistry;
            restore_thumbnail_records(&path, record, &mut registry, &inspector, &mut || {
                clear_thumbnail_settings(&settings_path)
            })?;
        }
        clear_thumbnail_settings(&settings_path)
    }
    pub fn conflicts(
        &self,
        strategy: file_icons_windows::conflict::Strategy,
    ) -> Result<file_icons_windows::conflict::Plan> {
        file_icons_windows::conflict::preview(strategy, &self.shell_source()?)
            .context(Failure::Recovery)
    }
    pub fn resolve_conflicts(
        &self,
        strategy: file_icons_windows::conflict::Strategy,
        confirmation: &str,
    ) -> Result<file_icons_windows::conflict::Plan> {
        file_icons_windows::conflict::execute(strategy, confirmation, &self.shell_source()?)
            .context(Failure::Recovery)
    }
    fn shell_source(&self) -> Result<std::path::PathBuf> {
        Ok(std::env::current_exe()?
            .parent()
            .context("executable parent missing")?
            .join("file_icons_shell.dll"))
    }
    pub fn language(&self, language: String) -> Result<()> {
        let mut settings = self.settings()?;
        settings.language = language;
        save(&settings)
    }
    pub fn import(&self, root: &Path, id: &str, progress: &Progress) -> Result<String> {
        let mut settings = self.settings()?;
        progress.begin_theme(1, 1);
        let pack = file_icons_core::import_observed(
            root,
            id,
            &home()?.join("packs"),
            None,
            || progress.check(),
            |completed, total| progress.converted(completed, total),
        )?;
        ensure!(
            settings.active.as_ref() != Some(&pack.id),
            Failure::Precondition
        );
        let id = pack.id.clone();
        if let Some(selection) = settings.selections.get_mut(&id) {
            selection.retain_supported(&pack.theme);
        }
        settings.packs.retain(|old| old.id != id);
        settings.packs.push(pack);
        progress.check()?;
        save(&settings)?;
        Ok(id)
    }
    pub fn download(
        &self,
        id: &str,
        progress: Arc<Progress>,
        provider: DownloadSource,
    ) -> Result<Vec<String>> {
        progress.check()?;
        let mut settings = self.settings()?;
        let packs = provider(id, &home()?.join("packs"), progress.clone())?;
        progress.check()?;
        for pack in &packs {
            ensure!(
                settings.active.as_ref() != Some(&pack.id)
                    || settings
                        .packs
                        .iter()
                        .any(|old| old.id == pack.id && old.provenance == pack.provenance),
                Failure::Precondition
            );
        }
        let ids = packs.iter().map(|p| p.id.clone()).collect();
        for pack in packs {
            if let Some(selection) = settings.selections.get_mut(&pack.id) {
                selection.retain_supported(&pack.theme);
            }
            if settings.active.as_ref() == Some(&pack.id) {
                continue;
            }
            settings.packs.retain(|old| old.id != pack.id);
            settings.packs.push(pack);
        }
        progress.check()?;
        save(&settings)?;
        Ok(ids)
    }
    pub fn apply(&self, id: &str, dll: &Path) -> Result<()> {
        let mut settings = self.settings()?;
        self.check_registration(&settings, false)?;
        let pack = settings
            .packs
            .iter()
            .find(|p| p.id == id)
            .context(Failure::NotFound)?;
        if !settings
            .selections
            .get(id)
            .cloned()
            .unwrap_or_default()
            .any_enabled(&pack.theme)
        {
            return file_icons_windows::restore(&mut settings);
        }
        file_icons_windows::activate(&mut settings, id, dll)
    }
    /// 下書きを保存し、適用中の場合だけ実効選択を同じ保存単位で更新する。
    pub fn select_targets(
        &self,
        id: &str,
        selection: file_icons_core::selection::Selection,
        dry_run: bool,
    ) -> Result<()> {
        self.save_targets(self.settings()?, id, selection, dry_run)
    }

    /// 同じ排他・設定の写しで編集と検証を終える。操作をまたいだキャッシュは持たない。
    pub fn edit_targets<T>(
        &self,
        id: &str,
        dry_run: bool,
        edit: impl FnOnce(&Settings, &mut file_icons_core::selection::Selection) -> Result<T>,
    ) -> Result<(file_icons_core::selection::Selection, T)> {
        let settings = self.settings()?;
        ensure!(
            settings.packs.iter().any(|pack| pack.id == id),
            Failure::NotFound
        );
        let mut selection = settings.selections.get(id).cloned().unwrap_or_default();
        let result = edit(&settings, &mut selection)?;
        self.save_targets(settings, id, selection.clone(), dry_run)?;
        Ok((selection, result))
    }

    fn save_targets(
        &self,
        mut settings: Settings,
        id: &str,
        selection: file_icons_core::selection::Selection,
        dry_run: bool,
    ) -> Result<()> {
        let pack = settings
            .packs
            .iter()
            .find(|p| p.id == id)
            .context(Failure::NotFound)?;
        selection
            .validate_for(&pack.theme)
            .context(Failure::Precondition)?;
        let active = settings.active.as_deref() == Some(id);
        let any = selection.any_enabled(&pack.theme);
        if active {
            self.check_registration(&settings, false)?;
        }
        if dry_run {
            return Ok(());
        }
        if active && any {
            // 古い版のDLLが登録済みでも、選択に対応した同梱DLLへ切り替える。
            let dll = std::env::current_exe()?
                .parent()
                .context("executable parent missing")?
                .join("file_icons_shell.dll");
            return file_icons_windows::activate_selection(
                &mut settings,
                id,
                &dll,
                Some(selection),
            );
        }
        settings.selections.insert(id.into(), selection);
        if active {
            return file_icons_windows::restore(&mut settings);
        }
        save(&settings)
    }
    /// 適用前の登録を基準に、除外理由を読み取りだけで取得する。
    pub fn icon_protection(
        &self,
        id: &str,
    ) -> Result<(bool, Vec<file_icons_windows::IconExclusion>)> {
        let settings = self.settings()?;
        let pack = settings
            .packs
            .iter()
            .find(|p| p.id == id)
            .context(Failure::NotFound)?;
        Ok((
            settings.preserve_existing_icons,
            file_icons_windows::protection_report(pack, &settings)?,
        ))
    }

    pub fn set_icon_protection(&self, enabled: bool, dry_run: bool) -> Result<()> {
        self.set_icon_protection_rules(enabled, self.settings()?.icon_protection_rules, dry_run)
    }

    pub fn inspect_icon_sources(
        &self,
        id: Option<&str>,
        enabled: bool,
        rules: file_icons_windows::IconProtectionRules,
        include_saved: bool,
        retained: Vec<String>,
    ) -> Result<Vec<file_icons_windows::ExistingIcon>> {
        let mut settings = self.settings()?;
        settings.preserve_existing_icons = enabled;
        settings.icon_protection_rules = rules;
        if id.is_none()
            && settings.icon_protection_rules.extensions.is_empty()
            && settings.icon_protection_rules.providers.is_empty()
            && retained.is_empty()
        {
            return Ok(Vec::new());
        }
        let empty = file_icons_core::ImportedPack {
            id: String::new(),
            version: String::new(),
            label: String::new(),
            theme: Default::default(),
            provenance: Default::default(),
            warnings: Vec::new(),
        };
        let pack = match id {
            Some(id) => settings
                .packs
                .iter()
                .find(|p| p.id == id)
                .context(Failure::NotFound)?,
            None => &empty,
        };
        if include_saved {
            file_icons_windows::inspect_retained_icons(pack, &settings, &retained)
        } else {
            file_icons_windows::inspect_pack_icons(pack, &settings)
        }
    }

    pub fn set_icon_protection_rules(
        &self,
        enabled: bool,
        rules: file_icons_windows::IconProtectionRules,
        dry_run: bool,
    ) -> Result<()> {
        let mut settings = self.settings()?;
        self.check_registration(&settings, false)?;
        if dry_run {
            return Ok(());
        }
        if let Some(id) = settings.active.clone() {
            let dll = std::env::current_exe()?
                .parent()
                .context("executable parent missing")?
                .join("file_icons_shell.dll");
            file_icons_windows::activate_protection(
                &mut settings,
                &id,
                &dll,
                None,
                Some(enabled),
                Some(rules),
            )
        } else {
            settings.preserve_existing_icons = enabled;
            settings.icon_protection_rules = rules;
            save(&settings)
        }
    }

    pub fn restore(&self, recover: bool) -> Result<()> {
        let settings = if recover {
            self.recovery_settings()?
        } else {
            self.settings()?
        };
        self.check_registration(&settings, true)?;
        file_icons_windows::restore_registration()
    }
    pub fn remove(&self, id: &str) -> Result<()> {
        let mut settings = self.settings()?;
        file_icons_windows::remove_pack(&mut settings, id)
    }
    pub fn cleanup(&self) -> Result<()> {
        let mut settings = self.settings()?;
        ensure!(
            settings.active.is_none() && settings.journal.is_empty(),
            Failure::Precondition
        );
        let thumbnail = self.thumbnail_status()?;
        ensure!(
            thumbnail.recovery.is_none() && thumbnail.settings.selected.is_empty(),
            Failure::Precondition
        );
        file_icons_windows::cleanup_packs(&mut settings)?;
        file_icons_windows::cleanup_staged_shell()
    }
    pub fn cleanup_packs(&self) -> Result<()> {
        file_icons_windows::cleanup_packs(&mut self.settings()?)
    }
    pub fn recovery_settings(&self) -> Result<Settings> {
        match load() {
            Ok(settings) => Ok(settings),
            Err(_) => file_icons_windows::load_recovery()?.context(Failure::Recovery),
        }
    }
    pub fn check_registration(&self, settings: &Settings, restoring: bool) -> Result<()> {
        ensure!(!file_icons_windows::conflict::pending()?, Failure::Recovery);
        for change in &settings.journal {
            let current = file_icons_windows::UserRegistry.read(&change.key, &change.name)?;
            if current.as_ref() != Some(&change.after) && !(restoring && current == change.before) {
                let name = if change.name.is_empty() {
                    "(Default)"
                } else {
                    &change.name
                };
                let path = home()?.join("settings.json");
                let detail = if settings.language == "en" {
                    format!(
                        "Registration differs from the saved record. No changes were made.\nHKCU\\{} [{}]\nSettings: {}",
                        change.key,
                        name,
                        path.display()
                    )
                } else {
                    format!(
                        "登録状態が保存済みの記録と異なるため、変更を中止しました。\nHKCU\\{} [{}]\n設定ファイル: {}",
                        change.key,
                        name,
                        path.display()
                    )
                };
                return Err(anyhow::Error::new(Failure::Conflict).context(detail));
            }
        }
        ensure!(
            restoring || settings.journal.is_empty() || settings.active.is_some(),
            Failure::Recovery
        );
        Ok(())
    }
    /// 表示用の変更予定。保存先を作ったりレジストリを書き換えたりしない。
    pub fn preview_apply(&self, id: &str, dll: &Path) -> Result<serde_json::Value> {
        let settings = self.settings()?;
        self.check_registration(&settings, false)?;
        let pack = settings
            .packs
            .iter()
            .find(|p| p.id == id)
            .context(Failure::NotFound)?;
        ensure!(dll.is_file(), Failure::Precondition);
        let mut classes = pack
            .theme
            .registration_extensions()
            .iter()
            .map(|extension| file_icons_windows::association(extension))
            .collect::<Result<Vec<_>>>()?;
        if pack.theme.needs_unknown_class() {
            classes.push("Unknown".into());
        }
        let mut allowed = Vec::new();
        for class in classes {
            if !file_icons_windows::protected_class(&class)? {
                allowed.push(class);
            }
        }
        let mut classes = allowed;
        classes.sort();
        classes.dedup();
        let restore_only = !settings
            .selections
            .get(id)
            .cloned()
            .unwrap_or_default()
            .any_enabled(&pack.theme);
        if restore_only {
            classes.clear();
        }
        let exclusions = file_icons_windows::protection_report(pack, &settings)?;
        classes.retain(|class| {
            !exclusions
                .iter()
                .any(|item| item.class.eq_ignore_ascii_case(class))
        });
        Ok(
            serde_json::json!({"pack_id": id, "previous_pack_id": settings.active, "restore_only":restore_only, "classes": classes, "preserve_existing_icons": settings.preserve_existing_icons, "icon_exclusions":exclusions, "restore_changes": settings.journal, "shell_source": dll, "recheck_on_execution": true}),
        )
    }
}

fn apply_reviewed_groups(
    path: &Path,
    existing: Option<&file_icons_windows::thumbnails::Recovery>,
    additional: &file_icons_windows::thumbnails::Recovery,
    registry: &mut (impl Registry + file_icons_windows::thumbnails::KeyState),
    inspector: &(
         impl file_icons_windows::thumbnails::DependencyInspector
         + file_icons_windows::thumbnails::ProviderState
     ),
) -> Result<file_icons_windows::thumbnails::Recovery> {
    match existing {
        Some(record) => record.append_groups(path, additional, registry, inspector),
        None => {
            additional.apply_new(path, registry, inspector)?;
            Ok(additional.clone())
        }
    }
}

fn set_shared_thumbnail_record(
    path: &Path,
    mut record: file_icons_windows::thumbnails::Recovery,
    format: file_icons_windows::thumbnails::Format,
    enabled: bool,
    registry: &mut (impl Registry + file_icons_windows::thumbnails::KeyState),
    inspector: &(
         impl file_icons_windows::thumbnails::DependencyInspector
         + file_icons_windows::thumbnails::ProviderState
     ),
    before_final_removal: &mut impl FnMut() -> Result<()>,
) -> Result<BTreeSet<String>> {
    let index = record
        .groups
        .iter()
        .position(|group| group.class.is_some() && group.members.contains(format.extension()))
        .context("shared thumbnail group missing")?;
    let selected = record.selected_extensions()?;
    if selected.contains(format.extension()) == enabled {
        record.check_applied_group(index, registry, inspector)?;
        return Ok(selected);
    }
    let selected_in_group = selected
        .iter()
        .filter(|extension| record.groups[index].members.contains(*extension))
        .count();
    if !enabled && selected_in_group == 1 {
        record.restore_checked_group(index, registry, inspector)?;
        if record.groups.len() == 1 {
            before_final_removal()?;
        }
        return match record.remove_restored_group(path, index, registry, inspector)? {
            Some(remaining) => remaining.selected_extensions(),
            None => Ok(BTreeSet::new()),
        };
    }
    record = record.set_shared_member(path, index, format, enabled, registry, inspector)?;
    record.selected_extensions()
}

fn disable_thumbnail_record(
    path: &Path,
    record: file_icons_windows::thumbnails::Recovery,
    format: file_icons_windows::thumbnails::Format,
    registry: &mut (impl Registry + file_icons_windows::thumbnails::KeyState),
    inspector: &(
         impl file_icons_windows::thumbnails::DependencyInspector
         + file_icons_windows::thumbnails::ProviderState
     ),
    before_final_removal: &mut impl FnMut() -> Result<()>,
) -> Result<BTreeSet<String>> {
    let index = record
        .groups
        .iter()
        .position(|group| group.members.contains(format.extension()))
        .context("thumbnail group missing")?;
    if record.groups[index].class.is_some() {
        return set_shared_thumbnail_record(
            path,
            record,
            format,
            false,
            registry,
            inspector,
            before_final_removal,
        );
    }
    record.restore_checked_group(index, registry, inspector)?;
    if record.groups.len() == 1 {
        before_final_removal()?;
    }
    match record.remove_restored_group(path, index, registry, inspector)? {
        Some(remaining) => remaining.selected_extensions(),
        None => Ok(BTreeSet::new()),
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;
    use file_icons_core::registry::Value;
    use file_icons_thumbnail_core::format::Format;
    use file_icons_windows::thumbnails::{
        self, Dependencies, DependencyInspector, Group, KeyState, ProviderState, Recovery,
        Reference, Scope,
    };
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
    use windows_core::GUID;

    #[derive(Default)]
    struct Memory(Rc<RefCell<BTreeMap<(String, String), Value>>>);
    impl Registry for Memory {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            Ok(self
                .0
                .borrow()
                .get(&(key.to_ascii_lowercase(), name.to_ascii_lowercase()))
                .cloned())
        }
        fn write(&mut self, key: &str, name: &str, value: Option<&Value>) -> Result<()> {
            let id = (key.to_ascii_lowercase(), name.to_ascii_lowercase());
            if let Some(value) = value {
                self.0.borrow_mut().insert(id, value.clone());
            } else {
                self.0.borrow_mut().remove(&id);
            }
            Ok(())
        }
    }
    impl KeyState for Memory {
        fn key_exists(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn cleanup_created_keys(&mut self, _: &[String]) -> Result<()> {
            Ok(())
        }
    }
    struct Inspector(Rc<RefCell<BTreeMap<(String, String), Value>>>);
    impl DependencyInspector for Inspector {
        fn inspect(&self, group: &Group) -> Result<Dependencies> {
            let class = group.class.as_deref().unwrap_or("Example.Document");
            Ok(Dependencies {
                associations: group
                    .members
                    .iter()
                    .map(|ext| (ext.clone(), class.into()))
                    .collect(),
                references: group.class.as_ref().map(|class| vec![Reference { scope: Scope::Machine, key: format!("Software\\Classes\\{class}\\shellex\\{{e357fccd-a995-4576-b01f-234630154e96}}"), name: String::new(), value: None }]).unwrap_or_default(),
            })
        }
    }
    impl ProviderState for Inspector {
        fn provider(&self, extension: &str) -> Result<Option<GUID>> {
            let key = format!(
                "software\\classes\\{extension}\\shellex\\{{e357fccd-a995-4576-b01f-234630154e96}}"
            );
            let state = self.0.borrow();
            let parent = "software\\classes\\example.vector\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}";
            if matches!(extension, ".ai" | ".ait" | ".eps")
                && let Some(value) = state.get(&(parent.into(), String::new()))
                && value.bytes != [0, 0]
            {
                return Ok(Some(decode(value)?));
            }
            let Some(value) = state.get(&(key, String::new())) else {
                return Ok(None);
            };
            Ok(Some(decode(value)?))
        }
        fn notify(&self) -> Result<()> {
            Ok(())
        }
    }
    fn decode(value: &Value) -> Result<GUID> {
        ensure!(
            value.kind == 1 && value.bytes.len() == 78,
            "invalid provider"
        );
        let units: Vec<u16> = value
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let text = String::from_utf16(&units[..units.len() - 1])?;
        Ok(GUID::try_from(text.trim_matches(['{', '}']))?)
    }

    #[test]
    fn reviewed_shared_group_rejects_a_new_member_before_any_registry_write() {
        struct AddedMember<'a>(&'a Inspector);
        impl DependencyInspector for AddedMember<'_> {
            fn inspect(&self, group: &Group) -> Result<Dependencies> {
                let mut dependencies = self.0.inspect(group)?;
                if group.class.as_deref() == Some("Example.Vector") {
                    dependencies
                        .associations
                        .insert(".ait".into(), "Example.Vector".into());
                }
                Ok(dependencies)
            }
        }
        impl ProviderState for AddedMember<'_> {
            fn provider(&self, extension: &str) -> Result<Option<GUID>> {
                self.0.provider(extension)
            }
            fn notify(&self) -> Result<()> {
                Ok(())
            }
        }

        for append in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("thumbnail-recovery.json");
            let mut registry = Memory::default();
            let inspector = Inspector(registry.0.clone());
            let module = Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll");
            let existing = if append {
                let mut group = thumbnails::plan_extension_group(&registry, Format::Svg).unwrap();
                group.add_provider(&registry, Format::Svg, module).unwrap();
                group.capture_dependencies(&inspector).unwrap();
                group.capture_providers(&inspector).unwrap();
                group.capture_created_keys(&registry).unwrap();
                let record = Recovery {
                    format_version: 1,
                    groups: vec![group],
                    transition: None,
                };
                record.apply_new(&path, &mut registry, &inspector).unwrap();
                Some(record)
            } else {
                None
            };
            let parent = "Software\\Classes\\Example.Vector\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}";
            let old = Value {
                kind: 1,
                bytes: "{11111111-2222-4333-8444-555555555555}"
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            };
            registry.write(parent, "", Some(&old)).unwrap();
            let members = [".ai".into()].into_iter().collect();
            let mut group = thumbnails::plan_shared_group(
                &registry,
                "Example.Vector",
                members,
                &[".ai".into()].into_iter().collect(),
            )
            .unwrap();
            group.add_provider(&registry, Format::Ai, module).unwrap();
            group.capture_dependencies(&inspector).unwrap();
            group.capture_providers(&inspector).unwrap();
            group.capture_created_keys(&registry).unwrap();
            let reviewed = Recovery {
                format_version: 1,
                groups: vec![group],
                transition: None,
            };
            let before = registry.0.borrow().clone();
            let saved = fs::read(&path).ok();

            let error = apply_reviewed_groups(
                &path,
                existing.as_ref(),
                &reviewed,
                &mut registry,
                &AddedMember(&inspector),
            )
            .unwrap_err();
            assert!(
                format!("{error:#}").contains("thumbnail associations"),
                "{error:#}"
            );
            assert_eq!(*registry.0.borrow(), before);
            assert_eq!(fs::read(&path).ok(), saved);

            let applied = apply_reviewed_groups(
                &path,
                existing.as_ref(),
                &reviewed,
                &mut registry,
                &inspector,
            )
            .unwrap();
            let mut expected: BTreeSet<String> = [".ai".into()].into_iter().collect();
            if append {
                expected.insert(".svg".into());
            }
            assert_eq!(applied.selected_extensions().unwrap(), expected);
            assert_eq!(Recovery::load(&path).unwrap(), Some(applied));
        }
    }

    #[test]
    fn restoration_keeps_conflicting_group_and_restores_independent_group() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("thumbnail-recovery.json");
        let damaged_settings = root.path().join("thumbnail-settings.json");
        fs::write(&damaged_settings, b"damaged preferences").unwrap();
        let mut registry = Memory::default();
        let inspector = Inspector(registry.0.clone());
        let mut groups = Vec::new();
        for format in [Format::Svg, Format::Pdf] {
            let mut group = thumbnails::plan_extension_group(&registry, format).unwrap();
            group
                .add_provider(
                    &registry,
                    format,
                    Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
                )
                .unwrap();
            group.capture_dependencies(&inspector).unwrap();
            group.capture_providers(&inspector).unwrap();
            group.capture_created_keys(&registry).unwrap();
            groups.push(group);
        }
        let record = Recovery {
            format_version: 1,
            groups,
            transition: None,
        };
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        let svg = "Software\\Classes\\.svg\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}";
        let external = Value {
            kind: 3,
            bytes: vec![7, 8, 9],
        };
        registry.write(svg, "", Some(&external)).unwrap();
        assert!(
            restore_thumbnail_records(&path, record, &mut registry, &inspector, &mut || Ok(()))
                .is_err()
        );
        assert_eq!(registry.read(svg, "").unwrap(), Some(external));
        assert_eq!(inspector.provider(".pdf").unwrap(), None);
        let remaining = Recovery::load(&path).unwrap().unwrap();
        assert_eq!(remaining.groups.len(), 1);
        assert!(remaining.groups[0].members.contains(".svg"));
        let own = remaining.groups[0]
            .changes
            .iter()
            .find(|change| change.key.eq_ignore_ascii_case(svg))
            .unwrap()
            .after
            .clone();
        registry.write(svg, "", Some(&own)).unwrap();
        restore_thumbnail_records(&path, remaining, &mut registry, &inspector, &mut || Ok(()))
            .unwrap();
        assert!(!path.exists());
        assert_eq!(inspector.provider(".svg").unwrap(), None);
        assert_eq!(fs::read(damaged_settings).unwrap(), b"damaged preferences");
    }

    #[test]
    fn pending_shared_conflict_does_not_block_independent_restoration() {
        // 中断組の前後を解除し、保存する組番号と再試行時の復元も検証する。
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("thumbnail-recovery.json");
        let mut registry = Memory::default();
        let inspector = Inspector(registry.0.clone());
        let module = Path::new(r"C:\FileHue\file_icons_thumbnail_shell.dll");
        let parent =
            "Software\\Classes\\Example.Vector\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}";
        let original = Value {
            kind: 1,
            bytes: "{11111111-2222-4333-8444-555555555555}"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        registry.write(parent, "", Some(&original)).unwrap();
        let mut groups = Vec::new();
        for format in [Format::Svg, Format::Ai, Format::Pdf] {
            let mut group = if format == Format::Ai {
                thumbnails::plan_shared_group(
                    &registry,
                    "Example.Vector",
                    [".ai".into(), ".ait".into(), ".eps".into()]
                        .into_iter()
                        .collect(),
                    &[".ai".into(), ".eps".into()].into_iter().collect(),
                )
                .unwrap()
            } else {
                thumbnails::plan_extension_group(&registry, format).unwrap()
            };
            group.add_provider(&registry, format, module).unwrap();
            if format == Format::Ai {
                group.add_provider(&registry, Format::Eps, module).unwrap();
            }
            group.capture_dependencies(&inspector).unwrap();
            group.capture_providers(&inspector).unwrap();
            group.capture_created_keys(&registry).unwrap();
            groups.push(group);
        }
        let mut record = Recovery {
            format_version: 1,
            groups,
            transition: None,
        };
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        let ai = record.groups[1]
            .changes
            .iter()
            .find(|c| c.key.contains(".ai\\shellex"))
            .unwrap();
        record.transition = Some(thumbnails::Transition {
            group_index: 1,
            extension: ".ai".into(),
            from: ai.after.clone(),
            to: original.clone(),
        });
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let ait = "Software\\Classes\\.ait\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}";
        let external = Value {
            kind: 3,
            bytes: vec![7, 9],
        };
        registry.write(ait, "", Some(&external)).unwrap();
        let shared_before: BTreeMap<_, _> = record.groups[1]
            .changes
            .iter()
            .map(|c| {
                (
                    (c.key.clone(), c.name.clone()),
                    registry.read(&c.key, &c.name).unwrap(),
                )
            })
            .collect();
        let mut final_removals = 0;
        assert!(
            restore_thumbnail_records(&path, record, &mut registry, &inspector, &mut || {
                final_removals += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(inspector.provider(".svg").unwrap(), None);
        assert_eq!(inspector.provider(".pdf").unwrap(), None);
        assert_eq!(final_removals, 0);
        let pending = Recovery::load(&path).unwrap().unwrap();
        assert_eq!(pending.groups.len(), 1);
        assert_eq!(pending.transition.as_ref().unwrap().group_index, 0);
        for ((key, name), value) in shared_before {
            assert_eq!(registry.read(&key, &name).unwrap(), value);
        }
        registry.write(ait, "", Some(&original)).unwrap();
        restore_thumbnail_records(&path, pending, &mut registry, &inspector, &mut || {
            final_removals += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(final_removals, 1);
        assert!(!path.exists());
        assert_eq!(registry.read(parent, "").unwrap(), Some(original));
    }

    #[test]
    fn disabling_one_independent_format_keeps_the_other_registered() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("thumbnail-recovery.json");
        let mut registry = Memory::default();
        let inspector = Inspector(registry.0.clone());
        let mut groups = Vec::new();
        for format in [Format::Svg, Format::Pdf] {
            let mut group = thumbnails::plan_extension_group(&registry, format).unwrap();
            group
                .add_provider(
                    &registry,
                    format,
                    Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
                )
                .unwrap();
            group.capture_dependencies(&inspector).unwrap();
            group.capture_providers(&inspector).unwrap();
            group.capture_created_keys(&registry).unwrap();
            groups.push(group);
        }
        let record = Recovery {
            format_version: 1,
            groups,
            transition: None,
        };
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        let remaining = disable_thumbnail_record(
            &path,
            record,
            Format::Svg,
            &mut registry,
            &inspector,
            &mut || Ok(()),
        )
        .unwrap();
        assert_eq!(remaining, [".pdf".into()].into_iter().collect());
        assert_eq!(inspector.provider(".svg").unwrap(), None);
        assert_eq!(
            inspector.provider(".pdf").unwrap(),
            Some(GUID::from_u128(Format::Pdf.clsid()))
        );
        let record = Recovery::load(&path).unwrap().unwrap();
        record
            .check_applied_group(0, &registry, &inspector)
            .unwrap();
    }

    #[test]
    fn final_restoration_retains_recovery_until_settings_are_saved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("thumbnail-recovery.json");
        let mut registry = Memory::default();
        let inspector = Inspector(registry.0.clone());
        let mut group = thumbnails::plan_extension_group(&registry, Format::Svg).unwrap();
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
        let record = Recovery {
            format_version: 1,
            groups: vec![group],
            transition: None,
        };
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        assert!(
            disable_thumbnail_record(
                &path,
                record,
                Format::Svg,
                &mut registry,
                &inspector,
                &mut || anyhow::bail!("simulated settings failure")
            )
            .is_err()
        );
        assert_eq!(inspector.provider(".svg").unwrap(), None);
        let record = Recovery::load(&path).unwrap().unwrap();
        let mut called = false;
        let selected = disable_thumbnail_record(
            &path,
            record,
            Format::Svg,
            &mut registry,
            &inspector,
            &mut || {
                called = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(called && selected.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn disabling_last_shared_format_restores_and_removes_the_group() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("thumbnail-recovery.json");
        let mut registry = Memory::default();
        let inspector = Inspector(registry.0.clone());
        let parent =
            "Software\\Classes\\Example.Vector\\shellex\\{e357fccd-a995-4576-b01f-234630154e96}";
        let old = Value {
            kind: 1,
            bytes: "{11111111-2222-4333-8444-555555555555}"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        registry.write(parent, "", Some(&old)).unwrap();
        let members = [".ai".into(), ".ait".into(), ".eps".into()]
            .into_iter()
            .collect();
        let selected = [".ai".into(), ".eps".into()].into_iter().collect();
        let mut group =
            thumbnails::plan_shared_group(&registry, "Example.Vector", members, &selected).unwrap();
        let module = Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll");
        group.add_provider(&registry, Format::Ai, module).unwrap();
        group.add_provider(&registry, Format::Eps, module).unwrap();
        group.capture_dependencies(&inspector).unwrap();
        group.capture_providers(&inspector).unwrap();
        group.capture_created_keys(&registry).unwrap();
        let record = Recovery {
            format_version: 1,
            groups: vec![group],
            transition: None,
        };
        record.apply_new(&path, &mut registry, &inspector).unwrap();
        let selected = set_shared_thumbnail_record(
            &path,
            record,
            Format::Ai,
            false,
            &mut registry,
            &inspector,
            &mut || Ok(()),
        )
        .unwrap();
        assert_eq!(selected, [".eps".into()].into_iter().collect());
        let remaining = Recovery::load(&path).unwrap().unwrap();
        let selected = set_shared_thumbnail_record(
            &path,
            remaining,
            Format::Eps,
            false,
            &mut registry,
            &inspector,
            &mut || Ok(()),
        )
        .unwrap();
        assert!(selected.is_empty());
        assert!(!path.exists());
        assert_eq!(registry.read(parent, "").unwrap(), Some(old));
    }
}
