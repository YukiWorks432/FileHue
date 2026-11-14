use super::*;
use file_icons_catalog::{CATALOG, Progress};
use gpui_kit::base::Link;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    assets::IconName,
    component::{Icon, Sizable, button::ButtonVariants, input::Input, tag::Tag},
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

    pub(super) fn catalog(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let compact = window.viewport_size().width < window.rem_size() * 48.;
        let mut list = theme::card(cx).flex().flex_col();
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
            let downloading = self.operation.running(Kind::Download(entry.id));
            let failed = self.operation.retry_download(entry.id);
            let license = if entry.id == "vscode-icons" && self.settings.language != "en" {
                "コード: MIT ／ アイコン: CC BY-SA 4.0 ／ ブランドのアイコン: 各権利者の条件"
            } else {
                entry.license
            };
            let url = entry.source_url();
            let source_label = self.text("出典・利用条件", "Source and licenses");
            let source = Link::new(("source", index))
                .href(url)
                .open_with(|url, _, _, cx| cx.open_url(url))
                .accessibility_label(source_label)
                .text_color(colors.muted_foreground)
                .text_decoration_1()
                .cursor_pointer()
                .focus_visible(|style| style.border_1().border_color(colors.ring))
                .flex()
                .items_center()
                .gap_1()
                .child(source_label)
                .child(Icon::new(IconName::ExternalLink).size_4());
            let source = div().id(("source-hit", index)).child(source);
            #[cfg(feature = "test-support")]
            let source = source.test_support();
            let label = if downloading {
                self.text("取得中", "Downloading")
            } else if failed {
                self.text("再試行", "Retry download")
            } else if installed {
                self.text("取得済み", "Downloaded")
            } else if older {
                self.text("更新を取得", "Download update")
            } else {
                self.text("ダウンロード", "Download")
            };
            let mut controls = div().flex().items_center().justify_center().w(rems(11.));
            if downloading {
                let status = div()
                    .id(("catalog-state", index))
                    .role(Role::Status)
                    .aria_label(label)
                    .child(
                        Tag::info().small().child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(gpui_kit::component::spinner::Spinner::new().small())
                                .child(label),
                        ),
                    );
                #[cfg(feature = "test-support")]
                let status = status.test_support();
                controls = controls.child(status);
            } else if installed && !failed {
                let status = div()
                    .id(("catalog-state", index))
                    .role(Role::Status)
                    .aria_label(label)
                    .child(Tag::success().small().child(format!("✓ {label}")));
                #[cfg(feature = "test-support")]
                let status = status.test_support();
                controls = controls.child(status);
            } else {
                controls = controls.child(
                    theme::button(("download", index), cx)
                        .small()
                        .primary()
                        .min_w(rems(10.))
                        .icon(IconName::Download)
                        .label(label)
                        .disabled_elevated(self.operation.busy() || !self.valid)
                        .on_click(cx.listener(move |this, _, _, cx| this.download_pack(index, cx))),
                );
            }
            let info = div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{} · {}", entry.name, entry.version)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child(entry.author),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .min_w_0()
                                .flex_shrink(1.)
                                .when(compact, |text| text.w_full())
                                .child(license),
                        )
                        .child(source),
                )
                .when(failed, |info| {
                    info.child(div().text_sm().text_color(colors.warning_foreground).child(
                        self.text(
                            "取得に失敗しました。詳細を確認して再試行してください。",
                            "Download failed. Check the details and try again.",
                        ),
                    ))
                });
            let row = div()
                .px_3()
                .py_1()
                .flex()
                .flex_wrap()
                .items_center()
                .when(compact, |row| row.flex_col().items_start())
                .gap_3()
                .when(index + 1 < CATALOG.len(), |row| {
                    row.border_b_1().border_color(colors.border)
                })
                .child(info)
                .child(controls);
            list = list.child(row);
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(self.text(
                        "取得したパックは「取得済み」から確認・適用できます。",
                        "View and apply downloaded packs from Downloaded packs.",
                    )),
            )
            .child(list)
    }

    pub(super) fn select_details(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        // 同じ操作で閉じる。読込中でも閉じられ、後着の結果で再展開しない。
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
            self.rights = None;
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
        self.rights = None;
        self.operation.begin(Kind::Details, None);
        let guard = self._guard.clone();
        let task = cx.background_executor().spawn(async move {
            let _guard = guard;
            read_rights(&path)
        });
        let window = window.window_handle();
        let selected = id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = cx.update_window(window, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(rights) => {
                            if this.selected.as_deref() == Some(&selected) {
                                this.rights = Some(cx.new(|cx| {
                                    gpui_kit::component::input::TextareaState::new(window, cx)
                                        .default_value(rights)
                                }));
                            }
                            this.operation.finish(Status::Idle);
                        }
                        Err(error) => this
                            .operation
                            .finish(Status::Error(Failure::ReadRights, format!("{error:#}"))),
                    }
                    cx.notify();
                });
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
        let mut examples = div().grid().grid_cols(3).gap_3();
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
            examples = examples.child(self.preview_row(name, pack, format!("example-{index}"), cx));
        }
        details = details
            .child(examples)
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
        if self.show_rights
            && let Some(rights) = &self.rights
        {
            let area = div().id("pack-rights").h(rems(12.)).w_full().child(
                gpui_kit::component::input::Textarea::new(rights)
                    .aria_label(self.text("出典・利用条件の全文", "Full source and license text"))
                    .readonly(true)
                    .h(rems(12.))
                    .w_full(),
            );
            #[cfg(feature = "test-support")]
            let area = area.test_support();
            details = details.child(area);
        }
        Some(details)
    }

    fn preview_row(
        &self,
        name: &str,
        pack: &file_icons_core::ImportedPack,
        id: String,
        _cx: &mut Context<Self>,
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
                crate::icon_preview::preview(path.to_path_buf(), rems(1.5)).into_any_element()
            }
            None => Icon::new(IconName::File).size_6().into_any_element(),
        };
        let mut description = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .truncate()
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
        let row = div()
            .id(SharedString::from(id.clone()))
            .role(Role::Status)
            .aria_label(label)
            .flex()
            .items_center()
            .flex_wrap()
            .gap_3()
            .child(artwork)
            .child(description.flex_1());
        #[cfg(feature = "test-support")]
        let row = row.test_support();
        row.into_any_element()
    }
}

/// 元の記録を保持し、画面内で利用条件の全文を確認できる形にする。
fn read_rights(path: &std::path::Path) -> Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 16 * 1024 * 1024, "provenance too large");
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut output = String::new();
    let mut append = |text: &str| output.push_str(text);
    append(value["package"]["license"].as_str().unwrap_or(""));
    for key in ["repository", "url", "license_summary"] {
        if let Some(text) = value["download"][key].as_str() {
            append("\n");
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
    fn rights_view_includes_the_full_license_and_keeps_the_source() {
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
        assert_eq!(rendered, format!("MIT\nNOTICE\n{}", "あ".repeat(40_000)));
        assert!(rendered.starts_with("MIT\nNOTICE\nあ"));
        assert_eq!(std::fs::read(file.path()).unwrap(), before);
    }
}
