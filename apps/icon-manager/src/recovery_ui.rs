//! 復旧方法の選択、変更予定の確認、完了を段階ごとに示す。
use super::*;
use file_icons_windows::conflict::{Plan, Strategy};
use gpui_kit::component::{Sizable, button::ButtonVariants, radio::Radio, spinner::Spinner};
use gpui_kit::prelude::FluentBuilder;

#[derive(Default)]
pub(super) struct Editor {
    plan: Option<Plan>,
    alternative: Option<Plan>,
    unavailable: Option<String>,
    details: bool,
    all: bool,
    page: usize,
    reviewed: bool,
    needs_refresh: bool,
    pub completed: bool,
    focus: Option<FocusHandle>,
    previous_focus: Option<FocusHandle>,
}

impl Editor {
    pub(super) fn backup_hint(&self) -> Option<PathBuf> {
        self.plan.as_ref().map(|plan| plan.backup.clone())
    }
    pub(super) fn complete(&mut self, actual_backup: Option<PathBuf>) {
        if let (Some(plan), Some(path)) = (&mut self.plan, actual_backup) {
            plan.backup = path;
        }
        self.completed = true;
    }
}

#[cfg(test)]
impl Editor {
    pub(super) fn visual(plan: Plan) -> Self {
        Self {
            plan: Some(plan),
            unavailable: Some("Pack unavailable".into()),
            ..Default::default()
        }
    }
    pub(super) fn visual_stage(&mut self, reviewed: bool, completed: bool) {
        self.reviewed = reviewed;
        self.completed = completed;
    }
}

fn value_text(value: &Option<file_icons_core::registry::Value>, en: bool) -> String {
    let Some(value) = value else {
        return if en { "Absent" } else { "値なし" }.into();
    };
    if matches!(value.kind, 1 | 2) {
        let words = value
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .take(1024)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|w| *w != 0)
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&words)
    } else {
        format!(
            "{} {} · {} {}",
            if en { "type" } else { "型" },
            value.kind,
            value.bytes.len(),
            if en { "bytes" } else { "バイト" }
        )
    }
}

impl Manager {
    pub(super) fn open_conflicts(
        &mut self,
        strategy: Strategy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation.busy() {
            return;
        }
        let Some(service) = self._guard.clone() else {
            return;
        };
        if self.screen != Screen::Recovery {
            self.recovery.previous_focus = window.focused(cx);
            self.recovery_return = self.screen;
        }
        let focus = self.recovery.focus.get_or_insert_with(|| cx.focus_handle());
        focus.focus(window, cx);
        self.screen = Screen::Recovery;
        self.recovery.plan = None;
        self.recovery.alternative = None;
        self.recovery.unavailable = None;
        self.recovery.reviewed = false;
        self.recovery.needs_refresh = false;
        self.recovery.completed = false;
        self.recovery.page = 0;
        self.operation.begin(Kind::InspectConflicts, None);
        let task = cx.background_executor().spawn(async move {
            let preserve = service.conflicts(Strategy::PreserveExternal)?;
            let reapply = service.conflicts(Strategy::ReapplyTheme);
            Ok::<_, anyhow::Error>((preserve, reapply))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((preserve, reapply)) => {
                        let (plan, alternative) = match reapply {
                            Ok(reapply) if strategy == Strategy::ReapplyTheme => {
                                (reapply, Some(preserve))
                            }
                            Ok(reapply) => (preserve, Some(reapply)),
                            Err(error) => {
                                this.recovery.unavailable = Some(format!("{error:#}"));
                                (preserve, None)
                            }
                        };
                        this.conflict_attention = plan.conflicts > 0 || plan.resuming;
                        this.recovery.plan = Some(plan);
                        this.recovery.alternative = alternative;
                        this.operation.finish(Status::Idle);
                    }
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

    pub(super) fn close_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovery.reviewed {
            self.recovery.reviewed = false;
            if let Some(focus) = &self.recovery.focus {
                focus.focus(window, cx);
            }
        } else {
            self.screen = self.recovery_return;
            if let Some(focus) = self.recovery.previous_focus.take() {
                focus.focus(window, cx);
            }
        }
        cx.notify();
    }

    fn select_recovery(&mut self, strategy: Strategy, cx: &mut Context<Self>) {
        if self.operation.busy() || self.recovery.completed {
            return;
        }
        if self
            .recovery
            .plan
            .as_ref()
            .is_some_and(|p| p.strategy != strategy)
            && self
                .recovery
                .alternative
                .as_ref()
                .is_some_and(|p| p.strategy == strategy)
        {
            std::mem::swap(&mut self.recovery.plan, &mut self.recovery.alternative);
            self.recovery.reviewed = false;
            self.recovery.page = 0;
            cx.notify();
        }
    }

    pub(super) fn recovery_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let completed = self.recovery.completed;
        let title = if completed {
            self.text("アイコン設定を復旧しました", "Icon settings recovered")
        } else if self.recovery.reviewed {
            self.text("変更内容の最終確認", "Review changes before recovery")
        } else {
            self.text("アイコン設定の復旧", "Recover icon settings")
        };
        let mut body = div().w_full().min_w_0().max_w(rems(64.)).mx_auto().p_4().flex().flex_col().gap_4()
            .child(div().flex().flex_col().gap_1()
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_sm().text_color(colors.muted_foreground).child(if completed {
                    self.text("表示が更新されない場合は、ファイル操作を終えてからExplorerを再起動してください。", "If icons have not refreshed, finish file operations and restart Explorer.")
                } else if self.recovery.reviewed {
                    self.text("まだ変更していません。下の実行ボタンで確定します。", "Nothing has changed yet. Use the execution button below to commit.")
                } else {
                    self.text("復元記録と現在の設定を照合しました。復旧方法を選び、変更内容を確認してください。", "Review the saved registration and current settings, then choose how to recover.")
                })));
        if let Some(plan) = &self.recovery.plan {
            if !completed && !self.recovery.reviewed {
                body = body.child(theme::card(cx).p_4().child(self.recovery_methods(cx)));
            }
            body = body.child(theme::card(cx).p_4().child(self.recovery_summary(plan, cx)));
            if !completed {
                body = body.child(theme::card(cx).p_4().child(self.recovery_entries(plan, cx)));
            }
        }
        let scroll = div()
            .id("recovery-scroll")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .child(body);
        div()
            .size_full()
            .min_w_0()
            .when_some(self.recovery.focus.as_ref(), |view, focus| {
                view.track_focus(focus)
            })
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(scroll)
            .child(self.recovery_footer(cx))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && !this.operation.busy() {
                    this.close_recovery(window, cx);
                }
            }))
            .into_any_element()
    }

    fn recovery_methods(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let strategy = self.recovery.plan.as_ref().map(|p| p.strategy);
        let mut choices = div().flex().flex_col().gap_3().child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.text("復旧方法", "Recovery method")),
        );
        for (id, method, label, help) in [
            (
                "recovery-preserve",
                Strategy::PreserveExternal,
                self.text(
                    "外部変更を保持して解除",
                    "Restore and preserve external changes",
                ),
                self.text(
                    "このアプリの設定だけを元に戻します。外部アプリの変更は保持します。",
                    "Restore this app’s entries and keep changes made by other apps.",
                ),
            ),
            (
                "recovery-reapply",
                Strategy::ReapplyTheme,
                self.text("アイコンパックを再適用", "Reapply the icon pack"),
                self.text(
                    "記録にあるパックを適用します。外部値を退避し、後の解除で戻せます。",
                    "Apply the recorded pack. External values are saved for a later restore.",
                ),
            ),
        ] {
            let unavailable =
                method == Strategy::ReapplyTheme && self.recovery.unavailable.is_some();
            choices = choices.child(div().min_w_0().flex().flex_col().gap_1()
                .child(Radio::new(id).label(label).checked(strategy == Some(method))
                    .disabled(self.operation.busy() || unavailable)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_recovery(method, cx))))
                .child(div().pl_6().text_sm().text_color(colors.muted_foreground).child(if unavailable {
                    self.text("再適用できるパックや素材がありません。解除後、パック一覧から適用してください。", "The recorded pack or its assets are unavailable. Restore first, then apply a pack from the library.")
                } else { help })));
        }
        choices
    }

    fn recovery_summary(&self, plan: &Plan, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let completed = self.recovery.completed;
        let unchanged = plan
            .entries
            .iter()
            .filter(|e| e.state == "original")
            .count();
        let count = format!(
            "{}: {} · {}: {} · {}: {}",
            if completed {
                self.text("変更した登録", "Entries changed")
            } else {
                self.text("変更する登録", "Entries to change")
            },
            plan.writes,
            self.text("外部変更", "External changes"),
            plan.conflicts,
            self.text("既に復元済み", "Already restored"),
            unchanged
        );
        let counter = div()
            .id("recovery-summary")
            .role(Role::Status)
            .aria_label(count.clone())
            .text_sm()
            .child(count);
        #[cfg(feature = "test-support")]
        let counter = counter.test_support();
        let outcome = if plan.strategy == Strategy::PreserveExternal {
            if completed {
                self.text(
                    "適用を解除しました。外部アプリの設定は保持しています。",
                    "Icons are restored. External changes were preserved.",
                )
                .to_owned()
            } else {
                self.text(
                    "パックの適用を解除します",
                    "Restore icons and deactivate the pack",
                )
                .to_owned()
            }
        } else {
            format!(
                "{}: {} · {}",
                if completed {
                    self.text("適用中", "Active")
                } else {
                    self.text("再適用するパック", "Pack to reapply")
                },
                plan.pack_label.as_deref().unwrap_or_default(),
                plan.pack_version.as_deref().unwrap_or_default()
            )
        };
        let mut summary = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .py_3()
            .border_t_1()
            .border_b_1()
            .border_color(colors.border)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(outcome))
            .child(counter);
        if !completed {
            summary = summary.child(div().text_sm().text_color(colors.muted_foreground).child(if plan.entries.is_empty() && !plan.resuming {
                self.text("復旧が必要な登録はありません。パック一覧からアイコンを適用できます。", "No registration needs recovery. Apply an icon pack from the library.")
            } else if plan.writes == 0 {
                self.text("登録値の書き換えは不要です。実行すると残っている復元記録を整理します。", "No registry values need changing. Execution clears the remaining recovery record.")
            } else if self.recovery.reviewed {
                if plan.strategy == Strategy::PreserveExternal {
                    self.text("外部アプリの変更を保持し、このアプリの登録を解除します。", "Keep external changes and restore this app’s registration.")
                } else {
                    self.text("外部設定を退避して上書きします。後で解除すると退避した値に戻ります。", "Back up and overwrite external settings. A later restore returns to those values.")
                }
            } else {
                self.text("ここでは変更予定を表示しています。下の「変更内容を確認…」から実行へ進みます。", "This is a preview. Choose Review changes… below to proceed.")
            }));
            if plan.resuming {
                summary = summary.child(self.text(
                    "未完了の復旧を再開します。",
                    "Resume the incomplete recovery.",
                ));
            }
        }
        if plan.strategy == Strategy::ReapplyTheme {
            summary = summary.child(div().text_sm().child(format!(
                "{}: {}",
                self.text("保存済みの個別指定", "Saved overrides"),
                plan.selection.changes_from(&Default::default())
            )));
            if self.recovery.details {
                if let Some(pack) = self
                    .settings
                    .packs
                    .iter()
                    .find(|p| Some(&p.id) == plan.pack_id.as_ref())
                {
                    for target in pack
                        .theme
                        .targets()
                        .into_iter()
                        .filter(|t| plan.selection.explicit(&t.id).is_some())
                        .take(50)
                    {
                        summary = summary.child(div().text_xs().truncate().child(format!(
                            "{}: {}",
                            target.id,
                            if plan.selection.explicit(&target.id) == Some(true) {
                                self.text("有効", "Enabled")
                            } else {
                                self.text("無効", "Disabled")
                            }
                        )));
                    }
                }
                let selection = serde_json::to_string(&plan.selection).unwrap_or_default();
                summary = summary.child(
                    theme::button("recovery-copy-selection", cx)
                        .small()
                        .label(self.text(
                            "保存済み選択をすべてコピー",
                            "Copy the complete saved selection",
                        ))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(selection.clone()))
                        }),
                );
            }
        }
        if self.recovery.reviewed || self.recovery.details || completed {
            let path = plan.backup.display().to_string();
            summary = summary.child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{}: {path}", self.text("退避先", "Backup"))),
                    )
                    .child(
                        theme::button("recovery-copy-backup", cx)
                            .small()
                            .label(self.text("パスをコピー", "Copy path"))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(path.clone()))
                            }),
                    ),
            );
        }
        summary
    }

    fn recovery_entries(&self, plan: &Plan, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let mut rows: Vec<_> = plan
            .entries
            .iter()
            .filter(|e| self.recovery.all || e.state == "external")
            .collect();
        rows.sort_by_key(|e| (e.state != "external", &e.key, &e.name));
        let total = rows.len();
        let pages = total.div_ceil(50).max(1);
        let page = self.recovery.page.min(pages - 1);
        let mut list = div().min_w_0().flex().flex_col().gap_2().child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.text("変更されている設定", "Changed settings")),
                )
                .child(
                    theme::button("recovery-filter", cx)
                        .small()
                        .label(if self.recovery.all {
                            self.text("外部変更のみ", "External only")
                        } else {
                            self.text("全登録", "All entries")
                        })
                        .disabled_elevated(self.operation.busy())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.recovery.all = !this.recovery.all;
                            this.recovery.page = 0;
                            cx.notify();
                        })),
                )
                .child(
                    theme::button("recovery-details", cx)
                        .small()
                        .label(if self.recovery.details {
                            self.text("詳細を閉じる", "Hide details")
                        } else {
                            self.text("詳細", "Details")
                        })
                        .selected(self.recovery.details)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.recovery.details = !this.recovery.details;
                            cx.notify();
                        })),
                ),
        );
        if rows.is_empty() {
            list = list.child(div().py_4().text_sm().text_color(colors.muted_foreground).child(self.text("外部変更はありません。「全登録」で残っている設定を確認できます。", "No external changes. Choose All entries to inspect the remaining registration.")));
        }
        for entry in rows.into_iter().skip(page * 50).take(50) {
            let kind = if entry.key.ends_with("DefaultIcon") {
                self.text("アイコン", "Icon")
            } else if entry.key.ends_with("IconHandler") {
                self.text("ハンドラー", "Handler")
            } else {
                self.text("アプリの登録", "App registration")
            };
            let class = entry
                .key
                .strip_prefix("Software\\Classes\\")
                .and_then(|k| k.split('\\').next())
                .unwrap_or_default();
            let mut row = div()
                .min_w_0()
                .py_3()
                .border_b_1()
                .border_color(colors.border)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{class} · {kind}")),
                );
            if !entry.extensions.is_empty() {
                row = row.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(entry.extensions.join(", ")),
                );
            }
            for (label, value) in [
                (self.text("現在", "Current"), &entry.current),
                (self.text("処理後", "After recovery"), &entry.planned),
            ] {
                row = row.child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .w_24()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .child(value_text(value, self.settings.language == "en")),
                        ),
                );
            }
            if self.recovery.details {
                for (label, value) in [
                    (self.text("記録の適用値", "Recorded value"), &entry.recorded),
                    (self.text("解除先", "Restore baseline"), &entry.original),
                ] {
                    row = row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!(
                                "{label}: {} · {} {}",
                                value_text(value, self.settings.language == "en"),
                                self.text("型", "Type"),
                                value
                                    .as_ref()
                                    .map(|v| v.kind.to_string())
                                    .unwrap_or_else(|| "—".into())
                            )),
                    );
                }
                let raw = serde_json::to_string(entry).unwrap_or_default();
                row = row
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .truncate()
                            .text_color(colors.muted_foreground)
                            .child(format!("{} [{}]", entry.key, entry.name)),
                    )
                    .child(
                        theme::button(
                            SharedString::from(format!(
                                "recovery-copy:{}:{}",
                                entry.key, entry.name
                            )),
                            cx,
                        )
                        .small()
                        .label(self.text("登録の詳細をコピー", "Copy registration details"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(raw.clone()))
                        }),
                    );
            }
            list = list.child(row);
        }
        if total > 50 {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        theme::button("recovery-prev", cx)
                            .small()
                            .label(self.text("前へ", "Previous"))
                            .disabled_elevated(self.operation.busy() || page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recovery.page = this.recovery.page.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(format!("{} / {pages}", page + 1))
                    .child(
                        theme::button("recovery-next", cx)
                            .small()
                            .label(self.text("次へ", "Next"))
                            .disabled_elevated(self.operation.busy() || page + 1 >= pages)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recovery.page += 1;
                                cx.notify();
                            })),
                    ),
            );
        }
        list
    }

    fn recovery_footer(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let busy = self.operation.busy();
        let mut footer = div()
            .w_full()
            .min_w_0()
            .max_w(rems(64.))
            .mx_auto()
            .px_5()
            .py_3()
            .flex()
            .flex_col()
            .gap_2();
        let message = if busy {
            String::new()
        } else {
            self.operation
                .status()
                .render(self.settings.language == "en")
        };
        let has_message = !message.is_empty();
        let status = div()
            .id("recovery-status")
            .role(Role::Status)
            .aria_label(message.clone())
            .min_w_0()
            .text_sm()
            .max_h_20()
            .overflow_y_scroll()
            .text_color(if self.operation.failed() {
                colors.danger
            } else {
                colors.muted_foreground
            })
            .child(message);
        #[cfg(feature = "test-support")]
        let status = status.test_support();
        if has_message {
            footer = footer.child(status);
        }
        if self.operation.failed() {
            footer = footer.child(div().text_sm().child(self.text(
                "変更予定を再取得して、現在の状態を確認してください。",
                "Refresh the preview to check the current state before retrying.",
            )));
        }
        if let Some(detail) = &self.recovery.unavailable
            && self.recovery.details
        {
            footer = footer.child(
                theme::button("recovery-copy-unavailable", cx)
                    .small()
                    .label(self.text(
                        "再適用できない理由をコピー",
                        "Copy why reapply is unavailable",
                    ))
                    .on_click({
                        let detail = detail.clone();
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(detail.clone()))
                        }
                    }),
            );
        }
        let mut buttons = div()
            .min_w_0()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_2()
            .child(
                theme::button("recovery-close", cx)
                    .label(if self.recovery.completed {
                        self.text("パック一覧へ戻る", "Back to packs")
                    } else if self.recovery.reviewed {
                        self.text("戻る", "Back")
                    } else {
                        self.text("キャンセル", "Cancel")
                    })
                    .disabled_elevated(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.close_recovery(window, cx))),
            );
        if busy {
            let label = if self.operation.running(Kind::ResolveConflicts) {
                self.text("復旧中…", "Recovering…")
            } else {
                self.text("登録状態を確認中…", "Checking registration…")
            };
            let progress = div()
                .id("recovery-progress")
                .role(Role::Status)
                .aria_label(label)
                .flex()
                .items_center()
                .gap_2()
                .child(Spinner::new().small())
                .child(label);
            #[cfg(feature = "test-support")]
            let progress = progress.test_support();
            buttons = buttons.child(progress);
        }
        if let Some(plan) = &self.recovery.plan
            && !self.recovery.completed
        {
            let strategy = plan.strategy;
            if !self.recovery.reviewed {
                buttons = buttons.child(
                    theme::button("recovery-refresh", cx)
                        .label(self.text("再確認", "Refresh preview"))
                        .disabled_elevated(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_conflicts(strategy, window, cx)
                        })),
                );
            }
            buttons = buttons.child(div().flex_1());
            if self.recovery.reviewed {
                let label = if strategy == Strategy::PreserveExternal {
                    self.text(
                        "外部変更を保持して解除",
                        "Restore and preserve external changes",
                    )
                    .to_owned()
                } else {
                    format!(
                        "{} {}",
                        plan.overwrites,
                        self.text("件を上書きして再適用", "entries: overwrite and reapply")
                    )
                };
                let confirmation = plan.confirmation.clone();
                buttons = buttons.child(
                    theme::button("recovery-execute", cx)
                        .label(label)
                        .when(strategy == Strategy::PreserveExternal, |button| {
                            button.primary()
                        })
                        .when(strategy == Strategy::ReapplyTheme, |button| button.danger())
                        .disabled_elevated(
                            busy || self.recovery.needs_refresh
                                || (plan.entries.is_empty() && !plan.resuming),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let Some(service) = this._guard.clone() else {
                                return;
                            };
                            let id = confirmation.clone();
                            this.recovery.needs_refresh = true;
                            this.recovery.reviewed = false;
                            if let Some(focus) = &this.recovery.focus {
                                focus.focus(window, cx);
                            }
                            this.run(
                                Kind::ResolveConflicts,
                                move || service.resolve_conflicts(strategy, &id).map(|_| ()),
                                cx,
                            );
                        })),
                );
            } else {
                buttons = buttons.child(
                    theme::button("recovery-review", cx)
                        .label(self.text("変更内容を確認…", "Review changes…"))
                        .disabled_elevated(
                            busy || self.recovery.needs_refresh
                                || (plan.entries.is_empty() && !plan.resuming),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.recovery.reviewed = true;
                            if let Some(focus) = &this.recovery.focus {
                                focus.focus(window, cx);
                            }
                            cx.notify();
                        })),
                );
            }
        }
        div()
            .flex_shrink_0()
            .min_w_0()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.sidebar)
            .child(footer.child(buttons))
    }
}
