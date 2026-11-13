use super::*;

impl Manager {
    pub(super) fn target_details(
        &self,
        pack: &file_icons_core::ImportedPack,
        saved: &Selection,
        value: &Selection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut detail = theme::card(cx)
            .id("target-details")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(cx.theme().colors.group_box)
            .child(self.target_auxiliary_header("表示を確認", "Check icons", cx));
        if let Some(target) = &self.targets.focused {
            detail = detail.child(div().text_sm().child(format!(
                "{}: {} · {}",
                self.text("選択した対象", "Selected target"),
                target.name,
                if value.target_enabled(target) {
                    self.text("対象に含む", "Included")
                } else {
                    self.text(
                        "対象外・Windowsの元の表示",
                        "Excluded; original Windows icon",
                    )
                }
            )));
        }
        let typed = self.preview.read(cx).value().to_string();
        detail = detail
            .child(div().text_sm().child(self.text(
                "ファイル名で変更前と予定を確認",
                "Compare saved and draft results for a filename",
            )))
            .child(
                Input::new(&self.preview)
                    .id("target-preview")
                    .aria_label(self.text("結果を比較するファイル名", "Filename to compare"))
                    .w_full(),
            );
        if !typed.is_empty() {
            for (key, label, selection) in [
                ("target-current", self.text("保存済み", "Saved"), saved),
                ("target-planned", self.text("予定", "Draft"), value),
            ] {
                let result = selection.resolve(&pack.theme, &typed);
                let protection = self.target_preview_protected(&typed, key == "target-current");
                let protected = protection == Some(true);
                let inspected = !self.protection.inspection.loading
                    && self.protection.inspection.error.is_none()
                    && protection.is_some();
                let use_pack = result.is_some() && !protected && inspected;
                let reason = if file_icons_core::selection::protected_path(&typed) {
                    self.text(
                        "Windowsが管理する保護対象",
                        "Protected file type managed by Windows",
                    )
                } else if !selection.enabled(&typed) {
                    self.text("利用者設定で無効", "Disabled by selection")
                } else if !inspected {
                    self.text(
                        "保護状態を確認できていません",
                        "Protection has not been confirmed",
                    )
                } else if protected {
                    self.text("既存アイコンを保護中", "Original icon protected")
                } else {
                    result
                        .map(|(_, reason)| match reason {
                            "file_name" | "language_file_name" => {
                                self.text("固有名の規則", "Filename rule")
                            }
                            "language_pattern" => self.text("パターンの規則", "Pattern rule"),
                            _ => self.text("拡張子の規則", "Extension rule"),
                        })
                        .unwrap_or(self.text("未対応", "Unsupported"))
                };
                let text = format!(
                    "{label}: {typed} · {reason} · {}",
                    if use_pack {
                        self.text("パックのアイコン", "Pack icon")
                    } else {
                        self.text("Windowsの元の表示", "Original Windows icon")
                    }
                );
                let row = theme::inset(cx)
                    .p_3()
                    .id(key)
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .child(text);
                let row =
                    if let Some((icon, _)) = result.filter(|_| use_pack) {
                        row.child(crate::icon_preview::preview(icon.to_path_buf(), rems(2.)))
                    } else {
                        row.children(
                            self.target_source(&typed)
                                .and_then(|source| {
                                    self.protection.inspection.previews.as_ref().and_then(
                                        |previews| {
                                            previews.paths.get(&source.registration.extension)
                                        },
                                    )
                                })
                                .map(|icon| crate::icon_preview::preview(icon.clone(), rems(2.))),
                        )
                    };
                #[cfg(feature = "test-support")]
                let row = row.test_support();
                detail = detail.child(row);
            }
        }
        #[cfg(feature = "test-support")]
        let detail = detail.test_support();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(detail.track_scroll(&self.targets.detail_scroll))
            .vertical_scrollbar(&self.targets.detail_scroll)
            .into_any_element()
    }
    fn target_auxiliary_header(
        &self,
        ja: &'static str,
        en: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .child(
                theme::button("target-auxiliary-back", cx)
                    .small()
                    .label(self.text("対象一覧に戻る", "Back to targets"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.targets.auxiliary = None;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.text(ja, en)),
            )
    }
    pub(super) fn target_copy(
        &self,
        pack: &file_icons_core::ImportedPack,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let blocked = self.operation.busy() || !self.valid;
        let id = pack.id.clone();
        let mut detail = theme::card(cx).id("target-copy-panel").flex_1().min_w_0().min_h_0()
            .overflow_y_scroll().flex().flex_col().gap_3().p_4()
            .child(self.target_auxiliary_header("別パックの設定をコピー", "Copy settings from another pack", cx))
            .child(div().text_sm().child(format!("{}: {} · {}", self.text("引き継ぎ先", "Destination"), pack.label, pack.version)))
            .child(div().text_sm().text_color(cx.theme().colors.muted_foreground)
                .child(self.text("一致する対象の保存済み設定を下書きへコピーします。コピー先だけにある対象は保持します。", "Copy matching saved settings into this draft. Targets unique to this pack are kept.")));
        let supported: BTreeSet<_> = self.targets.all.iter().map(|target| &target.id).collect();
        let mut sources: Vec<_> = self.settings.packs.iter().filter(|p| p.id != id).collect();
        sources.sort_by_key(|source| {
            crate::library_ui::pack_family(source) != crate::library_ui::pack_family(pack)
        });
        for source in sources {
            let source_targets = source.theme.targets();
            // 表示には件数だけを使い、選択全体の構築と保持は確定操作まで遅らせる。
            let unmatched = source_targets
                .iter()
                .filter(|target| !target.protected() && !supported.contains(&target.id))
                .count();
            let destination = id.clone();
            let source_id = source.id.clone();
            let label = format!(
                "{}: {} ({} {})",
                self.text("コピー元", "Copy from"),
                format_args!("{} · {}", source.label, source.version),
                unmatched,
                self.text("件は対象外", "unmatched")
            );
            let mut candidate = theme::inset(cx)
                .flex()
                .flex_col()
                .items_start()
                .gap_3()
                .p_3()
                .flex_shrink_0()
                .child(div().text_sm().child(label))
                .child(
                    theme::button(SharedString::from(format!("target-copy-{}", source.id)), cx)
                        .small()
                        .label(self.text("一致する設定を引き継ぐ", "Inherit matching settings"))
                        .disabled_elevated(blocked)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.operation.busy() || !this.valid {
                                return;
                            }
                            let Some(source) =
                                this.settings.packs.iter().find(|p| p.id == source_id)
                            else {
                                return;
                            };
                            let Some(pack) =
                                this.settings.packs.iter().find(|p| p.id == destination)
                            else {
                                return;
                            };
                            if let Some(draft) = this.targets.drafts.get_mut(&destination) {
                                let empty = Selection::default();
                                let selection =
                                    this.settings.selections.get(&source_id).unwrap_or(&empty);
                                let (copied, _) =
                                    selection.copy_to(&source.theme, &pack.theme, &draft.value);
                                draft.replace(copied, &this.targets.all);
                            }
                            this.targets.results = None;
                            this.targets.auxiliary = None;
                            cx.notify();
                        })),
                );
            if source.version != pack.version
                && crate::library_ui::pack_family(source) == crate::library_ui::pack_family(pack)
            {
                let source_ids: BTreeSet<_> =
                    source_targets.iter().map(|target| &target.id).collect();
                let added: Vec<_> = self
                    .targets
                    .all
                    .iter()
                    .filter(|target| !source_ids.contains(&target.id))
                    .collect();
                let removed: Vec<_> = source_targets
                    .iter()
                    .filter(|target| !supported.contains(&target.id))
                    .collect();
                let summary = format!(
                    "{} {} · {} {} · {} {}",
                    self.text("一致", "Matching"),
                    supported.len() - added.len(),
                    self.text("追加", "Added"),
                    added.len(),
                    self.text("削除", "Removed"),
                    removed.len()
                );
                let summary_row = div()
                    .id(SharedString::from(format!("version-summary-{}", source.id)))
                    .role(Role::Status)
                    .aria_label(summary.clone())
                    .text_sm()
                    .child(summary);
                #[cfg(feature = "test-support")]
                let summary_row = summary_row.test_support();
                let source_id = source.id.clone();
                let expanded = self.targets.copy_changes.as_deref() == Some(&source.id);
                candidate = candidate.child(summary_row).child(
                    theme::button(
                        SharedString::from(format!("version-changes-{}", source.id)),
                        cx,
                    )
                    .small()
                    .label(self.text(
                        "追加・削除された対象を確認",
                        "Review added and removed targets",
                    ))
                    .selected(expanded)
                    .toggled(expanded)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.targets.copy_changes = if expanded {
                            None
                        } else {
                            Some(source_id.clone())
                        };
                        cx.notify();
                    })),
                );
                if expanded {
                    let changes = added
                        .iter()
                        .map(|target| ("+", target))
                        .chain(removed.iter().map(|target| ("−", target)))
                        .map(|(sign, target)| {
                            format!(
                                "{sign} {}{}",
                                if target.kind == "extension" { "." } else { "" },
                                target.name
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let changes = if changes.is_empty() {
                        self.text("追加・削除はありません", "No added or removed targets")
                            .to_owned()
                    } else {
                        changes
                    };
                    let changes = div()
                        .id("version-changes-list")
                        .role(Role::Status)
                        .aria_label(changes.clone())
                        .text_sm()
                        .whitespace_normal()
                        .child(changes);
                    #[cfg(feature = "test-support")]
                    let changes = changes.test_support();
                    candidate = candidate.child(changes);
                }
            }
            detail = detail.child(candidate);
        }
        if self.settings.packs.len() < 2 {
            detail = detail.child(div().text_sm().child(self.text(
                "コピー元となる別のパックがありません。",
                "No other packs are available to copy from.",
            )));
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(detail.track_scroll(&self.targets.detail_scroll))
            .vertical_scrollbar(&self.targets.detail_scroll)
            .into_any_element()
    }
}
