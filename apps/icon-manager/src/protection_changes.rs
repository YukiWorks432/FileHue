//! 保存予定と操作記録を区別し、指定内容と実効保護を比較する。
use super::*;
use file_icons_windows::ProtectionRule;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
struct Choice {
    individual: Option<ProtectionRule>,
    current_provider: Option<String>,
    providers: BTreeMap<String, ProtectionRule>,
    default: bool,
    effective: bool,
    review: bool,
}
#[derive(Clone, PartialEq, Eq)]
struct Change {
    extension: String,
    provider: String,
    registration_class: String,
    registration_source: String,
    reference: String,
    before: Choice,
    after: Choice,
}
#[derive(Clone, PartialEq, Eq)]
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
                if row.before.effective && !row.after.effective {
                    (released + 1, protected, choices)
                } else if !row.before.effective && row.after.effective {
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
        let ext = &source.registration.extension;
        let choice = |rules: &IconProtectionRules, default| {
            let mut result = source.clone();
            result.update_protection(rules, default);
            Choice {
                individual: rules.extensions.get(ext).cloned(),
                current_provider: source.provider_id.clone(),
                providers: rules
                    .providers
                    .iter()
                    .filter_map(|(id, entries)| {
                        entries.get(ext).map(|rule| (id.clone(), rule.clone()))
                    })
                    .collect(),
                default,
                effective: result.protect,
                review: result.needs_review,
            }
        };
        let before = choice(old, old_default);
        let after = choice(new, new_default);
        review += usize::from(after.review);
        if before.effective != after.effective
            || before.individual != after.individual
            || before.providers != after.providers
        {
            rows.push(Change {
                extension: ext.clone(),
                provider: source
                    .provider_name
                    .clone()
                    .unwrap_or_else(|| source.registration.class.clone()),
                registration_class: source.registration.class.clone(),
                registration_source: source.registration.source.clone(),
                reference: source.reference.clone(),
                before,
                after,
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
    pub(super) fn protection_view_select(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.protection.view_picker.is_none() {
            let items = Filter::ALL
                .into_iter()
                .map(|filter| ProtectionOption {
                    id: filter.id().into(),
                    label: filter.label(self).into(),
                    disabled: false,
                })
                .collect::<Vec<_>>();
            let ix = Filter::ALL
                .iter()
                .position(|filter| *filter == self.protection.filter)
                .unwrap();
            let picker = cx.new(|cx| {
                SelectState::new(
                    items,
                    Some(gpui_kit::component::IndexPath::default().row(ix)),
                    window,
                    cx,
                )
            });
            self.protection.view_subscription =
                Some(cx.subscribe(&picker, |this, _, event, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        let Some(filter) = Filter::ALL.into_iter().find(|filter| filter.id() == id)
                        else {
                            return;
                        };
                        this.protection.filter = filter;
                        this.protection.pinned.clear();
                        this.reset_protection_list();
                        cx.notify();
                    }
                }));
            self.protection.view_picker = Some(picker);
        }
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_sm().child(self.text("表示", "View")))
            .child(
                Select::new(self.protection.view_picker.as_ref().unwrap())
                    .id("protection-view-select")
                    .small()
                    .w_48()
                    .accessibility_label(self.text("表示", "View"))
                    .disabled(self.operation.busy()),
            )
            .into_any_element()
    }

    pub(super) fn protection_changes_panel(
        &mut self,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        // 操作記録は下書きと独立した値。以降の手動編集では書き換えない。
        if !self.protection.changes_recommended && self.protection.changes_pending.is_none() {
            self.protection.changes_pending = Some(compare(
                &self.protection.sources,
                self.settings.preserve_existing_icons,
                &self.settings.icon_protection_rules,
                self.protection.draft,
                &self.protection.rules,
            ));
        }
        let data = if self.protection.changes_recommended {
            self.protection.recommendation_changes.as_ref().unwrap()
        } else {
            self.protection.changes_pending.as_ref().unwrap()
        };
        if self.protection.changes_data.as_ref() != Some(data) {
            let count = data.rows.len();
            self.protection.changes_data = Some(data.clone());
            if let Some(list) = &self.protection.changes_list {
                list.reset(count);
            } else {
                self.protection.changes_list =
                    Some(ListState::new(count, ListAlignment::Top, px(0.)));
            }
        }
        let data = self.protection.changes_data.as_ref().unwrap();
        let (released, protected, choices) = data.counts();
        let default = data.default;
        let count = data.rows.len();
        let review = data.review;
        let unavailable = !self.protection.changes_recommended
            && (self.protection.loading || self.protection.error.is_some() || !self.valid);
        if self.protection.basis_picker.is_none() {
            let items = vec![
                ProtectionOption {
                    id: "pending".into(),
                    label: self.text("保存予定", "Pending save").into(),
                    disabled: false,
                },
                ProtectionOption {
                    id: "recommended".into(),
                    label: self
                        .text("直前のおすすめ操作", "Last recommended action")
                        .into(),
                    disabled: self.protection.recommendation_changes.is_none(),
                },
            ];
            let selected = usize::from(self.protection.changes_recommended);
            let picker = cx.new(|cx| {
                SelectState::new(
                    items,
                    Some(gpui_kit::component::IndexPath::default().row(selected)),
                    window,
                    cx,
                )
            });
            self.protection.basis_subscription =
                Some(cx.subscribe(&picker, |this, _, event, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.protection.changes_recommended = id == "recommended";
                        this.protection.changes_expanded.clear();
                        this.protection.changes_data = None;
                        cx.notify();
                    }
                }));
            self.protection.basis_picker = Some(picker);
        }
        let save_scope = if self.protection.changes_recommended {
            self.text(
                "操作直後の記録です。保存対象は現在の下書きです。",
                "Action snapshot. Saving commits the current draft.",
            )
            .to_string()
        } else if let Some(active) = &self.settings.active {
            let name = self
                .settings
                .packs
                .iter()
                .find(|pack| &pack.id == active)
                .map(|pack| pack.label.as_str())
                .unwrap_or(active);
            if self.settings.language == "en" {
                format!("Saves protection for all packs and reapplies {name}.")
            } else {
                format!("全パック共通の保護設定を保存し、適用中の「{name}」へ再適用します。")
            }
        } else {
            self.text(
                "全パック共通の保護設定を保存します。",
                "Saves protection for all packs.",
            )
            .to_string()
        };
        let mut panel = theme::card(cx)
            .id("protection-changes")
            .mx_4()
            .mb_3()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .p_3()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .child(div().text_sm().child(self.text("比較する内容", "Compare")))
                            .child(
                                Select::new(self.protection.basis_picker.as_ref().unwrap())
                                    .id("protection-changes-basis")
                                    .small()
                                    .w_64()
                                    .accessibility_label(self.text("比較する内容", "Compare")),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .when(self.protection.changes_recommended, |notice| {
                                notice
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .child(save_scope),
                    )
                    .when(self.protection.recommendation_changes.is_none(), |p| {
                        p.child(div().text_xs().text_color(colors.muted_foreground).child(
                            self.text(
                                "おすすめ操作の記録はまだありません。",
                                "No recommended action has been recorded.",
                            ),
                        ))
                    }),
            );
        if unavailable {
            let message = if self.protection.loading {
                self.text("保存予定を確認中…", "Checking pending changes…")
            } else if !self.valid {
                self.text(
                    "保存済み設定を読み込めないため、保存予定を確認できません。",
                    "Pending changes unavailable because saved settings could not be loaded.",
                )
            } else {
                self.text(
                    "保存予定を確認できません。「再調査」でやり直してください。",
                    "Pending changes unavailable. Choose Refresh to retry.",
                )
            };
            let status = div()
                .id("protection-changes-status")
                .role(Role::Status)
                .aria_label(message)
                .p_4()
                .child(message);
            #[cfg(feature = "test-support")]
            let status = status.test_support();
            panel = panel.child(status);
        } else {
            let summary = format!(
                "{} {released}  ·  {} {protected}  ·  {} {choices}",
                self.text("保護解除", "Released"),
                self.text("保護追加", "Protected"),
                self.text("指定更新", "Choices updated")
            );
            let summary = div()
                .id("protection-changes-summary")
                .role(Role::Status)
                .aria_label(summary.clone())
                .px_3()
                .pb_3()
                .text_sm()
                .child(summary);
            #[cfg(feature = "test-support")]
            let summary = summary.test_support();
            panel = panel.child(summary);
            if review > 0 {
                panel = panel.child(div().px_3().pb_2().text_sm().child(format!(
                    "{}: {review}",
                    self.text("要確認", "Review needed")
                )));
            }
            if let Some((before, after)) = default {
                panel = panel.child(div().px_3().pb_2().text_sm().child(format!(
                    "{}: {} → {}",
                    self.text("全体の既定", "Default"),
                    self.protection_state_text(before),
                    self.protection_state_text(after)
                )));
            }
            if count == 0 && default.is_none() {
                panel = panel.child(div().p_4().text_color(colors.muted_foreground).child(
                    if self.protection.changes_recommended {
                        self.text(
                            "この操作による変更はありません",
                            "This action made no changes",
                        )
                    } else {
                        self.text("未保存の変更はありません", "No unsaved changes")
                    },
                ));
            }
            if !compact && count > 0 {
                panel = panel.child(
                    div()
                        .px_3()
                        .py_2()
                        .border_t_1()
                        .border_b_1()
                        .border_color(colors.border)
                        .flex()
                        .gap_3()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(div().flex_1().min_w_0().child(self.text("対象", "Target")))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.text("変更前", "Before")),
                        )
                        .child(div().flex_1().min_w_0().child(self.text("変更後", "After")))
                        .child(div().w_32().child(self.text("変更の種類", "Change")))
                        .child(div().w_32()),
                );
            }
            let state = self.protection.changes_list.as_ref().unwrap().clone();
            let metrics = (
                window.viewport_size().width,
                window.rem_size(),
                self.settings.language.clone(),
            );
            if self.protection.changes_metrics.as_ref() != Some(&metrics) {
                state.remeasure();
                self.protection.changes_metrics = Some(metrics);
            }
            let view = cx.entity().downgrade();
            panel = panel.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        list(state.clone(), move |ix, _, cx| {
                            view.update(cx, |this, cx| this.protection_change_row(ix, compact, cx))
                                .unwrap_or_else(|_| div().into_any_element())
                        })
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .with_sizing_behavior(ListSizingBehavior::Auto),
                    )
                    .vertical_scrollbar(&state),
            );
        }
        #[cfg(feature = "test-support")]
        let panel = panel.test_support();
        panel.into_any_element()
    }
    fn protection_state_text(&self, protect: bool) -> &'static str {
        if protect {
            self.text("保護", "Protected")
        } else {
            self.text("パック許可", "Allow pack")
        }
    }
    fn protection_choice_text(&self, choice: &Choice) -> String {
        let mut parts = Vec::new();
        if let Some(rule) = &choice.individual {
            parts.push(format!(
                "{}: {}",
                self.text("個別", "Individual"),
                self.protection_state_text(rule.protect)
            ));
        } else {
            parts.push(
                self.text("基本設定に従う", "Inherit base setting")
                    .to_string(),
            );
        }
        for (provider, rule) in &choice.providers {
            parts.push(format!(
                "{}: {}",
                if choice.current_provider.as_ref() == Some(provider) {
                    self.text("ソフトの基本設定", "Software base")
                } else {
                    self.text("以前のソフト指定", "Previous software choice")
                },
                self.protection_state_text(rule.protect)
            ));
            if choice.providers.len() > 1 {
                parts.push(provider.clone());
            }
        }
        if choice.individual.is_none() && choice.providers.is_empty() {
            parts.push(format!(
                "{}: {}",
                self.text("全体の既定", "Default"),
                self.protection_state_text(choice.default)
            ));
        }
        parts.push(format!(
            "{}: {}{}",
            self.text("実効保護", "Effective"),
            self.protection_state_text(choice.effective),
            if choice.review {
                self.text("（要確認）", " (review needed)")
            } else {
                ""
            }
        ));
        parts.join("\n")
    }
    fn protection_change_row(
        &mut self,
        ix: usize,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = self.protection.changes_data.as_ref().unwrap().rows[ix].clone();
        let colors = cx.theme().colors;
        let extension = if row.extension.is_empty() {
            self.text(
                "拡張子のないファイルなど",
                "Extensionless files and patterns",
            )
            .to_string()
        } else {
            format!(".{}", row.extension)
        };
        let before = self.protection_choice_text(&row.before);
        let after = self.protection_choice_text(&row.after);
        let kind = if row.before.effective != row.after.effective {
            if row.after.effective {
                self.text("保護追加", "Protection added")
            } else {
                self.text("保護解除", "Protection released")
            }
            .to_string()
        } else {
            let same_choices = row.before.individual.as_ref().map(|r| r.protect)
                == row.after.individual.as_ref().map(|r| r.protect)
                && row
                    .before
                    .providers
                    .iter()
                    .map(|(id, r)| (id, r.protect))
                    .eq(row.after.providers.iter().map(|(id, r)| (id, r.protect)));
            format!(
                "{} · {}{}",
                if same_choices {
                    self.text("確認情報を更新", "Registration refreshed")
                } else {
                    self.text("指定更新", "Choice updated")
                },
                self.protection_state_text(row.after.effective),
                self.text("は継続", " unchanged")
            )
        };
        let target = row.extension.clone();
        let expanded = self.protection.changes_expanded.contains(&target);
        let actions = div()
            .w_32()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                theme::button(
                    SharedString::from(format!("protection-change-edit-{target}")),
                    cx,
                )
                .small()
                .label(self.text("設定を開く", "Open settings"))
                .disabled_elevated(
                    self.protection.source(&target).is_none() || self.operation.busy(),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.protection.filter = Filter::All;
                    this.protection.view_picker = None;
                    this.protection.pinned.clear();
                    if let Some(source) = this.protection.source(&target) {
                        let group = display_group(source);
                        this.protection.collapsed.remove(&group);
                    }
                    this.reset_protection_list();
                    if let Some(index) = this
                        .protection
                        .visible_rows
                        .iter()
                        .position(|ix| this.protection.rows[*ix].extension == target)
                        && let Some(list) = &this.protection.list
                    {
                        list.scroll_to(ListOffset {
                            item_ix: index,
                            offset_in_item: px(0.),
                        });
                    }
                    this.focus.focus(window, cx);
                    cx.notify();
                })),
            )
            .child({
                let target = row.extension.clone();
                theme::button(
                    SharedString::from(format!("protection-change-details-{target}")),
                    cx,
                )
                .small()
                .label(self.text("詳細", "Details"))
                .selected(expanded)
                .toggled(expanded)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.protection.changes_expanded.remove(&target) {
                        this.protection.changes_expanded.insert(target.clone());
                    }
                    if let Some(list) = &this.protection.changes_list {
                        list.remeasure();
                    }
                    cx.notify();
                }))
            });
        let identity = div()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(extension.clone()),
            )
            .when(!row.extension.is_empty(), |p| {
                p.child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(row.provider.clone()),
                )
            });
        let before_column = div()
            .flex_1()
            .min_w_0()
            .when(compact, |p| {
                p.child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(self.text("変更前", "Before")),
                )
            })
            .child(before.clone());
        let after_column = div()
            .flex_1()
            .min_w_0()
            .when(compact, |p| {
                p.child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(self.text("変更後", "After")),
                )
            })
            .child(after.clone());
        let label = format!(
            "{extension}: {before} → {after} · {kind} · {} → {}",
            self.protection_state_text(row.before.effective),
            self.protection_state_text(row.after.effective)
        );
        let mut element = div()
            .id(SharedString::from(format!(
                "protection-change-row-{}",
                row.extension
            )))
            .role(Role::Group)
            .aria_label(label)
            .border_b_1()
            .border_color(colors.border)
            .flex()
            .flex_col()
            .text_sm()
            .child(if compact {
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().flex().gap_3().child(identity).child(actions))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .min_w_0()
                            .child(before_column)
                            .child(after_column),
                    )
                    .child(kind)
            } else {
                div()
                    .p_3()
                    .flex()
                    .gap_3()
                    .items_start()
                    .child(identity)
                    .child(before_column)
                    .child(after_column)
                    .child(div().w_32().flex_shrink_0().child(kind))
                    .child(actions)
            });
        if expanded {
            let mut detail = theme::inset(cx).p_3().flex().flex_col().gap_2().text_xs();
            if !self.protection.changes_recommended
                && let Some(source) = self.protection.source(&row.extension)
            {
                detail = detail.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_3()
                        .child(self.protection_result(source, false, cx))
                        .child(self.protection_result(source, true, cx)),
                );
            } else {
                detail = detail.child(self.text(
                    "操作記録には画像を保存していません。",
                    "Images are not stored in the action snapshot.",
                ));
            }
            detail = detail.child(format!(
                "{}: {}\n{}: {}\n{}: {}",
                self.text("クラス", "Class"),
                row.registration_class,
                self.text("登録位置", "Registration"),
                row.registration_source,
                self.text("元の指定", "Original reference"),
                row.reference
            ));
            element = element.child(detail);
        }
        #[cfg(feature = "test-support")]
        let element = element.test_support();
        element.into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::{ExistingIcon, IconExclusion, IconProtectionRules, ProtectionReason, compare};
    fn source(extension: &str) -> ExistingIcon {
        ExistingIcon {
            registration: IconExclusion {
                extension: extension.into(),
                class: "Example.Code".into(),
                source: "HKCU".into(),
                reason: ProtectionReason::DefaultIcon,
            },
            provider_id: Some("example".into()),
            provider_name: Some("Example editor".into()),
            provider_inferred: false,
            open_app: None,
            fingerprint: "confirmed-registration".into(),
            reference: "example.dll,0".into(),
            location: None,
            provider_location: None,
            protect: true,
            needs_review: false,
            always_protected: extension == "exe",
        }
    }
    #[test]
    fn captures_inheritance_and_confirmation_without_counting_them_as_releases() {
        let ts = source("ts");
        let mut before = IconProtectionRules::default();
        before.set_provider(std::slice::from_ref(&ts), "example", Some(false));
        before.set_extension(&ts, Some(false));
        let mut after = before.clone();
        after.set_extension(&ts, None);
        let diff = compare(std::slice::from_ref(&ts), true, &before, true, &after);
        assert_eq!(diff.counts(), (0, 0, 1));
        assert_eq!(
            diff.rows[0]
                .before
                .individual
                .as_ref()
                .map(|rule| rule.protect),
            Some(false)
        );
        assert!(diff.rows[0].after.individual.is_none());
        assert!(!diff.rows[0].after.effective);
        assert!(!diff.rows[0].after.providers["example"].protect);
        // 確認情報だけが更新され、常時保護によって実効保護は変わらない例。
        let exe = source("exe");
        before = IconProtectionRules::default();
        before.set_extension(&exe, Some(true));
        before.extensions.get_mut("exe").unwrap().fingerprint = "previous-registration".into();
        after = before.clone();
        after.set_extension(&exe, Some(true));
        let diff = compare(&[exe], true, &before, true, &after);
        assert_eq!(diff.counts(), (0, 0, 1));
        assert!(diff.rows[0].before.review);
        assert!(!diff.rows[0].after.review);
        assert!(diff.rows[0].before.effective && diff.rows[0].after.effective);
    }
    #[test]
    fn default_and_removed_override_are_separate_targets_and_snapshots_keep_their_values() {
        let ts = source("ts");
        let media = source("m2ts");
        let mut saved = IconProtectionRules::default();
        saved.set_extension(&media, Some(false));
        let mut draft = saved.clone();
        draft.set_extension(&ts, Some(false));
        draft.set_extension(&media, None);
        let snapshot = compare(&[ts.clone(), media.clone()], true, &saved, true, &draft);
        assert_eq!(snapshot.counts(), (1, 1, 0));
        draft.set_extension(&ts, Some(true));
        let pending = compare(&[ts, media], true, &saved, false, &draft);
        assert_eq!(pending.default, Some((true, false)));
        assert_eq!(pending.counts(), (0, 0, 2));
        assert!(!snapshot.rows[0].after.effective);
        assert!(pending.rows[0].after.effective);
    }
}
