use anyhow::{Context, Result, ensure};

pub struct Args {
    pub json: bool,
    pub language: Option<String>,
    pub dry_run: bool,
    pub command: String,
    pub values: Vec<String>,
    pub theme: Option<String>,
    pub strategy: Option<file_icons_windows::conflict::Strategy>,
    pub confirmation: Option<String>,
}
impl Args {
    pub fn parse(raw: &[String]) -> Result<Self> {
        let mut result = Self {
            json: false,
            language: None,
            dry_run: false,
            command: String::new(),
            values: Vec::new(),
            theme: None,
            strategy: None,
            confirmation: None,
        };
        let mut values = Vec::new();
        let mut args = raw.iter();
        let mut literal = false;
        let mut help = false;
        let mut version = false;
        while let Some(arg) = args.next() {
            if literal {
                values.push(arg.clone());
                continue;
            }
            match arg.as_str() {
                "--" => literal = true,
                "--json" => {
                    ensure!(!result.json, "duplicate --json");
                    result.json = true;
                }
                "--dry-run" => {
                    ensure!(!result.dry_run, "duplicate --dry-run");
                    result.dry_run = true;
                }
                "--lang" => {
                    ensure!(result.language.is_none(), "duplicate --lang");
                    let value = args
                        .next()
                        .filter(|v| matches!(v.as_str(), "ja" | "en"))
                        .ok_or_else(|| anyhow::anyhow!("--lang ja|en"))?;
                    result.language = Some(value.clone());
                }
                "--theme" => {
                    ensure!(result.theme.is_none(), "duplicate --theme");
                    result.theme = Some(
                        args.next()
                            .filter(|v| !v.starts_with('-'))
                            .ok_or_else(|| anyhow::anyhow!("--theme <id>"))?
                            .clone(),
                    );
                }
                "--strategy" => {
                    ensure!(result.strategy.is_none(), "duplicate --strategy");
                    result.strategy = Some(match args.next().map(String::as_str) {
                        Some("preserve-external") => {
                            file_icons_windows::conflict::Strategy::PreserveExternal
                        }
                        Some("reapply-theme") => {
                            file_icons_windows::conflict::Strategy::ReapplyTheme
                        }
                        _ => anyhow::bail!("--strategy preserve-external|reapply-theme"),
                    });
                }
                "--confirm" => {
                    ensure!(result.confirmation.is_none(), "duplicate --confirm");
                    let id = args.next().context("--confirm <id>")?;
                    ensure!(
                        id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
                        "invalid confirmation id"
                    );
                    result.confirmation = Some(id.clone());
                }
                "--help" | "-h" => help = true,
                "--version" | "-V" => version = true,
                _ => {
                    ensure!(!arg.starts_with('-'), "unknown option: {arg}");
                    values.push(arg.clone());
                }
            }
        }
        if help || values.is_empty() || version {
            result.command = if version { "version" } else { "help" }.into();
            ensure!(
                !result.dry_run
                    && result.theme.is_none()
                    && result.strategy.is_none()
                    && result.confirmation.is_none(),
                "unexpected option"
            );
            return Ok(result);
        }
        let head = values.remove(0);
        result.command = if matches!(
            head.as_str(),
            "pack" | "catalog" | "targets" | "conflicts" | "protection" | "thumbnails"
        ) {
            ensure!(!values.is_empty(), "missing subcommand");
            format!("{head} {}", values.remove(0))
        } else {
            head
        };
        let count = match result.command.as_str() {
            "status" | "catalog list" | "pack list" | "restore" | "recover" | "cleanup"
            | "conflicts list" | "thumbnails show" | "thumbnails restore" => 0,
            "catalog show" | "pack show" | "pack download" | "pack inspect" | "pack import"
            | "pack remove" | "apply" | "thumbnails set" => 1,
            "resolve" => 2,
            "targets list" => 1,
            "protection show" => 0,
            "protection set" | "protection exclusions" => 1,
            "targets set" | "targets copy" => 2,
            "targets edit" => 3,
            _ => anyhow::bail!("unknown command: {}", result.command),
        };
        ensure!(values.len() == count, "expected {count} arguments");
        ensure!(
            result.strategy.is_none() || result.command == "recover",
            "--strategy requires recover"
        );
        ensure!(
            result.confirmation.is_none()
                || (result.command == "recover" && result.strategy.is_some() && !result.dry_run),
            "--confirm requires recover --strategy without --dry-run"
        );
        ensure!(
            result.strategy.is_none() || result.dry_run || result.confirmation.is_some(),
            "preview with --dry-run, then supply --confirm"
        );
        ensure!(
            result.theme.is_some() == (result.command == "pack import"),
            "pack import requires --theme; other commands do not accept it"
        );
        ensure!(
            !result.dry_run
                || matches!(
                    result.command.as_str(),
                    "apply"
                        | "restore"
                        | "recover"
                        | "cleanup"
                        | "pack remove"
                        | "targets set"
                        | "targets copy"
                        | "targets edit"
                        | "protection set"
                        | "thumbnails set"
                        | "thumbnails restore"
                ),
            "--dry-run is not supported by this command"
        );
        if result.command == "targets edit" {
            ensure!(
                matches!(values[2].as_str(), "on" | "off" | "inherit"),
                "expected on|off|inherit"
            );
        }
        if result.command == "protection set" {
            ensure!(
                matches!(values[0].as_str(), "on" | "off"),
                "expected on|off"
            );
        }
        result.values = values;
        Ok(result)
    }
}
