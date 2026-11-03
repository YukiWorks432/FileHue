//! パック切り替えの失敗時は新しい登録を解除して、以前のパックを再適用する。
use crate::{
    CLSID_TEXT, Settings, UserRegistry, apply, load, restore, restore_registration, save,
    stage_shell,
};
use anyhow::{Context, Result, ensure};
use file_icons_core::registry::Registry;
use std::{fmt, path::Path};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

/// 切り替えには失敗したが、以前のパックへの復帰は完了している。
#[derive(Debug)]
pub struct PreviousPackRestored {
    pub cause: anyhow::Error,
}
impl fmt::Display for PreviousPackRestored {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "previous icon pack restored: {}", self.cause)
    }
}
impl std::error::Error for PreviousPackRestored {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

/// 保存済みの設定を使用する。外部変更と未完了の登録を検出してから切り替える。
pub fn activate(settings: &mut Settings, pack_id: &str, source: &Path) -> Result<()> {
    activate_selection(settings, pack_id, source, None)
}

/// 選択の更新とDLL切り替えを、旧設定へ戻せる同じ操作として扱う。
pub fn activate_selection(
    settings: &mut Settings,
    pack_id: &str,
    source: &Path,
    selection: Option<file_icons_core::selection::Selection>,
) -> Result<()> {
    activate_options(settings, pack_id, source, selection, None)
}

/// 対象選択と共通の保護設定を、登録切り替えの失敗時にも一緒に戻す。
pub fn activate_options(
    settings: &mut Settings,
    pack_id: &str,
    source: &Path,
    selection: Option<file_icons_core::selection::Selection>,
    preserve_existing_icons: Option<bool>,
) -> Result<()> {
    activate_protection(
        settings,
        pack_id,
        source,
        selection,
        preserve_existing_icons,
        None,
    )
}

pub fn activate_protection(
    settings: &mut Settings,
    pack_id: &str,
    source: &Path,
    selection: Option<file_icons_core::selection::Selection>,
    preserve_existing_icons: Option<bool>,
    rules: Option<crate::IconProtectionRules>,
) -> Result<()> {
    *settings = load()?;
    ensure!(
        settings.packs.iter().any(|pack| pack.id == pack_id),
        "pack not found"
    );
    let installed = stage_shell(source)?;
    if settings.journal.is_empty() {
        let previous = settings.clone();
        if let Some(selection) = selection {
            settings.selections.insert(pack_id.into(), selection);
        }
        if let Some(enabled) = preserve_existing_icons {
            settings.preserve_existing_icons = enabled;
        }
        if let Some(rules) = rules {
            settings.icon_protection_rules = rules;
        }
        return match apply(settings, pack_id, &installed) {
            Err(cause) if previous.active.is_some() => {
                // 全種類が除外されていた場合も、保存・適用に失敗した設定を確定しない。
                restore_registration().context("cannot restore interrupted protection update")?;
                save(&previous).context("cannot restore previous protection preferences")?;
                *settings = previous;
                Err(PreviousPackRestored { cause }.into())
            }
            result => result,
        };
    }
    let previous_id = settings
        .active
        .clone()
        .context("finish pending recovery before switching packs")?;
    ensure!(
        settings.packs.iter().any(|pack| pack.id == previous_id),
        "previous pack is unavailable; restore registration first"
    );
    for change in &settings.journal {
        ensure!(
            UserRegistry.read(&change.key, &change.name)?.as_ref() == Some(&change.after),
            file_icons_core::registry::Conflict(format!(
                "registration changed before switch: {}",
                change.key
            ))
        );
    }
    let key = format!("Software\\Classes\\CLSID\\{CLSID_TEXT}\\InprocServer32");
    ensure!(
        settings
            .journal
            .iter()
            .any(|change| change.key == key && change.name.is_empty()),
        "previous Shell registration is missing from recovery record"
    );
    let previous_module: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(&key)?
        .get_value("")?;
    // 元の配布物が更新・削除されていても、戻す版のDLLを先に確保する。
    let previous_module =
        stage_shell(Path::new(&previous_module)).context("cannot preserve previous Shell DLL")?;
    let mut previous_preferences = settings.clone();
    previous_preferences.active = None;
    previous_preferences.journal.clear();
    previous_preferences.created_keys.clear();
    previous_preferences.fallbacks.clear();
    previous_preferences.original_handlers.clear();
    previous_preferences.icon_exclusions.clear();
    restore(settings)?;
    if let Some(selection) = selection {
        settings.selections.insert(pack_id.into(), selection);
    }
    if let Some(enabled) = preserve_existing_icons {
        settings.preserve_existing_icons = enabled;
    }
    if let Some(rules) = rules {
        settings.icon_protection_rules = rules;
    }
    if let Err(cause) = apply(settings, pack_id, &installed) {
        // 途中適用と途中復元のどちらでも、復元記録を使ってまず元のWindows状態へ戻す。
        restore_registration().with_context(|| {
            format!("pack switch failed; pending registration could not be restored: {cause:#}")
        })?;
        save(&previous_preferences).with_context(|| {
            format!("pack switch failed; previous preferences could not be restored: {cause:#}")
        })?;
        *settings = previous_preferences;
        apply(settings, &previous_id, &previous_module).with_context(|| {
            format!("pack switch failed; previous pack could not be reapplied: {cause:#}")
        })?;
        return Err(PreviousPackRestored { cause }.into());
    }
    Ok(())
}
