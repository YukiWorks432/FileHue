use super::*;
use file_icons_catalog::{CATALOG, Progress};
use gpui_kit::{
    assets::IconName,
    component::{Icon, Sizable, input::Input},
};
use std::{io::Read as _, sync::Arc};

impl Manager {
    fn download_pack(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.operation.busy() || !self.valid {
            return;
        }
        let Some(entry) = CATALOG.get(index) else {
            return;
        };
        let progress = Arc::new(Progress::default());
        let provider = self.download_source;
        let display_progress = progress.clone();
        let Some(manager) = self._guard.clone() else {
            return;
        };
        self.start(
            Kind::Download(entry.id),
            move || {
                manager
                    .download(entry.id, progress.clone(), provider)
                    .map(|_| ())
            },
            false,
            Some(display_progress),
            cx,
        );
    }

    pub(super) fn catalog(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let mut section = div().flex().flex_col().gap_3().child(
            div()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(self.text(
                    "公式配布元の確認済みバージョンです。取得後に適用できます。",
                    "Reviewed versions from official releases. Download, then apply.",
                )),
        );
        for (index, entry) in CATALOG.iter().enumerate() {
            let installed = entry.themes.iter().all(|theme| {
                self.settings
                    .packs
                    .iter()
                    .any(|pack| pack.id == entry.pack_id(theme))
            });
            let older = self.settings.packs.iter().any(|pack| {
                pack.id
                    .starts_with(&format!("{}.{}", entry.publisher, entry.package))
                    && pack.version != entry.version
            });
            let license = if entry.id == "vscode-icons" && self.settings.language != "en" {
                "コード: MIT ／ アイコン: CC BY-SA 4.0 ／ ブランドのアイコン: 各権利者の条件"
            } else {
                entry.license
            };
            let url = entry.source_url();
            section = section.child(
                theme::card(cx)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(format!(
                        "{} · {} · {}",
                        entry.name, entry.version, entry.author
                    ))
                    .child(div().text_sm().child(license))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                theme::button(("download", index), cx)
                                    .small()
                                    .icon(if installed {
                                        IconName::Check
                                    } else {
                                        IconName::Download
                                    })
                                    .label(if installed {
                                        self.text("取得済み", "Downloaded")
                                    } else if older {
                                        self.text("更新をダウンロード", "Download update")
                                    } else if self.operation.retry_download(entry.id) {
                                        self.text("ダウンロードを再試行", "Retry download")
                                    } else {
                                        self.text("ダウンロード", "Download")
                                    })
                                    .disabled_elevated(
                                        self.operation.busy() || !self.valid || installed,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.download_pack(index, cx)
                                    })),
                            )
                            .child(
                                theme::button(("source", index), cx)
                                    .small()
                                    .icon(IconName::ExternalLink)
                                    .accessibility_label(self.text(
                                        "出典・利用条件を開く",
                                        "Open source and license information",
                                    ))
                                    .tooltip(format!(
                                        "{} · {}",
                                        self.text("出典・利用条件", "Source and licenses"),
                                        entry.repository
                                    ))
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                            ),
                    ),
            );
        }
        section
    }

    pub(super) fn select_details(&mut self, id: &str, cx: &mut Context<Self>) {
        // 同じ操作で閉じる。読込中でも閉じられ、後着の結果で再展開しない。
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
            self.rights.clear();
            self.show_rights = false;
            cx.notify();
            return;
        }
        if self.operation.busy() {
            return;
        }
        let Some(pack) = self.settings.packs.iter().find(|pack| pack.id == id) else {
            return;
        };
        let path = pack.provenance.clone();
        self.selected = Some(id.to_owned());
        self.show_rights = false;
        self.rights.clear();
        self.operation.begin(Kind::Details, None);
        let guard = self._guard.clone();
        let task = cx.background_executor().spawn(async move {
            let _guard = guard;
            read_rights(&path)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(rights) => {
                        if this.selected.is_some() {
                            this.rights = rights;
                        }
                        this.operation.finish(Status::Complete);
                    }
                    Err(error) => {
                        if this.selected.is_some() {
                            this.rights = format!("{error:#}");
                        }
                        this.operation
                            .finish(Status::Error(Failure::ReadRights, format!("{error:#}")));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn pack_details(&self, cx: &mut Context<Self>) -> Option<Div> {
        let id = self.selected.as_ref()?;
        let pack = self.settings.packs.iter().find(|pack| &pack.id == id)?;
        let typed = self.preview.read(cx).value();
        let colors = cx.theme().colors;
        let mut details = theme::inset(cx)
            .flex_shrink_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(format!("{} · {}", pack.label, pack.version));
        if !pack.warnings.is_empty() {
            details = details.child(self.text(
                "警告: 次の規則はアイコンが未定義のため除外しました（先頭20件）。完全な記録は出典ファイルに保存しています。",
                "Warning: these rules reference undefined icons and were skipped (first 20). The provenance file contains the complete record.",
            ));
            for warning in pack.warnings.iter().take(20) {
                let kind = match warning.rule_type.as_str() {
                    "fileNames" => self.text("ファイル名", "Filename"),
                    "fileExtensions" => self.text("拡張子", "Extension"),
                    _ => self.text("言語", "Language"),
                };
                details = details.child(
                    div()
                        .text_sm()
                        .child(format!("{kind}: {} → {}", warning.rule, warning.icon_id)),
                );
            }
        }
        for (index, name) in [
            "Cargo.toml",
            "settings.toml",
            "package.json",
            "data.json",
            "README.md",
            "notes.md",
        ]
        .into_iter()
        .enumerate()
        {
            details = details.child(self.preview_row(name, pack, format!("example-{index}"), cx));
        }
        details = details
            .child(
                div()
                    .text_sm()
                    .child(self.text("ファイル名を入力して確認", "Try a filename")),
            )
            .child(
                Input::new(&self.preview)
                    .id("preview-name")
                    .w_full()
                    .bg(colors.popover),
            );
        if !typed.is_empty() {
            details = details.child(self.preview_row(&typed, pack, "typed-preview".into(), cx));
        }
        details = details.child(
            theme::button("toggle-rights", cx)
                .small()
                .icon(if self.show_rights {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .label(self.text(
                    "出典・利用条件（原文）",
                    "Source and licenses (original text)",
                ))
                .selected(self.show_rights)
                .toggled(self.show_rights)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_rights = !this.show_rights;
                    cx.notify();
                })),
        );
        if self.show_rights {
            let rights: String = self.rights.chars().take(32_000).collect();
            let license = div()
                .id("pack-rights")
                .role(Role::Status)
                .aria_label(rights.clone())
                .text_sm()
                .whitespace_normal()
                .child(rights);
            #[cfg(feature = "test-support")]
            let license = license.test_support();
            details = details
                .child(
                    div()
                        .id("license-scroll")
                        .h_48()
                        .overflow_y_scroll()
                        .p_3()
                        .border_t_1()
                        .border_color(colors.border)
                        .child(license),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child(format!(
                            "{}: {}",
                            self.text("完全な記録", "Full provenance"),
                            pack.provenance.display()
                        )),
                );
        }
        Some(details)
    }

    fn preview_row(
        &self,
        name: &str,
        pack: &file_icons_core::ImportedPack,
        id: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let empty = file_icons_core::selection::Selection::default();
        let selection = self.settings.selections.get(&pack.id).unwrap_or(&empty);
        let icon = selection.resolve(&pack.theme, name).map(|(icon, _)| icon);
        let label = match icon {
            Some(path) => format!(
                "{name} · {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            None => format!(
                "{name} · {}",
                self.text("Windowsの通常のアイコン", "Windows default icon")
            ),
        };
        let artwork = match icon {
            Some(path) => {
                crate::icon_preview::preview(path.to_path_buf(), rems(2.)).into_any_element()
            }
            None => Icon::new(IconName::File).size_8().into_any_element(),
        };
        let mut description = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(name.to_owned());
        if icon.is_none() {
            let message = self.text("Windowsの通常のアイコン", "Windows default icon");
            let hint = div()
                .id("fallback-hint")
                .role(Role::Label)
                .aria_label(message)
                .text_sm()
                .child(message);
            #[cfg(feature = "test-support")]
            let hint = hint.test_support();
            description = description.child(hint);
        }
        let pack_id = pack.id.clone();
        let filename = name.to_owned();
        let row = div()
            .id(SharedString::from(id.clone()))
            .role(Role::Status)
            .aria_label(label)
            .flex()
            .items_center()
            .flex_wrap()
            .gap_3()
            .child(artwork)
            .child(description.flex_1())
            .child(
                theme::button(SharedString::from(format!("check-{id}")), cx)
                    .small()
                    .label(self.text("表示予定を確認", "Check planned icon"))
                    .disabled_elevated(self.operation.busy() || !self.valid)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_preview_outcome(&pack_id, filename.clone(), window, cx);
                    })),
            );
        #[cfg(feature = "test-support")]
        let row = row.test_support();
        row.into_any_element()
    }
}

/// 表示上限に達した後の本文は文字列へ組み立てない。
fn read_rights(path: &std::path::Path) -> Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 16 * 1024 * 1024, "provenance too large");
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut output = String::new();
    let mut remaining = 32_000usize;
    let mut append = |text: &str| {
        for ch in text.chars().take(remaining) {
            output.push(ch);
            remaining -= 1;
        }
    };
    append(value["package"]["license"].as_str().unwrap_or(""));
    for key in [
        "url",
        "sha256",
        "repository",
        "version",
        "license_summary",
        "conversion",
    ] {
        if let Some(text) = value["download"][key].as_str() {
            append("\n");
            append(key);
            append(": ");
            append(text);
        }
    }
    if let Some(licenses) = value["licenses"].as_object() {
        for (name, text) in licenses {
            append("\n");
            append(name);
            append("\n");
            append(text.as_str().unwrap_or(""));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rights_view_caps_unicode_characters_and_keeps_the_complete_source() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let text = "あ".repeat(40_000);
        std::fs::write(
            file.path(),
            serde_json::to_vec(
                &serde_json::json!({"package":{"license":"MIT"},"licenses":{"NOTICE":text}}),
            )
            .unwrap(),
        )
        .unwrap();
        let before = std::fs::read(file.path()).unwrap();
        let rendered = super::read_rights(file.path()).unwrap();
        assert_eq!(rendered.chars().count(), 32_000);
        assert!(rendered.starts_with("MIT\nNOTICE\nあ"));
        assert_eq!(std::fs::read(file.path()).unwrap(), before);
    }
}
