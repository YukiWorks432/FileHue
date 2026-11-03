//! GUIとCLIの管理操作。同じ排他を保持したまま保存・登録処理を呼び出す。
use anyhow::{Context, Result, ensure};
pub use file_icons_catalog::Progress;
use file_icons_core::{ImportedPack, registry::Registry};
use file_icons_windows::{Settings, home, load, save};
use std::{fs, path::Path, sync::Arc};
pub type DownloadSource = fn(&str, &Path, Arc<Progress>) -> Result<Vec<ImportedPack>>;

#[derive(Debug)]
pub enum Failure {
    Busy,
    NotFound,
    Precondition,
    Conflict,
    Recovery,
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
