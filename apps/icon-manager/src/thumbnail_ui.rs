//! 内容のサムネイルを拡張子ごとに選ぶ画面。
use super::*;
use file_icons_manager_service::{ThumbnailSelectionPlan, ThumbnailStatus};
use file_icons_windows::thumbnails::Format;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable, button::ButtonVariants, scroll::ScrollableElement, switch::Switch, tag::Tag,
};
use gpui_kit::prelude::FluentBuilder;
use std::collections::{BTreeMap, BTreeSet};

fn format_name(extension: &str, english: bool) -> &'static str {
    let (ja, en) = match extension {
        ".ai" => ("Adobe Illustrator用ファイル", "Adobe Illustrator file"),
        ".ait" => ("Illustratorテンプレート", "Illustrator template"),
        ".eps" => ("EPSファイル", "EPS file"),
        ".svg" => ("SVGファイル", "SVG file"),
        ".psd" => ("Adobe Photoshop用ファイル", "Adobe Photoshop file"),
        ".psb" => (
            "Adobe Photoshop用の大容量ファイル",
            "Large Adobe Photoshop file",
        ),
        ".pdf" => ("PDF", "PDF"),
        _ => return "",
    };
    if english { en } else { ja }
}

#[derive(Default)]
pub(super) struct Editor {
    pub(super) return_to: Option<Screen>,
    pub(super) draft: BTreeSet<String>,
    initialized: bool,
    pub(super) status: Option<ThumbnailStatus>,
    pub(super) plan: Option<ThumbnailSelectionPlan>,
    plan_details: bool,
    card_focus: BTreeMap<&'static str, FocusHandle>,
    error: Option<String>,
    scroll: ScrollHandle,
}

impl Editor {
    pub(super) fn applied(&self) -> BTreeSet<String> {
        self.status
            .as_ref()
            .and_then(|status| status.recovery.as_ref())
            .and_then(|record| record.selected_extensions().ok())
            .unwrap_or_default()
    }
    pub(super) fn dirty(&self) -> bool {
        self.status.is_some() && self.draft != self.applied()
    }
    pub(super) fn discard(&mut self) {
        self.draft = self.applied();
        self.plan = None;
    }
}

impl Manager {
    pub(super) fn open_thumbnails(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy() || self._guard.is_none() {
            return;
        }
        self.thumbnails.return_to = Some(self.screen);
        for format in Format::ALL {
            self.thumbnails
                .card_focus
                .entry(format.extension())
                .or_insert_with(|| cx.focus_handle());
        }
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
        self.thumbnails.plan_details = false;
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

    pub(super) fn thumbnail_panel(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let failed = self.thumbnails.error.is_some()
            || self.thumbnails.status.as_ref().is_some_and(|status| {
                status.missing_recovery || status.group_errors.iter().any(Option::is_some)
            });
        let busy = self.operation.busy();
        let actual = self.thumbnails.applied();
        let dirty = self.thumbnails.dirty();
        let unavailable = busy || failed || self.thumbnails.status.is_none();
        let compact = window.viewport_size().width < window.rem_size() * 48.;
        let mut list = theme::card(cx)
            .grid()
            .grid_cols(if compact { 1 } else { 2 });
        for format in Format::ALL {
            let extension = format.extension();
            let chosen = self.thumbnails.draft.contains(extension);
            let applied = actual.contains(extension);
            let description = format_name(extension, self.settings.language == "en");
            let preview_kind = match format {
                Format::Svg => self.text("SVG画像の見本", "SVG image example"),
                Format::Pdf => self.text("先頭ページの見本", "First page example"),
                _ => self.text("埋め込み画像の見本", "Embedded preview example"),
            };
            let (state, badge) = if failed || self.thumbnails.status.is_none() {
                (self.text("要確認", "Needs review"), Tag::warning())
            } else if applied {
                (
                    self.text("プレビュー有効", "Previews enabled"),
                    Tag::success(),
                )
            } else {
                (
                    self.text("プレビュー無効", "Previews disabled"),
                    Tag::secondary(),
                )
            };
            let status = div()
                .id(SharedString::from(format!("thumbnail-state-{extension}")))
                .role(Role::Status)
                .aria_label(state)
                .flex()
                .flex_wrap()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(self.text("現在", "Current")),
                )
                .child(
                    badge
                        .small()
                        .child(if failed || self.thumbnails.status.is_none() {
                            state.to_owned()
                        } else if applied {
                            format!("✓ {state}")
                        } else {
                            format!("− {state}")
                        }),
                )
                .when(
                    !failed && self.thumbnails.status.is_some() && chosen != applied,
                    |status| {
                        let label = self.text("変更あり", "Changed");
                        let change = div()
                            .id(SharedString::from(format!("thumbnail-change-{extension}")))
                            .role(Role::Status)
                            .aria_label(label)
                            .child(Tag::info().small().outline().child(label));
                        #[cfg(feature = "test-support")]
                        let change = change.test_support();
                        status.child(change)
                    },
                );
            #[cfg(feature = "test-support")]
            let status = status.test_support();
            let control = Switch::new(format!("thumbnail-{extension}"))
                .checked(chosen)
                .accessibility_label(format!("{description} {extension}"))
                .disabled(unavailable)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    cx.stop_propagation();
                    this.operation.clear_result();
                    this.thumbnails.plan = None;
                    if *checked {
                        this.thumbnails.draft.insert(extension.into());
                    } else {
                        this.thumbnails.draft.remove(extension);
                    }
                    cx.notify();
                }));
            let sample = div()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .w(rems(2.8))
                        .h(rems(2.4))
                        .relative()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(IconName::File)
                                .size_8()
                                .text_color(colors.muted_foreground),
                        )
                        .child(
                            div()
                                .absolute()
                                .bottom_1()
                                .left_0()
                                .w_full()
                                .text_center()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .child(extension.trim_start_matches('.').to_uppercase()),
                        ),
                )
                .child(
                    div()
                        .text_color(colors.muted_foreground)
                        .child(self.text("アイコン →", "Icon →")),
                )
                .child(img("file-preview-example.png").w(rems(3.2)).h(rems(2.4)))
                .child(
                    div()
                        .text_color(colors.muted_foreground)
                        .child(preview_kind),
                );
            let focus = self
                .thumbnails
                .card_focus
                .get(extension)
                .expect("thumbnail focus");
            let card = div()
                .id(SharedString::from(format!("thumbnail-card-{extension}")))
                .track_focus(focus)
                .tab_stop(false)
                .when(focus.contains_focused(window, cx), |card| {
                    card.bg(colors.muted)
                })
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .min_w_0()
                .border_b_1()
                .border_color(colors.border)
                .when(!unavailable, |card| {
                    card.hover(|style| style.bg(colors.muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.operation.clear_result();
                            this.thumbnails.plan = None;
                            if chosen {
                                this.thumbnails.draft.remove(extension);
                            } else {
                                this.thumbnails.draft.insert(extension.into());
                            }
                            cx.notify();
                        }))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(description),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(colors.muted_foreground)
                                        .child(extension),
                                ),
                        )
                        .child(control),
                )
                .child(sample)
                .child(status);
            #[cfg(feature = "test-support")]
            let card = card.test_support();
            list = list.child(card);
        }

        let mut bulk = div().flex().items_center().gap_2();
        for (id, ja, en, enabled) in [
            ("thumbnail-enable-all", "すべて有効", "Enable all", true),
            ("thumbnail-disable-all", "すべて無効", "Disable all", false),
        ] {
            bulk = bulk.child(
                theme::button(id, cx)
                    .small()
                    .label(self.text(ja, en))
                    .disabled_elevated(unavailable)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.operation.clear_result();
                        this.thumbnails.draft = if enabled {
                            Format::ALL
                                .iter()
                                .map(|format| format.extension().into())
                                .collect()
                        } else {
                            BTreeSet::new()
                        };
                        this.thumbnails.plan = None;
                        cx.notify();
                    })),
            );
        }
        let heading = div()
            .w_full()
            .max_w(rems(56.))
            .mx_auto()
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
            .child(bulk)
            .child(
                theme::button("thumbnail-restore", cx)
                    .small()
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
            .gap_3()
            .p_4()
            .w_full()
            .max_w(rems(56.))
            .mx_auto()
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
                    items
                        .iter()
                        .map(|extension| {
                            format!(
                                "{} ({extension})",
                                format_name(extension, self.settings.language == "en")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
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
                .when(!plan.additions.is_empty(), |preview| {
                    preview.child(div().text_sm().child(format!(
                        "{}: {}",
                        self.text("有効にする", "Enable"),
                        names(&plan.additions)
                    )))
                })
                .when(!plan.removals.is_empty(), |preview| {
                    preview.child(div().text_sm().child(format!(
                        "{}: {}",
                        self.text("元に戻す", "Restore"),
                        names(&plan.removals)
                    )))
                });
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
                paths.extend(group.changes.iter().map(|change| change.key.clone()));
                let unaffected = group
                    .members
                    .iter()
                    .filter(|extension| {
                        !plan.additions.contains(*extension) && !plan.removals.contains(*extension)
                    })
                    .collect::<Vec<_>>();
                if group.class.is_some() && !unaffected.is_empty() {
                    preview = preview.child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.text("関連するファイル形式", "Related file types")),
                    );
                    for extension in unaffected {
                        let outcome = if plan.selected.contains(extension) {
                            self.text("内容を表示", "Content preview")
                        } else {
                            self.text("従来の表示", "Previous display")
                        };
                        let name = format_name(extension, self.settings.language == "en");
                        let label = if name.is_empty() {
                            extension.clone()
                        } else {
                            format!("{name} ({extension})")
                        };
                        preview =
                            preview.child(div().text_sm().child(format!("{label}: {outcome}")));
                    }
                }
            }
            preview = preview.child(div().text_sm().text_color(colors.muted_foreground).child(
                self.text(
                    "アイコンと既定のアプリは変更しません。",
                    "Icons and default apps are unchanged.",
                ),
            ));
            if !paths.is_empty() {
                preview = preview.child(
                    div().flex().child(
                        theme::button("thumbnail-plan-details", cx)
                            .small()
                            .min_w(rems(11.))
                            .label(if self.thumbnails.plan_details {
                                self.text("技術情報を閉じる", "Hide technical details")
                            } else {
                                self.text("技術情報", "Technical details")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.thumbnails.plan_details = !this.thumbnails.plan_details;
                                cx.notify();
                            })),
                    ),
                );
                if self.thumbnails.plan_details {
                    for path in paths {
                        preview = preview.child(
                            div()
                                .text_sm()
                                .text_color(colors.muted_foreground)
                                .child(format!("HKCU\\{path}")),
                        );
                    }
                }
            }
            #[cfg(feature = "test-support")]
            let preview = preview.id("thumbnail-plan").test_support();
            panel = panel.child(preview);
        }
        panel = panel.child(list);
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
        let message = self.operation.message(self.settings.language == "en");
        let status = div()
            .id("status")
            .role(Role::Status)
            .aria_label(message.clone())
            .flex_1()
            .min_w_0()
            .max_h_24()
            .overflow_y_scroll()
            .text_sm()
            .flex()
            .items_center()
            .gap_2()
            .text_color(if self.operation.failed() {
                colors.danger
            } else {
                colors.muted_foreground
            })
            .when(busy, |status| {
                status.child(gpui_kit::component::spinner::Spinner::new().small())
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
            .when(dirty, |actions| {
                actions.child(
                    div()
                        .text_sm()
                        .child(self.text("未適用の変更があります", "There are unapplied changes")),
                )
            })
            .child(
                theme::button("thumbnail-discard", cx)
                    .label(self.text("変更を破棄", "Discard changes"))
                    .disabled_elevated(unavailable || !dirty)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.operation.clear_result();
                        this.thumbnails.discard();
                        cx.notify();
                    })),
            )
            .child(
                theme::button("thumbnail-apply", cx)
                    .primary()
                    .min_w(rems(12.))
                    .label(if self.thumbnails.plan.is_some() {
                        self.text("変更を適用", "Apply changes")
                    } else {
                        self.text("変更内容を確認", "Review changes")
                    })
                    .disabled_elevated(unavailable || !dirty)
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
