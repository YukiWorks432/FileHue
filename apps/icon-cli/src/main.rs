//! コンソール用の管理入口。GUIの初期化やウィンドウ生成を行わない。

#[cfg(test)]
#[path = "../../../test-support/process.rs"]
mod test_process;
use anyhow::{Context, Result};
use file_icons_manager_service::{Failure, Manager, Progress};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use windows::Win32::System::{Com::*, Console::*};
use windows::core::BOOL;
mod args;
mod commands;
use args::Args;

static PROGRESS: OnceLock<Arc<Progress>> = OnceLock::new();
unsafe extern "system" fn interrupt(event: u32) -> BOOL {
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        if let Some(progress) = PROGRESS.get() {
            progress.cancel();
        }
        BOOL(1)
    } else {
        BOOL(0)
    }
}
struct Console;
impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            let _ = SetConsoleCtrlHandler(Some(interrupt), false);
            CoUninitialize();
        }
    }
}
fn initialize(progress: Arc<Progress>) -> Result<Console> {
    let _ = PROGRESS.set(progress);
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        if let Err(error) = SetConsoleCtrlHandler(Some(interrupt), true) {
            CoUninitialize();
            return Err(error.into());
        }
    }
    Ok(Console)
}
fn text(en: bool, ja: &'static str, english: &'static str) -> &'static str {
    if en { english } else { ja }
}
fn classify(error: &anyhow::Error) -> (&'static str, u8) {
    if error.is::<file_icons_windows::conflict::ConfirmationChanged>() {
        return ("registration_conflict", 6);
    }
    if error
        .downcast_ref::<file_icons_catalog::Cancelled>()
        .is_some()
    {
        return ("cancelled", 130);
    }
    if let Some(failure) = error.downcast_ref::<Failure>() {
        return match failure {
            Failure::Busy => ("manager_busy", 3),
            Failure::NotFound => ("not_found", 4),
            Failure::Precondition => ("precondition_failed", 4),
            Failure::Conflict => ("registration_conflict", 6),
            Failure::Recovery => ("recovery_required", 7),
        };
    }
    if error
        .downcast_ref::<file_icons_core::registry::Conflict>()
        .is_some()
    {
        return ("registration_conflict", 6);
    }
    if error
        .downcast_ref::<file_icons_windows::PackCleanupPending>()
        .is_some()
        || error
            .downcast_ref::<file_icons_windows::ShellCleanupPending>()
            .is_some()
    {
        return ("cleanup_pending", 5);
    }
    ("operation_failed", 5)
}
fn message(code: &str, en: bool) -> &'static str {
    match code {
        "invalid_arguments" => text(
            en,
            "引数が正しくありません。--helpを確認してください。",
            "Invalid arguments. See --help.",
        ),
        "manager_busy" => text(
            en,
            "別の管理操作が実行中です。GUIを閉じて再試行してください。",
            "Another manager is running. Close the GUI and retry.",
        ),
        "not_found" => text(
            en,
            "指定した識別子が見つかりません。",
            "The requested identifier was not found.",
        ),
        "precondition_failed" => text(
            en,
            "操作の前提条件を満たしていません。対象と適用状態を確認してください。",
            "Operation preconditions were not met. Check the target and active state.",
        ),
        "registration_conflict" => text(
            en,
            "登録の競合を検出しました。外部の変更を保持します。",
            "Registration conflict detected. External changes are preserved.",
        ),
        "recovery_required" => text(
            en,
            "設定または登録の復旧が必要です。recover --dry-runを確認してください。",
            "Settings or registration require recovery. Inspect recover --dry-run.",
        ),
        "cancelled" => text(
            en,
            "処理を中断しました。未完成のパックは一覧に追加していません。",
            "Cancelled. No incomplete pack was added to the library.",
        ),
        "cleanup_pending" => text(
            en,
            "整理が一部残っています。状態を確認して再試行してください。",
            "Some cleanup is pending. Check status and retry.",
        ),
        _ => text(
            en,
            "処理に失敗しました。詳細と状態を確認してください。",
            "Operation failed. Inspect the details and current status.",
        ),
    }
}
fn emit(json_output: bool, envelope: &Value, en: bool) -> io::Result<()> {
    if json_output {
        writeln!(io::stdout().lock(), "{}", serde_json::to_string(envelope)?)
    } else if envelope["ok"] == false {
        writeln!(
            io::stderr().lock(),
            "{}
{}",
            envelope["error"]["message"].as_str().unwrap_or_default(),
            envelope["error"]["details"].as_str().unwrap_or_default()
        )
    } else {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", text(en, "結果", "Result"))?;
        render(&mut out, &envelope["data"], en, 0)?;
        for warning in envelope["warnings"].as_array().into_iter().flatten() {
            writeln!(out, "{}", warning.as_str().unwrap_or_default())?;
        }
        Ok(())
    }
}
fn render(out: &mut impl Write, value: &Value, en: bool, depth: usize) -> io::Result<()> {
    match value {
        Value::Object(items) => {
            for (key, value) in items {
                let label = match key.as_str() {
                    "active_pack_id" => text(en, "適用中のパック", "Active pack"),
                    "preserve_existing_icons" => {
                        text(en, "既存のアイコンを保護する", "Protect existing icons")
                    }
                    "icon_exclusions" => text(en, "除外された種類", "Excluded file types"),
                    "extension" => text(en, "拡張子", "Extension"),
                    "class" => text(en, "関連付けクラス", "Association class"),
                    "confirmation" => text(en, "確認用識別子", "Confirmation"),
                    "strategy" => text(en, "復旧方法", "Recovery strategy"),
                    "backup" => text(en, "退避先", "Backup"),
                    "conflicts" => text(en, "外部変更", "External changes"),
                    "writes" => text(en, "書き換え件数", "Writes"),
                    "overwrites" => text(en, "上書き件数", "Overwrites"),
                    "resuming" => text(en, "途中処理から再開", "Resuming"),
                    "conflict_recovery_pending" => text(en, "登録復旧が未完了", "Recovery pending"),
                    "recovered" => text(en, "復旧完了", "Recovered"),
                    "entries" => text(en, "対象登録", "Registration entries"),
                    "current" => text(en, "現在値", "Current"),
                    "recorded" => text(en, "記録の適用値", "Recorded applied value"),
                    "original" => text(en, "解除先", "Restore baseline"),
                    "planned" => text(en, "処理後", "Planned"),
                    "packs" => text(en, "パック", "Packs"),
                    "registration_state" => text(en, "登録状態", "Registration state"),
                    "pending_pack_removals" => text(en, "削除保留", "Pending removals"),
                    "provenance" => text(en, "出典・利用条件", "Provenance and licenses"),
                    "warning_count" => text(en, "警告の件数", "Warning count"),
                    "reason" => text(en, "選択理由", "Resolution reason"),
                    "protected" => text(
                        en,
                        "Windowsが管理する保護対象",
                        "Protected file type managed by Windows",
                    ),
                    "dry_run" => text(en, "変更予定のみ", "Dry run"),
                    "saved_selection" => text(en, "保存された選択", "Saved selection"),
                    "planned_selection" => text(en, "記録された選択", "Recorded selection"),
                    "selected" => text(en, "選択", "Selected"),
                    "additions" => text(en, "追加", "Additions"),
                    "removals" => text(en, "解除", "Removals"),
                    "group_errors" => text(en, "登録の確認結果", "Registration errors"),
                    "recovery_pending" => text(en, "復旧が必要", "Recovery pending"),
                    "help" => text(en, "使い方", "Usage"),
                    "catalog" => text(en, "公式パック", "Catalog"),
                    "id" => text(en, "識別子", "ID"),
                    "name" | "label" => text(en, "名称", "Name"),
                    "version" => text(en, "版", "Version"),
                    "author" => text(en, "作者", "Author"),
                    "themes" => text(en, "テーマ", "Themes"),
                    "license" | "licenses" => text(en, "利用条件", "Licenses"),
                    "source" => text(en, "出典", "Source"),
                    "storage" => text(en, "保存先", "Storage"),
                    "plan" => text(en, "変更予定", "Plan"),
                    "pack" | "pack_id" => text(en, "パック", "Pack"),
                    "pack_ids" => text(en, "パック識別子", "Pack IDs"),
                    "icon" => text(en, "画像", "Icon"),
                    "filename" => text(en, "ファイル名", "Filename"),
                    "applied" => text(en, "適用済み", "Applied"),
                    "restored" => text(en, "復元済み", "Restored"),
                    "cleaned" => text(en, "整理済み", "Cleaned"),
                    "registration_changes" => text(en, "登録変更数", "Registration changes"),
                    "classes" => text(en, "関連付けクラス", "Association classes"),
                    "previous_pack_id" => text(en, "切替前のパック", "Previous pack"),
                    "restore_changes" => text(en, "復元対象の変更", "Restoration changes"),
                    "removed_pack_id" | "remove_pack_id" => text(en, "削除対象", "Removal target"),
                    "shell_candidates" => text(en, "配置済みDLLの候補", "Staged DLL candidates"),
                    "shell_source" => text(en, "DLLの導入元", "Shell DLL source"),
                    "keep_shared_storage" => text(en, "共有素材を保持", "Keep shared storage"),
                    "recheck_on_execution" | "verify_ownership_on_execution" => {
                        text(en, "実行時に再検証", "Recheck on execution")
                    }
                    "backup_settings_if_unreadable" => text(
                        en,
                        "設定が読めない場合に退避",
                        "Back up unreadable settings",
                    ),
                    _ => key,
                };
                writeln!(out, "{}{}:", "  ".repeat(depth), label)?;
                render(out, value, en, depth + 1)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                render(out, item, en, depth)?;
            }
        }
        Value::String(value) => writeln!(
            out,
            "{}{}",
            "  ".repeat(depth),
            if value == "default_icon" {
                text(
                    en,
                    "既存のアイコン画像が指定されています",
                    "An existing icon image is registered",
                )
            } else if value == "icon_handler" {
                text(
                    en,
                    "既存の専用アイコン処理があります",
                    "An existing icon handler is registered",
                )
            } else if value == "protected_file_type" {
                text(
                    en,
                    "Windowsが管理する保護対象",
                    "Protected file type managed by Windows",
                )
            } else {
                value
            }
        )?,
        _ => writeln!(out, "{}{}", "  ".repeat(depth), value)?,
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    let raw_os = std::env::args_os().skip(1).collect::<Vec<_>>();
    let valid_unicode = raw_os.iter().all(|s| s.to_str().is_some());
    let raw = raw_os
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let options_end = raw.iter().position(|v| v == "--").unwrap_or(raw.len());
    let options = &raw[..options_end];
    let mut json_output = options.iter().any(|v| v == "--json");
    let explicit_en = options
        .windows(2)
        .find(|v| v[0] == "--lang")
        .is_some_and(|v| v[1] == "en");
    let parsed = if valid_unicode {
        Args::parse(&raw)
    } else {
        Err(anyhow::anyhow!("argument is not valid Unicode"))
    };
    let mut en = explicit_en;
    let independent = parsed.as_ref().is_ok_and(|args| {
        matches!(
            args.command.as_str(),
            "help" | "version" | "catalog list" | "catalog show" | "pack inspect"
        )
    });
    // 状態を読む場合も同じ排他で言語を取得し、保存途中の組み合わせを読まない。
    // 管理操作の言語は、下の操作用ロックを取得してから一度だけ読む。
    if (parsed.is_err() || independent)
        && !options.iter().any(|v| v == "--lang")
        && file_icons_windows::home().is_ok_and(|path| path.is_dir())
        && let Ok(_manager) = Manager::acquire()
    {
        en = file_icons_windows::load()
            .map(|s| s.language == "en")
            .unwrap_or(false);
    }
    let mut registration_notice = false;
    let result = match parsed {
        Err(error) => Err(("invalid_arguments", 2, error)),
        Ok(args) => {
            json_output = args.json;
            registration_notice = matches!(
                args.command.as_str(),
                "apply" | "restore" | "recover" | "thumbnails set" | "thumbnails restore"
            ) && !args.dry_run;
            let progress = Arc::new(Progress::default());
            (|| -> Result<Value> {
                let _console = initialize(progress.clone())?;
                if independent {
                    return commands::independent(&args, en);
                }
                let manager = Manager::acquire()?;
                if args.language.is_none() {
                    en = file_icons_windows::load()
                        .map(|s| s.language == "en")
                        .unwrap_or(false);
                }
                commands::run(&manager, &args, progress, en)
            })()
            .map_err(|error| {
                let (code, exit) = classify(&error);
                (code, exit, error)
            })
        }
    };
    let (envelope, exit) = match result {
        Ok(data) => {
            let mut warnings: Vec<String> = if registration_notice {
                vec![text(
                    en,
                    "表示が更新されない場合は、ファイル操作を終えてからExplorerを再起動してください。",
                    "If the display does not update, finish file operations and restart Explorer.",
                ).into()]
            } else {
                vec![]
            };
            if data["warning_count"]
                .as_u64()
                .is_some_and(|count| count > 0)
            {
                warnings.push(text(
                    en,
                    "未定義のアイコンを参照する規則を除外しました。pack showで除外した規則と出典を確認してください。",
                    "Rules referencing undefined icons were skipped. Use pack show to inspect the skipped rules and provenance.",
                ).into());
            }
            (
                json!({"schema_version":1,"ok":true,"data":data,"warnings":warnings}),
                0,
            )
        }
        Err((code, exit, error)) => (
            json!({"schema_version":1,"ok":false,"error":{"code":code,"message":message(code,en),"details":format!("{error:#}")}}),
            exit,
        ),
    };
    if emit(json_output, &envelope, en).is_err() {
        return 5.into();
    }
    exit.into()
}

#[cfg(test)]
mod tests;
