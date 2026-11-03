# Command-line management

On Windows 11 x64, `file-icons.exe` uses the same pack library, lock, settings and registration/recovery operations as the GUI. Keep the CLI, GUI and `file_icons_shell.dll` together in the extracted distribution. Close the GUI before commands that access saved state. Catalog queries, local inspection, help and version remain available while the GUI is running.

```powershell
.\file-icons.exe --help --lang en
.\file-icons.exe catalog list --json
.\file-icons.exe catalog show vscode-icons
.\file-icons.exe pack download vscode-icons
.\file-icons.exe pack list --json
.\file-icons.exe pack inspect "C:\themes\my theme"
.\file-icons.exe pack import "C:\themes\my theme" --theme "theme-id"
```

Downloads use pinned official releases with the same destination restrictions, hashes and resource limits as the GUI. Arbitrary URLs and verification bypasses are not supported. Downloading or importing does not apply icons. No programs from imported extensions are executed. Multiple themes require an explicit theme ID.

Rules referencing undefined icons are skipped with a localized warning and `data.warning_count` in download/import results. Pack summaries in `pack list` and `pack show` also include `warning_count`. The complete records are available in `data.provenance.warnings` from `pack show --json`, with stable `rule_type`, `rule`, and `icon_id` keys. A successful partial import exits with code 0 and remains unapplied. Themes with no supported rules left, missing images, or failed safety checks are rejected.

Replace `<pack-id>` with the complete ID returned by `pack list`; do not type the angle brackets literally.

```powershell
.\file-icons.exe pack show "<pack-id>" --json
.\file-icons.exe resolve "<pack-id>" "Cargo.toml" --json
.\file-icons.exe apply "<pack-id>" --dry-run
.\file-icons.exe apply "<pack-id>"
.\file-icons.exe status --json
.\file-icons.exe restore --dry-run
.\file-icons.exe restore
```

`pack show` preserves the complete stored provenance and license texts. `resolve` does not open the named file: it uses Explorer's resolver, including checking whether the selected icon exists. Reasons are `file_name`, `file_extension`, `language_file_name`, `language_pattern`, `language_extension`, or `windows_default` when no valid theme icon is returned.

`apply` also switches packs. `restore` reverses this application's registration changes while preserving external edits and default applications. When settings cannot be read, normal operations stop. Use `recover --dry-run`, then `recover`, only when an independent recovery record is available. Settings are backed up; a readable separate library and language are retained. Unsupported settings formats are not automatically overwritten; use a compatible version to review them before recovery if you need to retain their contents.

```powershell
.\file-icons.exe pack remove "<pack-id>" --dry-run
.\file-icons.exe pack remove "<pack-id>"
.\file-icons.exe cleanup --dry-run
.\file-icons.exe cleanup
```

Removal deletes the library entry and owned generated assets used exclusively by that pack. It preserves source folders and refuses active packs. Failed cleanup remains recorded. After restoring registration, `cleanup` retries pending asset removal and cleans unused staged DLLs.

`--dry-run` is available for `apply`, `restore`, `recover`, `pack remove` and `cleanup`. It does not save settings, convert icons, alter registration or delete assets. Coordination may create the management folder and lock file. Plans describe affected classes, existing restoration records and shared storage; execution rechecks ownership, content, permissions and conflicts. A successful preview does not guarantee later execution will succeed.

Explorer is never terminated automatically. If icons do not refresh, finish file operations and restart Explorer through Task Manager.

## Output and automation

`--json` writes exactly one JSON document to stdout; download progress goes to stderr. Success has `schema_version: 1`, `ok: true`, `data` and `warnings`. Failure has `schema_version: 1`, `ok: false`, and `error` containing stable `code`, localized `message` and diagnostic `details`. Parse codes rather than messages. Keys, identifiers and state values are never translated.

`--lang ja|en` changes only this invocation, without saving the GUI preference. Otherwise the saved language is read under the management lock; unavailable, damaged or busy settings fall back to Japanese. Underlying diagnostics retain their original language. Use `--` before literal positional arguments. Unknown commands/options, extra arguments and duplicate options fail. The CLI never waits for interactive input or elevation.

When registration preflight detects a conflict, GUI and CLI diagnostics identify the HKCU key, value name, and settings file path. The default value is shown as `(Default)`. Preflight does not change registration. Preserve external changes and recovery records; rebooting or cleaning build artifacts does not reset saved registration state.

| Exit code | Meaning | Common error.code |
| --- | --- | --- |
| 0 | Success, help or preview | — |
| 2 | Invalid arguments | invalid_arguments |
| 3 | Management lock busy | manager_busy |
| 4 | Missing target or unmet precondition | not_found / precondition_failed |
| 5 | Network, I/O, conversion or pending cleanup | operation_failed / cleanup_pending |
| 6 | Registration conflict | registration_conflict |
| 7 | Settings/registration recovery required | recovery_required |
| 130 | Cancelled | cancelled |

A successful `status` query returns `registration_state` as `inactive`, `active` or `recovery_required`, with exit code 0 even when it reports a problem. Unreadable settings return 7. `pending_pack_removals` lists cleanup requests.

Ctrl+C / Ctrl+Break request cooperative cancellation. Downloads and conversion stop at safe checkpoints without adding incomplete packs. Registration, restoration and deletion finish their current safe transaction; a completed operation returns success. Forced termination relies on the independent recovery record.

Actual CLI subprocess tests cover parsing, JSON, exit codes, language, locking, import, resolution, removal and previews that preserve state. Registration/switching/conflict/recovery tests redirect the registry into a process-specific namespace. GUI tests continue to use GPUI test-support. Run heavy commands under the shared CPU lock described in the development guide.

```powershell
cargo test -p file-icons-cli --locked
cargo test -p file-icons-cli --test cli --locked -- --ignored --exact official_downloads_through_real_cli_keep_registration_inactive
```

The second command explicitly downloads all three official packs. Automated isolated checks do not replace acceptance in real Explorer.

## Confirmed conflict recovery

Use `conflicts list --json` to inspect all entries and external changes. Preview one of the following strategies before executing it:

```powershell
.\file-icons.exe recover --strategy preserve-external --dry-run --json
.\file-icons.exe recover --strategy reapply-theme --dry-run --json
.\file-icons.exe recover --strategy preserve-external --confirm <confirmation> --json
.\file-icons.exe recover --strategy reapply-theme --confirm <confirmation> --json
```

Replace `<confirmation>` with `data.plan.confirmation` from the matching preview. `preserve-external` removes the app's registration while retaining external and already restored values. `reapply-theme` reapplies the recorded pack and saved selection. External values become the new restore baseline, so a later normal restore returns to those values; unchanged entries retain their earlier baseline. Unsaved GUI drafts are never applied implicitly.

The confirmation binds the strategy, settings, recovery records, observed registry values, pack/version/selection and bundled DLL contents. Missing confirmation is an argument error (exit 2); a stale or mismatched confirmation is a registration conflict (exit 6). Incomplete recovery or invalid records require recovery (exit 7). Preview again before retrying. Existing apply/restore/recover calls retain their conflict checks; no unconditional force flag is provided.

Plans include `schema_version`, `confirmation`, `strategy`, `pack_id`, `pack_version`, `selection`, `conflicts`, `writes`, `overwrites`, `backup`, `resuming` and `entries`. Entry states are `external`, `applied`, `original` or `new`. `current` is the observed value, `recorded` is the recorded applied value, `original` is the restore baseline and `planned` is the intended result. Registry values preserve `kind` and raw `bytes`; absent values are null. `conflicts list` returns the plan directly, while recover returns it in `plan` alongside `dry_run` and `recovered`. JSON identifiers remain language independent.

Before registry writes, inputs and the plan are durably copied under `conflict-backups/<confirmation>/`, and `conflict-operation.json` records the operation. Changes are rechecked before execution, each registry write, settings save and completion. An incomplete operation blocks normal mutations and can be reviewed and resumed after restarting. Backups are retained. Unknown formats and invalid scopes cannot be bypassed. Default apps, open commands, file contents and protected `.lnk`/`.exe` classes remain protected. `status.conflict_recovery_pending` reports an incomplete operation. See the [detailed recovery contract in Japanese](conflict-recovery.ja.md).

## Select icon targets

`targets list <pack-id> --json` returns the stable target identifiers, categories, effective enabled state and explicit overrides. Use `targets edit <pack-id> <scope> <on|off|inherit>` to edit all targets, a category such as `category:data`, a name search such as `query:.json`, or an exact identifier such as `file:package.json`. `inherit` removes the explicit override. A filename override takes priority over its extension. Disabled filenames use the original Windows icon rather than falling through to another themed icon.

`targets set <pack-id> changes.json` accepts a UTF-8 object mapping identifiers to true, false, or null (inherit), up to 1 MiB. Unknown identifiers reject the whole change. `targets copy <source-id> <destination-id>` copies matching identifiers and reports unmatched targets. These change commands support `--dry-run`. An inactive pack is saved without applying it; changes to an active pack are applied using the bundled DLL. Disabling every target restores the previous registration. `resolve` also returns `selection_enabled` to distinguish a disabled target from an unsupported filename.

## Protected file types

`protection show` returns the application-wide `preserve_existing_icons` setting, enabled by default. `protection exclusions <pack-id>` lists the exclusions expected on the next apply. Each entry contains `extension`, `class`, `reason` and `source`; an empty extension represents extensionless files and name patterns. Reasons are `default_icon`, `icon_handler`, `protected_file_type` and `changed_registration`. `protection set on|off [--dry-run]` saves the setting and reapplies an active pack using its original registration baseline. Dry runs do not change settings or registration. Existing image references and handlers on the class or extensions sharing that effective class protect the shared class, including Windows registrations. Unknown setting owners are not inferred. External conflicts retain the existing recovery flow. Runtime format 4, library format 4 and recovery format 2 accept older formats; loading alone does not change registration. `resolve` describes theme and target rules; use `protection exclusions` for registration exclusions.

`.lnk` and `.exe` always retain their Windows-managed icons. Their target checkboxes are disabled; bulk edits and copies cannot enable them. Explicit CLI enabling fails without saving any changes. `targets list --json` exposes `protected`; `resolve --json` returns `reason: protected_file_type`. To remove registrations left by an older version, explicitly apply the theme again. Existing external changes are reported as conflicts. Restart Explorer if its old DLL or icon cache remains in use. Launching the app alone does not change registration.

Provider and file type exceptions saved in the GUI are respected by apply, recovery and `protection exclusions`. `protection set on|off` changes the default policy while retaining those exceptions. A changed or removed registration requires review before a previous overwrite permission can be used again.
