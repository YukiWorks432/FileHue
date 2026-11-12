//! 内容のサムネイルを拡張子ごとに選ぶ画面。
use super::*;
use file_icons_manager_service::{ThumbnailSelectionPlan, ThumbnailStatus};
use file_icons_windows::thumbnails::Format;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    assets::IconName,
    component::{Sizable, button::ButtonVariants, scroll::ScrollableElement},
};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) return_to: Option<Screen>,
    pub(super) draft: BTreeSet<String>,
    initialized: bool,
    pub(super) status: Option<ThumbnailStatus>,
    pub(super) plan: Option<ThumbnailSelectionPlan>,
    error: Option<String>,
    scroll: ScrollHandle,
}

impl Manager {
    pub(super) fn open_thumbnails(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy() || self._guard.is_none() {
            return;
        }
        self.thumbnails.return_to = Some(self.screen);
        self.screen = Screen::Thumbnails;
        self.thumbnails.plan = None;
        self.thumbnail_operation(|_| Ok(()), !self.thumbnails.initialized, false, cx);
    }

    fn preview_thumbnails(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy() {
            return;
        }
        let Some(manager) = self._guard.clone() else {
            return;
        };
        self.thumbnails.plan = None;
        let selected = self.thumbnails.draft.clone();
        self.operation.begin(Kind::Thumbnails, None);
        let task = cx
            .background_executor()
            .spawn(async move { manager.preview_thumbnail_selection(selected) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(plan) if plan.selected == this.thumbnails.draft => {
                        this.thumbnails.plan = Some(plan);
                        this.thumbnails.scroll.set_offset(point(px(0.), px(0.)));
                        this.operation.finish(Status::Idle);
                    }
                    Ok(_) => this.operation.finish(Status::Idle),
                    Err(error) => this
                        .operation
                        .finish(Status::Error(Failure::Operation, format!("{error:#}"))),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn thumbnail_operation(
        &mut self,
        operation: impl FnOnce(&ManagerService) -> Result<()> + Send + 'static,
        reset_draft: bool,
        report_success: bool,
        cx: &mut Context<Self>,
    ) {
        if self.operation.busy() {
            return;
        }
        let Some(manager) = self._guard.clone() else {
            return;
        };
        self.operation.begin(Kind::Thumbnails, None);
        let task = cx.background_executor().spawn(async move {
            let result = operation(&manager);
            let status = manager.thumbnail_status();
            (result, status)
        });
        cx.spawn(async move |this, cx| {
            let (result, status) = task.await;
            let _ = this.update(cx, |this, cx| {
                let success = result.is_ok();
                if report_success {
                    this.thumbnails.plan = None;
                }
                this.operation.finish(match result {
                    Ok(()) if report_success => Status::Complete,
                    Ok(()) => Status::Idle,
                    Err(error) => Status::Error(Failure::Operation, format!("{error:#}")),
                });
                match status {
                    Ok(status) => {
                        if success && reset_draft {
                            this.thumbnails.draft = status
                                .recovery
                                .as_ref()
                                .and_then(|record| record.selected_extensions().ok())
                                .unwrap_or_default();
                            this.thumbnails.initialized = true;
                        }
                        this.thumbnails.error = None;
                        this.thumbnails.status = Some(status);
                    }
                    Err(error) => {
                        this.thumbnails.status = None;
                        this.thumbnails.error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn thumbnail_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let failed = self.thumbnails.error.is_some()
            || self.thumbnails.status.as_ref().is_some_and(|status| {
                status.missing_recovery || status.group_errors.iter().any(Option::is_some)
            });
        let busy = self.operation.busy();
        let actual = self
            .thumbnails
            .status
            .as_ref()
            .and_then(|status| status.recovery.as_ref())
            .and_then(|record| record.selected_extensions().ok())
            .unwrap_or_default();
        let mut list = theme::card(cx).flex().flex_col();
        for format in Format::ALL {
            let extension = format.extension();
            let chosen = self.thumbnails.draft.contains(extension);
            let description = match format {
                Format::Ai => self.text("埋め込みプレビュー", "Embedded preview"),
                Format::Eps => self.text("埋め込みプレビュー", "Embedded preview"),
                Format::Svg => self.text("画像を描画", "Render image"),
                Format::Psd | Format::Psb => self.text("埋め込みプレビュー", "Embedded preview"),
                Format::Pdf => self.text("先頭ページ", "First page"),
            };
            let applied = if failed || self.thumbnails.status.is_none() {
                self.text("要確認", "Unknown")
            } else if actual.contains(extension) {
                self.text("適用中", "Applied")
            } else {
                self.text("未適用", "Not applied")
            };
            list = list.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .child(
                        theme::button(format!("thumbnail-{extension}"), cx)
                            .small()
                            .label(extension.to_owned())
                            .selected(chosen)
                            .when(chosen, |button| button.primary().icon(IconName::Check))
                            .disabled_elevated(busy || failed || self.thumbnails.status.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.thumbnails.plan = None;
                                if !this.thumbnails.draft.remove(extension) {
                                    this.thumbnails.draft.insert(extension.into());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(description),
                    )
                    .child(div().flex_1().min_w_0())
                    .child(div().text_sm().child(applied)),
            );
        }
        let heading = div()
            .flex_shrink_0()
            .px_4()
            .pt_4()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.text("ファイルのプレビュー", "File previews")),
            )
            .child(div().flex_1())
            .child(
                theme::button("thumbnail-restore", cx)
                    .label(self.text("すべて元に戻す", "Restore all"))
                    .disabled_elevated(
                        busy || self._guard.is_none()
                            || self
                                .thumbnails
                                .status
                                .as_ref()
                                .is_some_and(|status| status.missing_recovery),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.thumbnail_operation(
                            |manager| manager.restore_thumbnails(),
                            true,
                            true,
                            cx,
                        );
                    })),
            );
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(self.text(
                        "エクスプローラーで内容をプレビューするファイル形式を選びます。",
                        "Choose file types to preview in Explorer.",
                    ))
                    .child(self.text(
                        "アイコンと既定のアプリは変更しません。",
                        "Icons and default apps are unchanged.",
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(self.text(
                        "AI・PSD・PSB・EPS は表示に必要なプレビュー情報がある場合に表示します。",
                        "AI, PSD, PSB and EPS are shown when suitable preview data is available.",
                    )),
            );
        if let Some(plan) = &self.thumbnails.plan {
            let names = |items: &BTreeSet<String>| {
                if items.is_empty() {
                    self.text("なし", "None").to_string()
                } else {
                    items.iter().cloned().collect::<Vec<_>>().join(", ")
                }
            };
            let mut preview = theme::card(cx)
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.text("適用前の確認", "Review changes")),
                )
                .child(div().text_sm().child(format!(
                    "{}: {}",
                    self.text("有効にする", "Enable"),
                    names(&plan.additions)
                )))
                .child(div().text_sm().child(format!(
                    "{}: {}",
                    self.text("元に戻す", "Restore"),
                    names(&plan.removals)
                )));
            let groups = plan
                .existing_groups
                .iter()
                .chain(plan.new_groups.iter())
                .flat_map(|record| &record.groups)
                .filter(|group| {
                    group.members.iter().any(|extension| {
                        plan.additions.contains(extension) || plan.removals.contains(extension)
                    })
                });
            let mut paths = BTreeSet::new();
            for group in groups {
                if let Some(class) = &group.class {
                    preview =
                        preview.child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
                            format!(
                                "{}: {class}",
                                self.text("共有される関連付け", "Shared association")
                            ),
                        ));
                    for extension in &group.members {
                        let outcome = if plan.selected.contains(extension) {
                            self.text("内容を表示", "Content preview")
                        } else {
                            self.text("従来の表示", "Previous display")
                        };
                        preview =
                            preview.child(div().text_sm().child(format!("{extension}: {outcome}")));
                    }
                }
                for change in &group.changes {
                    paths.insert(change.key.clone());
                }
            }
            if !paths.is_empty() {
                preview = preview.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.text("関連する登録先", "Related registry locations")),
                );
                for path in paths {
                    preview = preview.child(
                        div()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(format!("HKCU\\{path}")),
                    );
                }
            }
            preview = preview.child(div().text_sm().text_color(colors.muted_foreground).child(
                self.text(
                    "登録先は変更内容に応じて更新・復元し、適用時にもう一度確認します。",
                    "Locations are updated or restored as needed and checked again when applied.",
                ),
            ));
            #[cfg(feature = "test-support")]
            let preview = preview.id("thumbnail-plan").test_support();
            panel = panel.child(preview);
        }
        panel = panel
            .child(list)
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(self.text(
                        "上の選択は適用するまで下書きです。",
                        "The selection above is a draft until applied.",
                    )),
            );
        if failed {
            panel = panel.child(div().text_color(colors.danger).child(self.text(
                "登録を確認できません。詳細を確認してください。",
                "The registration could not be verified. Review the details below.",
            )));
            if let Some(error) = &self.thumbnails.error {
                panel = panel.child(div().text_sm().child(error.clone()));
            }
            if let Some(status) = &self.thumbnails.status {
                if status.missing_recovery {
                    let missing = div().id("thumbnail-missing-recovery").text_sm().child(self.text(
                        "復元記録が見つかりません。設定と登録を保持しています。以前の復元記録がある場合は、戻してから再度開いてください。",
                        "The recovery record is missing. Settings and registrations are preserved. If you have a previous recovery record, restore it and reopen this screen.",
                    ));
                    #[cfg(feature = "test-support")]
                    let missing = missing.test_support();
                    panel = panel.child(missing);
                }
                for error in status.group_errors.iter().flatten() {
                    panel = panel.child(div().text_sm().child(error.clone()));
                }
            }
        }
        let message = self
            .operation
            .status()
            .render(self.settings.language == "en");
        let status = div()
            .id("status")
            .role(Role::Status)
            .aria_label(message.clone())
            .flex_1()
            .min_w_0()
            .max_h_24()
            .overflow_y_scroll()
            .text_sm()
            .text_color(if self.operation.failed() {
                colors.danger
            } else {
                colors.muted_foreground
            })
            .child(message);
        #[cfg(feature = "test-support")]
        let status = status.test_support();
        let actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .p_4()
            .bg(colors.sidebar)
            .border_t_1()
            .border_color(colors.border)
            .child(self.back_button("navigate-back", cx))
            .child(status)
            .child(
                theme::button("thumbnail-discard", cx)
                    .label(self.text("変更を破棄", "Discard changes"))
                    .disabled_elevated(busy || failed || self.thumbnails.status.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.thumbnails.plan = None;
                        this.thumbnails.draft = this
                            .thumbnails
                            .status
                            .as_ref()
                            .and_then(|status| status.recovery.as_ref())
                            .and_then(|record| record.selected_extensions().ok())
                            .unwrap_or_default();
                        cx.notify();
                    })),
            )
            .child(
                theme::button("thumbnail-apply", cx)
                    .primary()
                    .label(if self.thumbnails.plan.is_some() {
                        self.text("選択を適用", "Apply selection")
                    } else {
                        self.text("変更内容を確認", "Review changes")
                    })
                    .disabled_elevated(busy || failed || self.thumbnails.status.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this
                            .thumbnails
                            .plan
                            .as_ref()
                            .is_some_and(|plan| plan.selected == this.thumbnails.draft)
                        {
                            this.preview_thumbnails(cx);
                            return;
                        }
                        let reviewed = this
                            .thumbnails
                            .plan
                            .take()
                            .expect("reviewed thumbnail plan");
                        let changed = this.text(
                            "確認後に登録が変更されました。変更内容をもう一度確認してください。",
                            "The registration changed after review. Review the changes again.",
                        );
                        this.thumbnail_operation(
                            move |manager| {
                                let actual = manager
                                    .preview_thumbnail_selection(reviewed.selected.clone())?;
                                anyhow::ensure!(actual == reviewed, "{changed}");
                                manager.apply_reviewed_thumbnail_selection(reviewed)
                            },
                            true,
                            true,
                            cx,
                        );
                    })),
            );
        let scroll = div()
            .id("thumbnail-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.thumbnails.scroll)
            .child(panel);
        #[cfg(feature = "test-support")]
        let scroll = scroll.test_support();
        let viewport = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(scroll)
            .vertical_scrollbar(&self.thumbnails.scroll);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(heading)
            .child(viewport)
            .child(actions)
    }
}
