//! 主要操作と状態を固定し、パック一覧だけをスクロールする。
use super::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    assets::IconName,
    component::{
        Icon, Sizable,
        button::{ButtonGroup, ButtonVariants},
    },
};

impl Manager {
    fn navigation_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("manager")
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| {
                    this.navigate_back(window, cx);
                    cx.stop_propagation();
                }),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "back"
                    || (event.keystroke.key == "left" && event.keystroke.modifiers.alt)
                {
                    this.navigate_back(window, cx);
                    cx.stop_propagation();
                }
            }))
    }
    fn navigate_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.busy() {
            return;
        }
        if self.thumbnails.open {
            self.thumbnails.open = false;
            self.focus.focus(window, cx);
            cx.notify();
        } else if self.about.open {
            if self.about.license_text.take().is_none() {
                self.about.open = false;
            }
            self.about.scroll.set_offset(point(px(0.), px(0.)));
            self.focus.focus(window, cx);
            cx.notify();
        } else if self.protection.open {
            self.close_protection(window, cx);
        } else if self.recovery.is_open() {
            self.close_recovery(window, cx);
        } else if self.targets.pack.is_some() {
            self.back_from_targets(window, cx);
        } else if self.browsing {
            self.show_library(false, window, cx);
        } else if self.pending.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        } else if self.selected.take().is_some() {
            self.rights.clear();
            self.show_rights = false;
            self.focus.focus(window, cx);
            cx.notify();
        }
    }
    pub(super) fn back_button(&self, id: &'static str, cx: &mut Context<Self>) -> Button {
        theme::button(id, cx)
            .small()
            .icon(IconName::ArrowLeft)
            .label(
                if !self.about.open && self.protection.open && self.protection.dirty(&self.settings)
                {
                    self.text("変更を破棄して戻る", "Discard and return")
                } else {
                    self.text("戻る", "Back")
                },
            )
            .disabled_elevated(self.operation.busy())
            .on_click(cx.listener(|this, _, window, cx| this.navigate_back(window, cx)))
    }
    pub(super) fn header(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap_3()
            .px_4()
            .py_3()
            .bg(colors.sidebar)
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(
                        self.thumbnails.open
                            || self.about.open
                            || self.targets.pack.is_some()
                            || self.protection.open
                            || self.recovery.is_open()
                            || self.browsing
                            || self.selected.is_some()
                            || self.pending.is_some(),
                        |row| {
                            row.child(self.back_button(
                                if self.targets.pack.is_some()
                                    && !self.recovery.is_open()
                                    && !self.protection.open
                                {
                                    "target-back"
                                } else {
                                    "navigate-back"
                                },
                                cx,
                            ))
                        },
                    )
                    .child(
                        Icon::new(IconName::PanelsTopLeft)
                            .size_5()
                            .text_color(colors.primary),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("FileHue"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        theme::button("ja", cx)
                            .small()
                            .label("日本語")
                            .disabled_elevated(self.operation.busy())
                            .selected(self.settings.language != "en")
                            .on_click(cx.listener(|this, _, _, cx| this.set_language("ja", cx))),
                    )
                    .child(
                        theme::button("en", cx)
                            .small()
                            .label("English")
                            .disabled_elevated(self.operation.busy())
                            .selected(self.settings.language == "en")
                            .on_click(cx.listener(|this, _, _, cx| this.set_language("en", cx))),
                    )
                    .child(
                        theme::button("appearance", cx)
                            .small()
                            .icon(if cx.theme().mode.is_dark() {
                                IconName::Sun
                            } else {
                                IconName::Moon
                            })
                            .accessibility_label(self.text("明暗を切替", "Toggle appearance"))
                            .tooltip(self.text(
                                "明るい表示と暗い表示を切り替え",
                                "Switch between light and dark appearance",
                            ))
                            .on_click(cx.listener(|_, _, _, cx| {
                                let mode = if cx.theme().mode.is_dark() {
                                    ThemeMode::Light
                                } else {
                                    ThemeMode::Dark
                                };
                                theme::apply(mode, cx);
                            })),
                    )
                    .child(
                        theme::button("conflicts-open", cx)
                            .icon(IconName::TriangleAlert)
                            .small()
                            .when(self.conflict_attention, |button| button.warning())
                            .label(if self.conflict_attention {
                                self.text("競合あり・確認", "Conflicts detected · Review")
                            } else {
                                self.text("競合を確認", "Review conflicts")
                            })
                            .selected(self.recovery.is_open())
                            .disabled_elevated(
                                self.about.open
                                    || self.thumbnails.open
                                    || self.operation.busy()
                                    || self._guard.is_none()
                                    || self.recovery.is_open()
                                    || self.protection.open,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_conflicts(
                                    file_icons_windows::conflict::Strategy::PreserveExternal,
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        theme::button("protection-open", cx)
                            .icon(IconName::ShieldCheck)
                            .small()
                            .label(self.text("アイコンの保護", "Icon protection"))
                            .disabled_elevated(
                                self.about.open
                                    || self.thumbnails.open
                                    || self.operation.busy()
                                    || !self.valid
                                    || self.protection.open,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_protection(window, cx)),
                            ),
                    ),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        let blocked = self.operation.busy() || !self.valid;
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_2()
            .px_4()
            .py_2()
            .child(
                ButtonGroup::new("pack-view")
                    .small()
                    .child(
                        theme::button("library", cx)
                            .icon(IconName::Library)
                            .label(self.text("取得済み", "Downloaded packs"))
                            .selected(!self.browsing)
                            .when(!self.browsing, |button| {
                                button
                                    .primary()
                                    .icon(IconName::Check)
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_library(false, window, cx)
                            })),
                    )
                    .child(
                        theme::button("browse", cx)
                            .icon(IconName::Search)
                            .label(self.text("パックを探す", "Browse packs"))
                            .selected(self.browsing)
                            .when(self.browsing, |button| {
                                button
                                    .primary()
                                    .icon(IconName::Check)
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_library(true, window, cx)
                            })),
                    ),
            )
            .child(
                theme::button("import", cx)
                    .ml_4()
                    .small()
                    .icon(IconName::FolderOpen)
                    .label(self.text("パックを取り込む…", "Import pack…"))
                    .tooltip(self.text(
                        "package.jsonを含むVS Code拡張フォルダーを選択",
                        "Choose a VS Code extension folder containing package.json",
                    ))
                    .disabled_elevated(blocked)
                    .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
            )
            .child(
                theme::button("thumbnails-open", cx)
                    .small()
                    .label(self.text("内容のサムネイル", "Content thumbnails"))
                    .disabled_elevated(self.operation.busy() || self._guard.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.open_thumbnails(cx))),
            )
            .child(
                theme::button("about-open", cx)
                    .small()
                    .icon(IconName::Info)
                    .label(self.text("アプリ情報", "About"))
                    .disabled_elevated(self.operation.busy())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.about.open = true;
                        this.about.scroll.set_offset(point(px(0.), px(0.)));
                        this.focus.focus(window, cx);
                        cx.notify();
                    })),
            )
    }

    fn registration(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let active = self
            .settings
            .active
            .as_ref()
            .and_then(|id| self.settings.packs.iter().find(|p| &p.id == id));
        let label = if let Some(pack) = active {
            format!("{} · {}", self.text("適用中", "Applied"), pack.label)
        } else if !self.settings.journal.is_empty() {
            self.text("復元が必要です", "Recovery needed").to_owned()
        } else {
            self.text("Windowsの元の表示", "Original Windows icons")
                .to_owned()
        };
        div().flex().items_center().flex_wrap().gap_3().px_4().py_3()
            .flex_shrink_0().bg(colors.group_box).border_b_1().border_color(colors.border)
            .child(Icon::new(if active.is_some() { IconName::Check } else { IconName::Monitor }).size_4())
            .child(div().flex_1().min_w_0().child(label))
            .child(theme::button("restore", cx).small().icon(IconName::RotateCcw)
                .label(if self.valid { self.text("Windows標準に戻す", "Restore Windows defaults") }
                    else { self.text("設定を退避して元に戻す", "Back up settings and restore") })
                .disabled_elevated(self.operation.busy() || self._guard.is_none() || (self.settings.journal.is_empty() && self.settings.active.is_none()))
                .on_click(cx.listener(|this, _, _, cx| {
                    let Some(manager) = this._guard.clone() else { return; };
                    this.start(Kind::Restore, move || manager.restore(true), true, None, cx);
                })))
            .child(theme::button("restart-help", cx).small().icon(IconName::Info)
                .accessibility_label(self.text("アイコンが変わらない場合", "If icons do not update"))
                .tooltip(self.text("表示が変わらない場合は、コピーなどを終えてからタスク マネージャーでExplorerを再起動し、フォルダーを開き直してください。", "If icons do not update, finish file operations, restart Windows Explorer in Task Manager, then reopen folders.")))
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let message = self
            .operation
            .status()
            .render(self.settings.language == "en");
        let status = div()
            .id("status")
            .role(Role::Status)
            .aria_label(message.clone())
            .text_sm()
            .min_w_0()
            .max_h_24()
            .overflow_y_scroll()
            .flex_1()
            .text_color(if self.operation.failed() {
                colors.danger
            } else {
                colors.muted_foreground
            })
            .child(message);
        #[cfg(feature = "test-support")]
        let status = status.test_support();
        let mut footer = div()
            .flex_shrink_0()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .bg(colors.sidebar)
            .border_t_1()
            .border_color(colors.border);
        let mut actions = div().flex().items_center().flex_wrap().gap_3();
        if let Some(progress) = self.operation.progress() {
            let stage = progress.stage.load(std::sync::atomic::Ordering::Relaxed);
            let label = if progress.is_cancelled() {
                self.text(
                    "キャンセルの完了を待っています…",
                    "Waiting for cancellation…",
                )
            } else if self.operation.running(Kind::Import) {
                self.text("パックを確認・変換中", "Preparing and converting pack")
            } else {
                match stage {
                    0 => self.text("ダウンロード中", "Downloading"),
                    1 => self.text("検証・展開中", "Verifying and extracting"),
                    _ => self.text("アイコンを変換中", "Converting icons"),
                }
            };
            let mut message = if self.operation.running(Kind::Import) {
                label.to_owned()
            } else {
                format!(
                    "{label} · {} KiB",
                    progress.bytes.load(std::sync::atomic::Ordering::Relaxed) / 1024
                )
            };
            let conversion = progress.conversion();
            if conversion.total > 0 {
                message.push_str(&format!(
                    " · {} {}/{} · {} {}/{}",
                    self.text("テーマ", "Theme"),
                    conversion.theme,
                    conversion.themes,
                    self.text("画像", "Icons"),
                    conversion.completed,
                    conversion.total,
                ));
            }
            let elapsed = progress.elapsed().as_secs();
            message.push_str(&format!(
                " · {} {}:{}",
                self.text("経過", "Elapsed"),
                elapsed / 60,
                format_args!("{:02}", elapsed % 60)
            ));
            let progress_status = div()
                .id("operation-progress")
                .role(Role::Status)
                .aria_label(message.clone())
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .text_sm()
                .child(gpui_kit::component::spinner::Spinner::new().small())
                .child(message);
            #[cfg(feature = "test-support")]
            let progress_status = progress_status.test_support();
            actions = actions.child(progress_status).child(
                theme::button("cancel-download", cx)
                    .small()
                    .icon(IconName::X)
                    .disabled_elevated(progress.is_cancelled())
                    .label(self.text("キャンセル", "Cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(progress) = this.operation.progress() {
                            progress.cancel();
                        }
                        cx.notify();
                    })),
            );
        }
        let cleanup = theme::button("cleanup-shell", cx)
            .small()
            .icon(IconName::BrushCleaning)
            .accessibility_label(
                self.text("不要なアプリ用ファイルを整理", "Clean up unused app files"),
            )
            .tooltip(self.text(
                "Windowsの元の表示に戻した後、未使用のShell DLLを整理",
                "Clean up unused Shell DLLs after restoring Windows defaults",
            ))
            .disabled_elevated(
                self.operation.busy()
                    || !self.valid
                    || self.settings.active.is_some()
                    || !self.settings.journal.is_empty(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                let Some(manager) = this._guard.clone() else {
                    return;
                };
                this.run(Kind::Cleanup, move || manager.cleanup(), cx);
            }));
        footer = footer.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(status)
                .child(cleanup),
        );
        if self.operation.progress().is_some() {
            footer = footer.child(actions);
        }
        footer
    }
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        if self.thumbnails.open {
            return self
                .navigation_root(cx)
                .track_focus(&self.focus)
                .tab_stop(false)
                .size_full()
                .bg(colors.background)
                .text_color(colors.foreground)
                .flex()
                .flex_col()
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.navigate_back(window, cx);
                    }
                }))
                .child(self.header(cx))
                .child(self.thumbnail_panel(cx))
                .child(self.footer(cx));
        }
        if self.about.open {
            return self
                .navigation_root(cx)
                .track_focus(&self.focus)
                .tab_stop(false)
                .size_full()
                .bg(colors.background)
                .text_color(colors.foreground)
                .flex()
                .flex_col()
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.navigate_back(window, cx);
                    }
                }))
                .child(self.header(cx))
                .child(self.about_panel(cx));
        }
        if self.protection.open {
            return self
                .navigation_root(cx)
                .size_full()
                .bg(colors.background)
                .text_color(colors.foreground)
                .flex()
                .flex_col()
                .child(self.protection_panel(window, cx))
                .when(self.operation.busy() || self.operation.failed(), |root| {
                    root.child(self.footer(cx))
                });
        }
        if let Some(panel) = self.recovery_panel(cx) {
            return self
                .navigation_root(cx)
                .size_full()
                .bg(colors.background)
                .text_color(colors.foreground)
                .child(panel);
        }
        if let Some(panel) = self.target_panel(window, cx) {
            return self
                .navigation_root(cx)
                .size_full()
                .bg(colors.background)
                .text_color(colors.foreground)
                .child(panel);
        }
        let body = div().p_4().child(if self.browsing {
            self.catalog(cx)
        } else {
            self.library(cx)
        });
        let scroll = div()
            .id(if self.browsing {
                "catalog-scroll"
            } else {
                "pack-scroll"
            })
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(body);
        #[cfg(feature = "test-support")]
        let scroll = scroll.test_support();
        self.navigation_root(cx)
            .track_focus(&self.focus)
            .tab_stop(false)
            .size_full()
            .bg(colors.background)
            .text_color(colors.foreground)
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.navigate_back(window, cx);
                }
            }))
            .child(self.header(cx))
            .child(self.registration(cx))
            .child(self.toolbar(cx))
            .child(scroll)
            .child(self.footer(cx))
    }
}
