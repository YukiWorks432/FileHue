use super::*;
use file_icons_catalog::CATALOG;
use file_icons_core::ImportedPack;
use file_icons_windows::Settings;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn entry(id: &str) -> Result<&'static file_icons_catalog::CatalogEntry> {
    CATALOG
        .iter()
        .find(|entry| entry.id == id)
        .context(Failure::NotFound)
}
fn catalog(entry: &file_icons_catalog::CatalogEntry) -> Value {
    json!({"id":entry.id,"name":entry.name,"version":entry.version,"author":entry.author,"themes":entry.themes,"license":entry.license,"url":entry.url(),"sha256":entry.sha256})
}
fn pack<'a>(settings: &'a Settings, id: &str) -> Result<&'a ImportedPack> {
    settings
        .packs
        .iter()
        .find(|pack| pack.id == id)
        .context(Failure::NotFound)
}
fn summary(pack: &ImportedPack) -> Value {
    json!({"id":pack.id,"label":pack.label,"version":pack.version,"warning_count":pack.warnings.len()})
}
fn warning_count(manager: &Manager, ids: &[String]) -> Result<usize> {
    Ok(manager
        .settings()?
        .packs
        .iter()
        .filter(|pack| ids.contains(&pack.id))
        .map(|pack| pack.warnings.len())
        .sum())
}

pub fn independent(args: &Args, en: bool) -> Result<Value> {
    match args.command.as_str() {
        "version" => Ok(json!({"version":env!("CARGO_PKG_VERSION")})),
        "help" => Ok(
            json!({"help":format!("{}\nfile-icons [--json] [--lang ja|en] <command>\n\nstatus\ncatalog list\ncatalog show <id>\npack list\npack show <id>\npack download <catalog-id>\npack inspect <folder>\npack import <folder> --theme <theme-id>\npack remove <id> [--dry-run]\nresolve <pack-id> <filename>\napply <pack-id> [--dry-run]\nrestore [--dry-run]\nrecover [--dry-run]\nconflicts list\nrecover --strategy preserve-external|reapply-theme --dry-run\nrecover --strategy preserve-external|reapply-theme --confirm <id>\ncleanup [--dry-run]\ntargets list <pack-id>\ntargets set <pack-id> <changes.json> [--dry-run]\ntargets edit <pack-id> <all|category:id|query:text|target-id> <on|off|inherit> [--dry-run]\ntargets copy <source-id> <destination-id> [--dry-run]\nprotection show\nprotection set <on|off> [--dry-run]\nprotection exclusions <pack-id>\n\n{}",
            text(en,"Windowsのファイルアイコンを管理します。","Manage Windows file icons."),
            text(en,"取得だけでは適用しません。変更操作の前にGUIを閉じてください。--は以後の引数を値として扱います。","Downloading does not apply icons. Close the GUI before managing state. Use -- before literal arguments."))}),
        ),
        "catalog list" => Ok(json!({"catalog":CATALOG.iter().map(catalog).collect::<Vec<_>>()})),
        "catalog show" => Ok(catalog(entry(&args.values[0])?)),
        "pack inspect" => Ok(
            json!({"themes":file_icons_core::discover(&PathBuf::from(&args.values[0])).context(Failure::Precondition)?}),
        ),
        _ => unreachable!(),
    }
}

/// 進捗は標準エラーだけへ出し、結果JSONを混ぜない。
struct DisplayProgress {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl DisplayProgress {
    fn start(progress: Arc<Progress>, en: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let stage = match progress.stage.load(Ordering::Relaxed) {
                    0 => text(en, "取得中", "Downloading"),
                    1 => text(en, "検証・展開中", "Verifying and extracting"),
                    _ => text(en, "変換中", "Converting"),
                };
                let _ = writeln!(
                    io::stderr().lock(),
                    "{stage}: {} bytes",
                    progress.bytes.load(Ordering::Relaxed)
                );
                for _ in 0..5 {
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for DisplayProgress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn run(manager: &Manager, args: &Args, progress: Arc<Progress>, en: bool) -> Result<Value> {
    if args.command == "conflicts list" {
        return Ok(serde_json::to_value(manager.conflicts(
            file_icons_windows::conflict::Strategy::PreserveExternal,
        )?)?);
    }
    if args.command == "recover"
        && let Some(strategy) = args.strategy
    {
        let plan = if args.dry_run {
            manager.conflicts(strategy)?
        } else {
            manager.resolve_conflicts(
                strategy,
                args.confirmation
                    .as_deref()
                    .context(Failure::Precondition)?,
            )?
        };
        return Ok(json!({"dry_run":args.dry_run,"recovered":!args.dry_run,"plan":plan}));
    }
    progress.check()?;
    let dry = args.dry_run;
    let id = args.values.first().map(String::as_str).unwrap_or_default();
    match args.command.as_str() {
        "protection show" => {
            Ok(json!({"preserve_existing_icons":manager.settings()?.preserve_existing_icons}))
        }
        "protection set" => {
            let enabled = id == "on";
            manager.set_icon_protection(enabled, dry)?;
            Ok(json!({"preserve_existing_icons":enabled,"dry_run":dry}))
        }
        "protection exclusions" => {
            let (enabled, exclusions) = manager.icon_protection(id)?;
            Ok(json!({"pack_id":id,"preserve_existing_icons":enabled,"icon_exclusions":exclusions}))
        }
        "targets list" => {
            let settings = manager.settings()?;
            let selection = settings.selections.get(id).cloned().unwrap_or_default();
            let targets=pack(&settings,id)?.theme.targets().into_iter().map(|target|json!({"enabled":selection.target_enabled(&target),"override":selection.explicit(&target.id),"protected":target.protected(),"target":target})).collect::<Vec<_>>();
            Ok(
                json!({"pack_id":id,"targets":targets,"selection":selection,"categories":file_icons_core::selection::CATEGORIES.iter().map(|(id,ja,en_label)|json!({"id":id,"label":if en {*en_label}else{*ja}})).collect::<Vec<_>>()}),
            )
        }
        "targets edit" => {
            let scope = &args.values[1];
            let value = match args.values[2].as_str() {
                "on" => Some(true),
                "off" => Some(false),
                "inherit" => None,
                _ => anyhow::bail!(Failure::Precondition),
            };
            let (selection, changed_targets) =
                manager.edit_targets(id, dry, |settings, selection| {
                    let theme = &pack(settings, id)?.theme;
                    let query = scope
                        .strip_prefix("query:")
                        .map(file_icons_core::selection::TargetQuery::new);
                    let selected: Vec<_> = theme
                        .targets()
                        .into_iter()
                        .filter(|target| {
                            scope == "all"
                                || target.id == *scope
                                || scope
                                    .strip_prefix("category:")
                                    .is_some_and(|category| target.category == category)
                                || query.as_ref().is_some_and(|query| query.matches(target))
                        })
                        .collect();
                    anyhow::ensure!(!selected.is_empty(), Failure::NotFound);
                    if value == Some(true)
                        && selected.iter().any(|t| t.protected())
                        && selected.iter().any(|t| t.id == *scope)
                    {
                        return Err(anyhow::anyhow!(Failure::Precondition).context(text(
                            en,
                            "Windowsが管理する保護対象は有効にできません",
                            "Protected file types managed by Windows cannot be enabled",
                        )));
                    }
                    for target in &selected {
                        if target.protected() {
                            continue;
                        }
                        selection.set(&target.id, value)?;
                    }
                    Ok(selected.iter().filter(|t| !t.protected()).count())
                })?;
            Ok(
                json!({"pack_id":id,"dry_run":dry,"changed_targets":changed_targets,"selection":selection}),
            )
        }
        "targets set" | "targets copy" => {
            let destination = if args.command == "targets copy" {
                args.values[1].as_str()
            } else {
                id
            };
            let (selection, unmatched) =
                manager.edit_targets(destination, dry, |settings, selection| {
                    if args.command == "targets copy" {
                        let original = settings.selections.get(id).cloned().unwrap_or_default();
                        let (result, unmatched) = original.copy_to(
                            &pack(settings, id)?.theme,
                            &pack(settings, destination)?.theme,
                            selection,
                        );
                        *selection = result;
                        Ok(unmatched)
                    } else {
                        // 差分は一つのJSONで受け、複数対象を一回の保存で反映する。nullは継承へ戻す。
                        let bytes = file_icons_core::storage::read_file(
                            &std::env::current_dir()?.join(&args.values[1]),
                            1024 * 1024,
                        )?;
                        let changes: std::collections::BTreeMap<String, Option<bool>> =
                            serde_json::from_slice(&bytes).context(Failure::Precondition)?;
                        let supported: std::collections::BTreeSet<_> = pack(settings, id)?
                            .theme
                            .targets()
                            .into_iter()
                            .map(|target| target.id)
                            .collect();
                        anyhow::ensure!(
                            changes
                                .keys()
                                .all(|target| supported.contains(&target.to_lowercase())),
                            Failure::Precondition
                        );
                        for (target, value) in changes {
                            selection
                                .set(&target, value)
                                .context(Failure::Precondition)?;
                        }
                        Ok(Vec::new())
                    }
                })?;
            Ok(
                json!({"pack_id":destination,"dry_run":dry,"selection":selection,"unmatched":unmatched}),
            )
        }
        "status" => {
            let settings = file_icons_windows::load().context(Failure::Recovery)?;
            let consistent = manager.check_registration(&settings, false).is_ok();
            Ok(
                json!({"active_pack_id":settings.active,"registration_state":if consistent { if settings.journal.is_empty() { "inactive" } else { "active" } } else { "recovery_required" },"registration_changes":settings.journal.len(),"conflict_recovery_pending":file_icons_windows::conflict::pending()?,"pending_pack_removals":settings.pending_pack_removals,"packs":settings.packs.iter().map(summary).collect::<Vec<_>>()}),
            )
        }
        "pack list" => {
            Ok(json!({"packs":manager.settings()?.packs.iter().map(summary).collect::<Vec<_>>()}))
        }
        "pack show" => {
            let settings = manager.settings()?;
            let pack = pack(&settings, id)?;
            let provenance: Value = serde_json::from_slice(&file_icons_core::storage::read_file(
                &pack.provenance,
                16 * 1024 * 1024,
            )?)?;
            Ok(
                json!({"pack":summary(pack),"provenance":provenance,"storage":pack.provenance.parent()}),
            )
        }
        "resolve" => {
            let settings = manager.settings()?;
            let selection = settings.selections.get(id).cloned().unwrap_or_default();
            let (icon, reason) = selection
                .resolve(&pack(&settings, id)?.theme, &args.values[1])
                .map(|(icon, reason)| (Some(icon), reason))
                .unwrap_or((
                    None,
                    if file_icons_core::selection::protected_path(&args.values[1]) {
                        "protected_file_type"
                    } else {
                        "windows_default"
                    },
                ));
            Ok(
                json!({"pack_id":id,"filename":args.values[1],"icon":icon,"reason":reason,"selection_enabled":selection.enabled(&args.values[1])}),
            )
        }
        "pack download" => {
            entry(id)?;
            let _display = DisplayProgress::start(progress.clone(), en);
            let ids = manager.download(id, progress, file_icons_catalog::download)?;
            Ok(json!({"warning_count":warning_count(manager,&ids)?,"pack_ids":ids,"applied":false}))
        }
        "pack import" => {
            let root = PathBuf::from(id);
            let theme = args.theme.as_deref().context(Failure::Precondition)?;
            anyhow::ensure!(
                file_icons_core::discover(&root)?
                    .iter()
                    .any(|item| item.id == theme),
                Failure::NotFound
            );
            let id = manager.import(&root, theme, &progress)?;
            Ok(
                json!({"warning_count":warning_count(manager,std::slice::from_ref(&id))?,"pack_id":id,"applied":false}),
            )
        }
        "apply" => {
            let dll = std::env::current_exe()?
                .parent()
                .context("executable parent missing")?
                .join("file_icons_shell.dll");
            let preview = manager.preview_apply(id, &dll)?;
            if dry {
                return Ok(json!({"dry_run":true,"plan":preview}));
            }
            progress.check()?;
            manager.apply(id, &dll)?;
            Ok(json!({"active_pack_id":manager.settings()?.active}))
        }
        "restore" | "recover" => {
            let recover = args.command == "recover";
            let settings = if recover {
                manager.recovery_settings()?
            } else {
                manager.settings()?
            };
            manager.check_registration(&settings, true)?;
            if dry {
                return Ok(
                    json!({"dry_run":true,"restore_changes":settings.journal,"backup_settings_if_unreadable":recover,"recheck_on_execution":true}),
                );
            }
            progress.check()?;
            manager.restore(recover)?;
            Ok(json!({"active_pack_id":null,"restored":true}))
        }
        "pack remove" => {
            let settings = manager.settings()?;
            let pack = pack(&settings, id)?;
            anyhow::ensure!(
                settings.active.as_deref() != Some(id),
                Failure::Precondition
            );
            let storage = pack.provenance.parent();
            let shared = settings
                .packs
                .iter()
                .any(|other| other.id != id && other.provenance.parent() == storage);
            if dry {
                return Ok(
                    json!({"dry_run":true,"remove_pack_id":id,"storage":storage,"keep_shared_storage":shared,"verify_ownership_on_execution":true}),
                );
            }
            progress.check()?;
            manager.remove(id)?;
            Ok(json!({"removed_pack_id":id}))
        }
        "cleanup" => {
            let settings = manager.settings()?;
            anyhow::ensure!(
                settings.active.is_none() && settings.journal.is_empty(),
                Failure::Precondition
            );
            if dry {
                let path = file_icons_windows::home()?.join("shell");
                let candidates = if path.try_exists()? {
                    fs::read_dir(path)?
                        .take(1025)
                        .map(|entry| Ok(entry?.path()))
                        .collect::<Result<Vec<_>>>()?
                } else {
                    vec![]
                };
                anyhow::ensure!(candidates.len() <= 1024, Failure::Precondition);
                return Ok(
                    json!({"dry_run":true,"pending_pack_removals":settings.pending_pack_removals,"shell_candidates":candidates,"verify_ownership_on_execution":true}),
                );
            }
            progress.check()?;
            manager.cleanup()?;
            Ok(json!({"cleaned":true}))
        }
        _ => unreachable!(),
    }
}
