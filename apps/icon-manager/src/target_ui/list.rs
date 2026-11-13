use super::*;

impl Manager {
    pub(super) fn target_list(
        &self,
        pack: &file_icons_core::ImportedPack,
        all: &[Target],
        rows: &[&Target],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(test)]
        let _rows = crate::performance::section("target_rows");
        let view = cx.entity().downgrade();
        let measured = self.targets.list.clone();
        let mut list = div()
            .on_children_prepainted(move |bounds, window, cx| {
                let Some(bounds) = bounds.first() else { return };
                let _ = view.update(cx, |this, cx| {
                    if this.targets.list_width != Some(bounds.size.width) {
                        // gpui-pre 0.3.9は初回・幅変更時に未測定行の高さを消す。
                        // Listの配置後に補い、実測済みの寸法とスクロール位置は保持する。
                        measured
                            .clone()
                            .with_uniform_item_height(window.rem_size() * 4.);
                        this.targets.list_width = Some(bounds.size.width);
                        cx.notify();
                    }
                });
            })
            .id("target-rows")
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .px_3()
            .py_1();
        if rows.is_empty() {
            let query = self.targets.search.read(cx).value().to_lowercase();
            let search = TargetQuery::new(&query);
            let categories = matching_categories(&query);
            let elsewhere = all
                .iter()
                .any(|t| matches_query(t, &query, &search, &categories));
            let text = if self.protection.inspection.loading && self.targets.filter == 5 {
                self.text(
                    "保護状態を確認中です。完了すると対象を表示します。",
                    "Checking protection. Matching targets will appear when inspection completes.",
                )
            } else if elsewhere {
                self.text(
                    "このカテゴリ・状態には一致する対象がありません。全対象から検索できます。",
                    "No matches in this category or state. Search all targets to continue.",
                )
            } else {
                self.text(
                "このパックの対応定義には、検索に一致する対象がありません。保護によって一覧から隠れることはありません。",
                "No matching target is defined by this pack. Protection does not hide targets from this list.",
            )
            };
            let empty = div()
                .id("target-empty")
                .role(Role::Status)
                .aria_label(text)
                .text_sm()
                .p_4()
                .child(text);
            #[cfg(feature = "test-support")]
            let empty = empty.test_support();
            let mut actions = div().flex().flex_wrap().gap_2().child(
                theme::button("target-clear-filters", cx)
                    .small()
                    .label(self.text("検索と絞り込みを解除", "Clear search and filters"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.targets.category = None;
                        this.targets.filter = 0;
                        this.targets.reset_page();
                        this.targets
                            .search
                            .update(cx, |state, cx| state.set_value("", window, cx));
                        cx.notify();
                    })),
            );
            if elsewhere {
                actions = actions.child(
                    theme::button("target-search-all", cx)
                        .small()
                        .label(self.text("全対象から検索", "Search all targets"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.targets.category = None;
                            this.targets.filter = 0;
                            this.targets.reset_page();
                            cx.notify();
                        })),
                );
            }
            list = list.child(empty).child(actions);
        }
        let start = (self.targets.page * 100).min(rows.len().saturating_sub(1) / 100 * 100);
        let view = cx.entity().downgrade();
        let indices = self.targets.results.as_ref().unwrap().rows.clone();
        let id = pack.id.clone();
        let state = self.targets.list.clone();
        if !rows.is_empty() {
            list = list.child(
                gpui_kit::list(state.clone(), move |ix, _, cx| {
                    view.update(cx, |this, cx| {
                        let Some(target) = indices
                            .get(start + ix)
                            .and_then(|i| this.targets.all.get(*i))
                        else {
                            return div().into_any_element();
                        };
                        let Some(pack) = this.settings.packs.iter().find(|p| p.id == id) else {
                            return div().into_any_element();
                        };
                        let Some(draft) = this.targets.drafts.get(&id) else {
                            return div().into_any_element();
                        };
                        this.target_row(pack, target, &draft.value, cx)
                    })
                    .unwrap_or_else(|_| div().into_any_element())
                })
                .with_sizing_behavior(ListSizingBehavior::Auto)
                .flex_1()
                .min_h_0()
                .w_full(),
            );
        }
        #[cfg(feature = "test-support")]
        let list = list.test_support();
        // バーはスクロール内容と分け、表示領域に固定する。
        list.relative()
            .vertical_scrollbar(&state)
            .into_any_element()
    }
    fn target_row(
        &self,
        pack: &file_icons_core::ImportedPack,
        target: &Target,
        value: &Selection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(test)]
        let _row = crate::performance::section("target_row");
        let blocked = self.operation.busy() || !self.valid;
        let enabled = value.target_enabled(target);
        let toggle = target.id.clone();
        let mark = toggle.clone();
        let reset = toggle.clone();
        let focus = target.clone();
        let kind = match target.kind {
            "file" => self.text("固有名", "Filename"),
            "extension" => self.text("拡張子", "Extension"),
            _ => self.text("パターン", "Pattern"),
        };
        let state = if target.protected() {
            self.text("Windowsが管理・保護対象", "Protected; managed by Windows")
                .to_string()
        } else if value.explicit(&target.id).is_some() {
            format!("{kind} · {}", self.text("個別指定", "Override"))
        } else {
            kind.to_string()
        };
        let sample = if target.kind == "extension" {
            format!("sample.{}", target.name)
        } else {
            target.name.clone()
        };
        let outcome = self.target_outcome(pack, target, value);
        let source = self.target_source(&sample);
        let pack_icon = value
            .resolve(&pack.theme, &sample)
            .map(|(path, _)| path.to_path_buf());
        let icon = if outcome == protection::Outcome::Pack {
            pack_icon
        } else {
            source
                .and_then(|source| {
                    self.protection
                        .inspection
                        .previews
                        .as_ref()
                        .and_then(|previews| previews.paths.get(&source.registration.extension))
                })
                .cloned()
        };
        let icon_missing = icon.is_none();
        let icon_tip = if icon_missing && self.protection.inspection.previews_loading {
            self.text("アイコンの見本を読み込み中…", "Loading icon previews…")
        } else if icon_missing {
            self.text("見本を取得できません", "Icon preview unavailable")
        } else {
            self.text("表示予定のアイコン", "Planned icon")
        };
        let mut row = div()
            .flex_shrink_0()
            .w_full()
            .flex()
            .min_w_0()
            .py_2()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .items_center()
            .gap_3()
            .bg(
                if self.targets.selecting && self.targets.marked.contains(&target.id) {
                    cx.theme().colors.secondary
                } else {
                    cx.theme().colors.group_box
                },
            );
        if self.targets.selecting {
            row = row.child(
                Checkbox::new(SharedString::from(format!("target-mark-{}", target.id)))
                    .small()
                    .checked(self.targets.marked.contains(&target.id))
                    .accessibility_label(format!(
                        "{} {}",
                        self.text("行を選択", "Select row"),
                        target.name
                    ))
                    .disabled(blocked || target.protected())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.targets.marked.remove(&mark) {
                            this.targets.marked.insert(mark.clone());
                        }
                        cx.notify();
                    })),
            );
        }
        let mut icon_slot = div()
            .size_6()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center();
        if let Some(icon) = icon {
            icon_slot = icon_slot.child(
                crate::icon_preview::preview(icon, crate::icon_preview::PreviewSize::Rem(1.5))
                    .size_6(),
            );
        } else if self.protection.inspection.previews_loading {
            icon_slot = icon_slot.child(Spinner::new().small());
        } else {
            icon_slot = icon_slot.child(
                gpui_kit::component::Icon::new(IconName::TriangleAlert)
                    .size_6()
                    .text_color(cx.theme().colors.muted_foreground),
            );
        }
        let name = if target.kind == "extension" {
            format!(".{}", target.name)
        } else {
            target.name.clone()
        };
        row = row
            .child(
                icon_slot
                    .id(SharedString::from(format!("target-icon-{}", target.id)))
                    .tooltip(move |window, cx| Tooltip::new(icon_tip).build(window, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("target-name-{}", target.id)))
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .aria_label(name.clone())
                            .child(name.clone())
                            .tooltip(move |window, cx| {
                                Tooltip::new(name.clone()).build(window, cx)
                            }),
                    )
                    .child({
                        let label = self.target_outcome_label(outcome);
                        let status = div()
                            .id(SharedString::from(format!("target-result-{}", target.id)))
                            .role(Role::Status)
                            .aria_label(label)
                            .text_xs()
                            .text_color(if outcome == protection::Outcome::Protected {
                                cx.theme().colors.foreground
                            } else {
                                cx.theme().colors.muted_foreground
                            })
                            .child(format!("{state} · {label}"));
                        #[cfg(feature = "test-support")]
                        let status = status.test_support();
                        status
                    }),
            )
            .child(
                Checkbox::new(SharedString::from(format!("target-toggle-{}", target.id)))
                    .small()
                    .checked(enabled)
                    .label(self.text("対象", "Include"))
                    .accessibility_label(format!(
                        "{} {}",
                        self.text("対象に含める", "Include"),
                        target.name
                    ))
                    .tooltip(if target.protected() {
                        self.text(
                            "Windowsが管理するため変更できません",
                            "Windows manages this icon; it cannot be changed",
                        )
                        .to_string()
                    } else {
                        target.name.clone()
                    })
                    .disabled(blocked || target.protected())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.targets
                            .edit(std::slice::from_ref(&toggle), Some(!enabled));
                        this.targets.focused = Some(focus.clone());
                        cx.notify();
                    })),
            );
        if value.explicit(&target.id).is_some() {
            row = row.child(
                theme::button(
                    SharedString::from(format!("target-reset-{}", target.id)),
                    cx,
                )
                .small()
                .w_16()
                .label(self.text("解除", "Reset"))
                .accessibility_label(format!(
                    "{} {}",
                    self.text("個別指定を解除", "Reset override for"),
                    target.name
                ))
                .disabled_elevated(blocked)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.targets.edit(std::slice::from_ref(&reset), None);
                    cx.notify();
                })),
            );
        } else {
            row = row.child(div().w_16().flex_shrink_0());
        }
        if source.is_some() {
            let id = target.id.clone();
            let expanded = self.targets.protection_target.as_deref() == Some(&target.id);
            row = row.child(
                theme::button(
                    SharedString::from(format!("target-protection-{}", target.id)),
                    cx,
                )
                .small()
                .w_24()
                .label(self.text("保護", "Protection"))
                .selected(expanded)
                .toggled(expanded)
                .disabled_elevated(
                    blocked
                        || self.protection.inspection.loading
                        || self.protection.inspection.error.is_some(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.targets.protection_target = if expanded { None } else { Some(id.clone()) };
                    cx.notify();
                })),
            );
        } else {
            row = row.child(div().w_24().flex_shrink_0());
        }
        let mut area = div().w_full().flex().flex_col().flex_shrink_0().child(row);
        if self.targets.protection_target.as_deref() == Some(&target.id)
            && let Some(source) = source
        {
            area = area.child(self.target_protection_editor(source, cx));
        }
        area.into_any_element()
    }
}
