//! 内容のサムネイルを拡張子ごとに選ぶ画面。
use super::*;
use file_icons_manager_service::ThumbnailStatus;
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
    error_details: bool,
    card_focus: BTreeMap<&'static str, FocusHandle>,
    example_icons: Option<super::original_icons::Previews>,
    example_icons_loading: bool,
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
        self.thumbnails.error_details = false;
        if self.thumbnails.example_icons.is_none() && !self.thumbnails.example_icons_loading {
            self.thumbnails.example_icons_loading = true;
            let task = cx
                .background_executor()
                .spawn(async { super::original_icons::generate_thumbnail_examples().ok() });
            cx.spawn(async move |this, cx| {
                let previews = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.thumbnails.example_icons = previews;
                    this.thumbnails.example_icons_loading = false;
                    cx.notify();
                });
            })
            .detach();
        }
        self.thumbnail_operation(|_| Ok(()), !self.thumbnails.initialized, false, cx);
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
        let mut list = div().grid().grid_cols(if compact { 1 } else { 2 }).gap_3();
        for format in Format::ALL {
            let extension = format.extension();
            let chosen = self.thumbnails.draft.contains(extension);
            let applied = actual.contains(extension);
            let description = format_name(extension, self.settings.language == "en");
            let (sample_name, sample_path, preview_kind) = match format {
                Format::Ai => (
                    "identity.ai",
                    "thumbnail-example-ai.png",
                    self.text("プレビュー情報がある場合", "With preview data"),
                ),
                Format::Eps => (
                    "festival.eps",
                    "thumbnail-example-eps.png",
                    self.text("プレビュー情報がある場合", "With preview data"),
                ),
                Format::Svg => (
                    "ribbon.svg",
                    "thumbnail-example-svg.png",
                    self.text("図形を表示", "Vector artwork"),
                ),
                Format::Psd => (
                    "studio.psd",
                    "thumbnail-example-psd.png",
                    self.text("プレビュー情報がある場合", "With preview data"),
                ),
                Format::Psb => (
                    "panorama.psb",
                    "thumbnail-example-psb.png",
                    self.text("プレビュー情報がある場合", "With preview data"),
                ),
                Format::Pdf => (
                    "project-notes.pdf",
                    "thumbnail-example-pdf.png",
                    self.text("先頭ページを表示", "First page"),
                ),
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
                .child(div().w(rems(9.)).child(badge.child(
                    if failed || self.thumbnails.status.is_none() {
                        format!("⚠ {state}")
                    } else if applied {
                        format!("✓ {state}")
                    } else {
                        format!("− {state}")
                    },
                )))
                .when(
                    !failed && self.thumbnails.status.is_some() && chosen != applied,
                    |status| {
                        let label = self.text("変更あり", "Changed");
                        let change = div()
                            .id(SharedString::from(format!("thumbnail-change-{extension}")))
                            .role(Role::Status)
                            .aria_label(label)
                            .child(Tag::info().small().child(label));
                        #[cfg(feature = "test-support")]
                        let change = change.test_support();
                        status.child(change).child(
                            div()
                                .text_sm()
                                .text_color(colors.info_foreground)
                                .child(if chosen {
                                    self.text("適用後：有効", "After apply: on")
                                } else {
                                    self.text("適用後：無効", "After apply: off")
                                }),
                        )
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
                    if *checked {
                        this.thumbnails.draft.insert(extension.into());
                    } else {
                        this.thumbnails.draft.remove(extension);
                    }
                    cx.notify();
                }));
            let existing_icon = self
                .thumbnails
                .example_icons
                .as_ref()
                .and_then(|previews| previews.paths.get(extension));
            let sample = div()
                .flex()
                .items_end()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(self.text("無効時", "Off")),
                        )
                        .child(
                            div()
                                .w(rems(4.))
                                .h(rems(2.5))
                                .bg(colors.muted)
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when_some(existing_icon, |container, path| {
                                    container.child(img(path.clone()).size_8())
                                })
                                .when(existing_icon.is_none(), |container| {
                                    container.child(
                                        Icon::new(IconName::File)
                                            .size_8()
                                            .text_color(colors.muted_foreground),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .h(rems(2.5))
                        .flex()
                        .items_center()
                        .text_color(colors.muted_foreground)
                        .child("→"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(self.text("有効時", "On")),
                        )
                        .child(img(sample_path).w(rems(4.8)).h(rems(2.5))),
                )
                .child(
                    div()
                        .h(rems(2.5))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(sample_name)
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(preview_kind),
                        ),
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
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .min_w_0()
                .bg(colors.group_box)
                .rounded_lg()
                .border_1()
                .border_color(colors.border)
                .when(focus.contains_focused(window, cx), |card| {
                    card.bg(colors.muted).border_color(colors.ring)
                })
                .when(!unavailable, |card| {
                    card.hover(|style| style.bg(colors.muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.operation.clear_result();
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
                        cx.notify();
                    })),
            );
        }
        let heading = div()
            .w_full()
            .max_w(rems(68.))
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
                    .icon(IconName::RotateCcw)
                    .label(self.text("元の設定に戻す…", "Restore previous settings…"))
                    .disabled_elevated(
                        busy || self._guard.is_none()
                            || self
                                .thumbnails
                                .status
                                .as_ref()
                                .is_some_and(|status| status.missing_recovery),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        use gpui_kit::component::WindowExt as _;
                        let owner = cx.entity().downgrade();
                        let title = this.text(
                            "元のサムネイル設定に戻します",
                            "Restore previous thumbnail settings",
                        );
                        let description = this.text(
                            "FileHueによる変更を解除し、適用前の設定へ戻します。",
                            "Remove FileHue's changes and restore the previous settings.",
                        );
                        let cancel = this.text("キャンセル", "Cancel");
                        let restore = this.text("元に戻す", "Restore settings");
                        window.open_alert_dialog(cx, move |dialog, _, _| {
                            let owner = owner.clone();
                            dialog
                                .entrance(gpui_kit::component::dialog::DialogEntrance::None)
                                .title(title)
                                .description(description)
                                .confirm()
                                .cancel_text(cancel)
                                .ok_text(restore)
                                .on_ok(move |_, _, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.thumbnail_operation(
                                            |manager| manager.restore_thumbnails(),
                                            true,
                                            true,
                                            cx,
                                        )
                                    });
                                    true
                                })
                        });
                    })),
            );
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .w_full()
            .max_w(rems(68.))
            .mx_auto()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(self.text(
                        "エクスプローラーのサムネイルに、ファイルの内容を表示します。",
                        "Show file contents in Explorer thumbnails.",
                    ))
                    .child(self.text(
                        "アイコンと既定のアプリは変更しません。画像は表示例です。",
                        "Icons and default apps are unchanged. Images are examples.",
                    )),
            );
        if failed {
            panel = panel.child(
                theme::card(cx)
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().text_sm().child(
                        if self.thumbnails.status.as_ref().is_some_and(|status| status.missing_recovery) {
                            self.text("復元記録が見つかりません。現在の設定を保持しています。", "The recovery record is missing. Current settings are preserved.")
                        } else {
                            self.text("現在の設定を確認できません。再確認してください。", "The current settings could not be verified. Please check again.")
                        },
                    ))
                    .child(theme::button("thumbnail-error-details", cx).small()
                        .label(if self.thumbnails.error_details { self.text("詳細を閉じる", "Hide details") } else { self.text("詳細", "Details") })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.thumbnails.error_details = !this.thumbnails.error_details;
                            cx.notify();
                        })))
                    .child(
                        theme::button("thumbnail-recheck", cx)
                            .small()
                            .label(self.text("再確認", "Check again"))
                            .disabled_elevated(busy || self._guard.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                            this.thumbnail_operation(
                                    |_| Ok(()),
                                    !this.thumbnails.initialized,
                                    false,
                                    cx,
                                );
                            })),
                    ),
            );
        }
        if failed && self.thumbnails.error_details {
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
        panel = panel.child(list);
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
            .when(dirty, |actions| {
                actions.child(
                    theme::button("thumbnail-discard", cx)
                        .label(self.text("変更を破棄", "Discard changes"))
                        .disabled_elevated(unavailable || !dirty)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.operation.clear_result();
                            this.thumbnails.discard();
                            cx.notify();
                        })),
                )
            })
            .when(dirty, |actions| {
                actions.child(
                    theme::button("thumbnail-apply", cx)
                        .primary()
                        .min_w(rems(12.))
                        .label(self.text("変更を適用", "Apply changes"))
                        .disabled_elevated(unavailable || !dirty)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let selected = this.thumbnails.draft.clone();
                            this.thumbnail_operation(
                                move |manager| manager.set_thumbnail_selection(selected),
                                true,
                                true,
                                cx,
                            );
                        })),
                )
            });
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
