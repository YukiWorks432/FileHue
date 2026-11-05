//! 保護設定の操作直後と保存予定を、同じ形式で比較する。
use super::*;

#[derive(Clone)]
struct Change {
    extension: String,
    provider: String,
    before: bool,
    after: bool,
}

#[derive(Clone)]
pub(super) struct Changes {
    default: Option<(bool, bool)>,
    rows: Vec<Change>,
    review: usize,
}

impl Changes {
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        self.rows
            .iter()
            .fold((0, 0, 0), |(released, protected, choices), row| {
                if row.before && !row.after {
                    (released + 1, protected, choices)
                } else if !row.before && row.after {
                    (released, protected + 1, choices)
                } else {
                    (released, protected, choices + 1)
                }
            })
    }
}

pub(super) fn compare(
    sources: &[ExistingIcon],
    old_default: bool,
    old: &IconProtectionRules,
    new_default: bool,
    new: &IconProtectionRules,
) -> Changes {
    let mut rows = Vec::new();
    let mut review = 0;
    for source in sources {
        let mut before = source.clone();
        before.update_protection(old, old_default);
        let mut after = source.clone();
        after.update_protection(new, new_default);
        review += usize::from(after.needs_review);
        let ext = &source.registration.extension;
        let provider_changed = old
            .providers
            .keys()
            .chain(new.providers.keys())
            .any(|provider| {
                old.providers
                    .get(provider)
                    .and_then(|entries| entries.get(ext))
                    != new
                        .providers
                        .get(provider)
                        .and_then(|entries| entries.get(ext))
            });
        if before.protect != after.protect
            || old.extensions.get(ext) != new.extensions.get(ext)
            || provider_changed
        {
            rows.push(Change {
                extension: ext.clone(),
                provider: source
                    .provider_name
                    .clone()
                    .unwrap_or_else(|| source.registration.class.clone()),
                before: before.protect,
                after: after.protect,
            });
        }
    }
    Changes {
        default: (old_default != new_default).then_some((old_default, new_default)),
        rows,
        review,
    }
}

impl Manager {
    pub(super) fn protection_changes_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let changes = if self.protection.changes_recommended {
            self.protection.recommendation_changes.clone().unwrap()
        } else {
            compare(
                &self.protection.sources,
                self.settings.preserve_existing_icons,
                &self.settings.icon_protection_rules,
                self.protection.draft,
                &self.protection.rules,
            )
        };
        let state = |value| {
            if value {
                self.text("保護", "Protected")
            } else {
                self.text("パック許可", "Allow pack")
            }
        };
        let mut list = div()
            .id("protection-changes-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2();
        if let Some((before, after)) = changes.default {
            list = list.child(div().child(format!(
                "{}: {} → {}",
                self.text("全体の既定", "Default"),
                state(before),
                state(after)
            )));
        }
        for row in &changes.rows {
            let extension = if row.extension.is_empty() {
                self.text("拡張子なし・名前パターン", "Extensionless and patterns")
                    .to_string()
            } else {
                format!(".{}", row.extension)
            };
            let description = if row.before == row.after {
                self.text("指定のみ変更", "Choice changed; protection unchanged")
                    .to_string()
            } else {
                format!("{} → {}", state(row.before), state(row.after))
            };
            let target = row.extension.clone();
            let label = format!("{extension} · {} · {description}", row.provider);
            list = list.child(
                div()
                    .id(SharedString::from(format!(
                        "protection-change-row-{target}"
                    )))
                    .role(Role::Group)
                    .aria_label(label.clone())
                    .border_b_1()
                    .border_color(colors.border)
                    .py_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(label))
                    .child(
                        theme::button(
                            SharedString::from(format!("protection-change-edit-{target}")),
                            cx,
                        )
                        .small()
                        .label(self.text("編集", "Edit"))
                        .disabled_elevated(self.protection.source(&target).is_none())
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.protection.changes_open = false;
                                this.protection.filter = Filter::All;
                                this.protection.pinned.clear();
                                if let Some(source) = this.protection.source(&target) {
                                    let group = source.group_id();
                                    this.protection.collapsed.remove(&group);
                                }
                                this.reset_protection_list();
                                if let Some(index) =
                                    this.protection.visible_rows.iter().position(|ix| {
                                        this.protection.rows[*ix].extension == target
                                    })
                                    && let Some(list) = &this.protection.list
                                {
                                    list.scroll_to(ListOffset {
                                        item_ix: index,
                                        offset_in_item: px(0.),
                                    });
                                }
                                this.focus.focus(window, cx);
                                cx.notify();
                            },
                        )),
                    ),
            );
            if !self.protection.changes_recommended
                && let Some(source) = self.protection.source(&row.extension)
            {
                list = list.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_3()
                        .text_sm()
                        .child(self.protection_result(source, false, cx))
                        .child(self.protection_result(source, true, cx)),
                );
            }
        }
        if changes.rows.is_empty() && changes.default.is_none() {
            list = list.child(self.text("変更はありません", "No changes"));
        }
        let mut panel = div()
            .id("protection-changes")
            .track_focus(&self.focus)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.protection.changes_open = false;
                    this.focus.focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        theme::button("protection-changes-back", cx)
                            .small()
                            .label(self.text("編集に戻る", "Back to editing"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.protection.changes_open = false;
                                this.focus.focus(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_lg()
                            .child(self.text("保護設定の変更内容", "Protection changes")),
                    ),
            );
        let mut tabs = div().px_4().flex().flex_wrap().gap_2();
        tabs = tabs.child(
            theme::button("protection-changes-pending", cx)
                .small()
                .label(self.text("保存予定", "Pending save"))
                .selected(!self.protection.changes_recommended)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.protection.changes_recommended = false;
                    cx.notify();
                })),
        );
        tabs = tabs.child(
            theme::button("protection-changes-recommended", cx)
                .small()
                .label(self.text("直前のおすすめ操作", "Last recommended action"))
                .selected(self.protection.changes_recommended)
                .disabled_elevated(self.protection.recommendation_changes.is_none())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.protection.changes_recommended = true;
                    cx.notify();
                })),
        );
        panel = panel.child(tabs).child(div().px_4().py_2().text_sm().child(if self.protection.changes_recommended {
            self.text("操作直後の結果です。その後の編集は保存予定で確認できます。", "Results immediately after the action. Later edits appear under pending save.")
        } else if self.settings.active.is_some() {
            self.text("保存すると全パック共通の保護設定を更新し、適用中のパックを再適用します。", "Saving updates protection for all packs and reapplies the active pack.")
        } else {
            self.text("保存すると全パック共通の保護設定を更新します。", "Saving updates protection for all packs.")
        })).child(div().px_4().text_sm().child(self.text("パックの対象外や画像がない種類では、パックを許可しても表示が変わらない場合があります。", "Allowing a pack does not change icons for unsupported or excluded types.")))
            .child(div().px_4().text_sm().child(format!("{}: {}", self.text("要確認", "Review needed"), changes.review)))
            .child(div().relative().flex_1().min_h_0().flex().flex_col()
                .child(list.track_scroll(&self.protection.changes_scroll))
                .vertical_scrollbar(&self.protection.changes_scroll));
        #[cfg(feature = "test-support")]
        let panel = panel.test_support();
        panel.into_any_element()
    }
}
