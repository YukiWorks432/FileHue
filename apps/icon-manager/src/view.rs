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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Screen {
    Packs,
    Targets,
    Thumbnails,
    About,
    Recovery,
    Protection,
}

impl Manager {
    pub(super) fn confirm_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        proceed: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let dirty = match self.screen {
            Screen::Targets => self.target_changes_pending(),
            Screen::Thumbnails => self.thumbnails.dirty(),
            _ => false,
        };
        if !dirty {
            self.operation.clear_result();
            proceed(self, window, cx);
            return;
        }
        use gpui_kit::component::WindowExt as _;
        let title = self.text("未適用の変更を破棄しますか？", "Discard unapplied changes?");
        let description = self.text(
            "変更を反映せずに、この画面を離れます。",
            "Leave this screen without applying the changes.",
        );
        let cancel = self.text("編集を続ける", "Keep editing");
        let discard = self.text("破棄して移動", "Discard and leave");
        let owner = cx.entity().downgrade();
        let proceed = std::rc::Rc::new(proceed);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            let proceed = proceed.clone();
            dialog
                .entrance(gpui_kit::component::dialog::DialogEntrance::None)
                .title(title)
                .description(description)
                .confirm()
                .cancel_text(cancel)
                .ok_text(discard)
                .on_ok(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.screen == Screen::Targets {
                            this.targets.discard(&this.settings);
                            this.protection.draft.enabled = this.settings.preserve_existing_icons;
                            this.protection.draft.rules =
                                this.settings.icon_protection_rules.clone();
                        }
                        if this.screen == Screen::Thumbnails {
                            this.thumbnails.discard();
                        }
                        this.operation.clear_result();
                        proceed(this, window, cx);
                    });
                    true
                })
        });
    }
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
    fn screen_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = cx.theme().colors;
        self.navigation_root(cx)
            .size_full()
            .bg(colors.background)
            .text_color(colors.foreground)
    }
    fn standard_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        self.screen_root(cx)
            .track_focus(&self.focus)
            .tab_stop(false)
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.navigate_back(window, cx);
                }
            }))
    }
    fn navigate_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.busy() && !self.operation.running(Kind::InspectConflicts) {
            return;
        }
        if self.screen == Screen::Thumbnails && self.thumbnails.dirty() {
            self.confirm_navigation(window, cx, |this, window, cx| {
                this.navigate_back(window, cx)
            });
            return;
        }
        self.operation.clear_result();
        if self.screen == Screen::Thumbnails {
            self.screen = self.thumbnails.return_to.take().unwrap_or(Screen::Packs);
            self.focus.focus(window, cx);
            cx.notify();
        } else if self.screen == Screen::About {
            if self.about.license_text.take().is_none() {
                self.screen = self.about.return_to.take().unwrap_or(Screen::Packs);
            }
            self.about.scroll.set_offset(point(px(0.), px(0.)));
            self.focus.focus(window, cx);
            cx.notify();
        } else if self.screen == Screen::Protection {
            self.close_protection(window, cx);
        } else if self.screen == Screen::Recovery {
            self.close_recovery(window, cx);
        } else if self.screen == Screen::Targets {
            self.back_from_targets(window, cx);
        } else if self.browsing {
            self.show_library(false, window, cx);
        } else if self.pending.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        } else if self.selected.take().is_some() {
            self.rights = None;
            self.show_rights = false;
            self.focus.focus(window, cx);
            cx.notify();
        }
    }
    pub(super) fn back_button(&self, id: &'static str, cx: &mut Context<Self>) -> Button {
        theme::button(id, cx)
            .small()
            .icon(IconName::ArrowLeft)
            .label(self.text("戻る", "Back"))
            .disabled_elevated(self.operation.busy())
            .on_click(cx.listener(|this, _, window, cx| this.navigate_back(window, cx)))
    }
    pub(super) fn header(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let brand = div()
            .id("app-brand")
            .text_lg()
            .font_weight(FontWeight::SEMIBOLD)
            .child("FileHue");
        #[cfg(feature = "test-support")]
        let brand = brand.test_support();
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
                    .child(
                        Icon::new(IconName::PanelsTopLeft)
                            .size_5()
                            .text_color(colors.primary),
                    )
                    .child(brand),
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
                        theme::button("about-open", cx)
                            .small()
                            .icon(IconName::Info)
                            .accessibility_label(self.text("アプリ情報", "About"))
                            .tooltip(self.text("アプリ情報", "About"))
                            .disabled_elevated(
                                self.operation.busy()
                                    || matches!(
                                        self.screen,
                                        Screen::Targets | Screen::Protection | Screen::Recovery
                                    ),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.screen != Screen::About {
                                    this.confirm_navigation(window, cx, |this, window, cx| {
                                        this.about.return_to = Some(this.screen);
                                        this.about.license_text = None;
                                        this.screen = Screen::About;
                                        this.about.scroll.set_offset(point(px(0.), px(0.)));
                                        this.focus.focus(window, cx);
                                        cx.notify();
                                    });
                                }
                            })),
                    ),
            )
    }

    pub(super) fn import_button(&self, cx: &mut Context<Self>) -> Button {
        theme::button("import", cx)
            .small()
            .icon(IconName::FolderOpen)
            .label(self.text("パックを取り込む…", "Import pack…"))
            .tooltip(self.text("手元のパックを取り込みます。", "Import a local pack."))
            .disabled_elevated(self.operation.busy() || !self.valid)
            .on_click(cx.listener(|this, _, _, cx| this.choose(cx)))
    }

    fn pack_heading(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_shrink_0()
            .bg(cx.theme().colors.sidebar)
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .child(Icon::new(IconName::Library).size_4())
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if self.browsing {
                        self.text("パックを探す", "Browse packs")
                    } else {
                        self.text("取得済みパック", "Your packs")
                    }),
            )
            .when(!self.browsing, |row| {
                row.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(self.settings.packs.len().to_string()),
                )
            })
            .child(div().flex_1())
            .child(self.import_button(cx))
    }

    pub(super) fn toolbar(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        if self.screen == Screen::Targets {
            return div();
        }
        if compact && self.screen == Screen::Protection {
            return div()
                .flex_shrink_0()
                .bg(cx.theme().colors.sidebar)
                .px_4()
                .py_1()
                .text_sm()
                .child(self.text("既存アイコンの保護", "Existing icon protection"));
        }
        if compact && self.screen == Screen::Recovery {
            return div()
                .flex_shrink_0()
                .bg(cx.theme().colors.sidebar)
                .flex()
                .flex_wrap()
                .gap_2()
                .px_4()
                .py_1()
                .child(
                    theme::button("protection-open", cx)
                        .small()
                        .icon(IconName::ShieldCheck)
                        .label(self.text("アイコンの保護", "Icon protection"))
                        .disabled_elevated(self.operation.busy() || !self.valid)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_protection(window, cx)),
                        ),
                );
        }
        // 復旧・対象編集・保護の途中では、戻る操作で現在の作業を明示的に閉じる。
        let locked = self.operation.busy()
            || matches!(
                self.screen,
                Screen::Targets | Screen::Protection | Screen::Recovery
            );
        let packs = self.screen == Screen::Packs;
        div()
            .flex_shrink_0()
            .bg(cx.theme().colors.sidebar)
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
                            .selected(packs && !self.browsing)
                            .disabled_elevated(locked)
                            .when(packs && !self.browsing, |b| {
                                b.primary()
                                    .icon(IconName::Check)
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_navigation(window, cx, |this, window, cx| {
                                    this.screen = Screen::Packs;
                                    this.show_library(false, window, cx);
                                });
                            })),
                    )
                    .child(
                        theme::button("browse", cx)
                            .icon(IconName::Search)
                            .label(self.text("パックを探す", "Browse packs"))
                            .selected(packs && self.browsing)
                            .disabled_elevated(locked)
                            .when(packs && self.browsing, |b| {
                                b.primary()
                                    .icon(IconName::Check)
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_navigation(window, cx, |this, window, cx| {
                                    this.screen = Screen::Packs;
                                    this.show_library(true, window, cx);
                                });
                            })),
                    ),
            )
            .child(
                theme::button("protection-open", cx)
                    .small()
                    .icon(IconName::ShieldCheck)
                    .label(self.text("アイコンの保護", "Icon protection"))
                    .selected(self.screen == Screen::Protection)
                    .when(self.screen == Screen::Protection, |b| b.primary())
                    .disabled_elevated(
                        self.operation.busy() || !self.valid || self.screen == Screen::Protection,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_navigation(window, cx, |this, window, cx| {
                            this.open_protection(window, cx)
                        });
                    })),
            )
            .child(
                theme::button("conflicts-open", cx)
                    .small()
                    .icon(IconName::TriangleAlert)
                    .when(self.conflict_attention, |b| b.warning())
                    .label(if self.conflict_attention {
                        self.text("競合あり・確認", "Conflicts detected · Review")
                    } else {
                        self.text("競合を確認", "Review conflicts")
                    })
                    .selected(self.screen == Screen::Recovery)
                    .disabled_elevated(
                        self.operation.busy()
                            || self._guard.is_none()
                            || matches!(self.screen, Screen::Recovery | Screen::Protection),
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
                theme::button("thumbnails-open", cx)
                    .small()
                    .ml_4()
                    .icon(IconName::FileImage)
                    .label(self.text("ファイルのプレビュー", "File previews"))
                    .tooltip(self.text(
                        "エクスプローラーで内容を表示するファイル形式を選びます。",
                        "Choose file types to preview in Explorer.",
                    ))
                    .selected(self.screen == Screen::Thumbnails)
                    .when(self.screen == Screen::Thumbnails, |b| b.primary())
                    .disabled_elevated(locked || self._guard.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.screen != Screen::Thumbnails {
                            this.open_thumbnails(cx);
                        }
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
        div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_3()
            .px_4()
            .py_3()
            .flex_shrink_0()
            .bg(colors.sidebar)
            .border_b_1()
            .border_color(colors.border)
            .child(
                Icon::new(if active.is_some() {
                    IconName::Check
                } else {
                    IconName::Monitor
                })
                .size_4(),
            )
            .child(div().flex_1().min_w_0().child(label))
            .child(
                theme::button("restore", cx)
                    .small()
                    .icon(IconName::RotateCcw)
                    .label(if self.valid {
                        self.text("Windows標準に戻す", "Restore Windows defaults")
                    } else {
                        self.text("設定を退避して元に戻す", "Back up settings and restore")
                    })
                    .tooltip(self.text(
                        "元のアイコン設定に戻します。\n表示が変わらない場合は、ファイル操作を終えてから\nタスク マネージャーでExplorerを再起動し、\nフォルダーを開き直してください。",
                        "Restore the original icon settings.\nIf icons do not update, finish file operations,\nrestart Windows Explorer in Task Manager,\nthen reopen folders.",
                    ))
                    .disabled_elevated(
                        self.operation.busy()
                            || self._guard.is_none()
                            || (self.settings.journal.is_empty() && self.settings.active.is_none()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(manager) = this._guard.clone() else {
                            return;
                        };
                        this.start(Kind::Restore, move || manager.restore(true), true, None, cx);
                    })),
            )
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let message = self.operation.message(self.settings.language == "en");
        let status = div()
            .id("status")
            .role(Role::Status)
            .aria_label(message.clone())
            .text_sm()
            .min_w_0()
            .max_h_24()
            .overflow_y_scroll()
            .flex_1()
            .flex()
            .items_center()
            .gap_2()
            .text_color(if self.operation.failed() {
                colors.danger
            } else {
                colors.muted_foreground
            })
            .when(
                self.operation.busy() && self.operation.progress().is_none(),
                |row| row.child(gpui_kit::component::spinner::Spinner::new().small()),
            )
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
        footer = footer.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .when(
                    self.screen == Screen::About
                        || (self.screen == Screen::Packs
                            && (self.browsing
                                || self.selected.is_some()
                                || self.pending.is_some())),
                    |row| row.child(self.back_button("navigate-back", cx)),
                )
                .child(status)
                .when(self.operation.progress().is_some(), |row| {
                    row.child(actions)
                }),
        );
        footer
    }

    pub(super) fn cleanup_button(&self, cx: &mut Context<Self>) -> Button {
        theme::button("cleanup-shell", cx)
            .small()
            .icon(IconName::BrushCleaning)
            .label(self.text("不要なアプリ用ファイルを整理", "Clean up unused app files"))
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
            }))
    }
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < window.rem_size() * 64.;
        if self.screen == Screen::Thumbnails {
            return self
                .standard_root(cx)
                .child(self.header(cx))
                .child(self.toolbar(compact, cx))
                .child(self.thumbnail_panel(window, cx));
        }
        if self.screen == Screen::About {
            return self
                .standard_root(cx)
                .child(self.header(cx))
                .child(self.toolbar(compact, cx))
                .child(self.about_panel(cx))
                .child(self.footer(cx));
        }
        if self.screen == Screen::Protection {
            return self
                .screen_root(cx)
                .flex()
                .flex_col()
                .child(self.header(cx))
                .child(self.toolbar(compact, cx))
                .child(self.protection_panel(window, cx));
        }
        if self.screen == Screen::Recovery {
            return self.screen_root(cx).child(self.recovery_panel(compact, cx));
        }
        if self.screen == Screen::Targets
            && let Some(panel) = self.target_panel(window, cx)
        {
            return self.screen_root(cx).child(panel);
        }
        let body = div().p_4().child(if self.browsing {
            self.catalog(window, cx)
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
        self.standard_root(cx)
            .child(self.header(cx))
            .child(self.toolbar(compact, cx))
            .child(self.registration(cx))
            .child(self.pack_heading(cx))
            .child(scroll)
            .child(self.footer(cx))
    }
}
