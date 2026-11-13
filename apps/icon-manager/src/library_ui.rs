//! 取得済みパックの一覧、取り込み候補、削除の意思確認。
use super::*;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    assets::IconName,
    component::{Icon, Sizable, WindowExt, button::ButtonVariant},
};

pub(super) fn pack_control(action: &str, id: &str) -> SharedString {
    format!("pack:{id}:{action}").into()
}

impl Manager {
    pub(super) fn show_library(
        &mut self,
        browsing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.operation.clear_result();
        self.browsing = browsing;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn library(&self, cx: &mut Context<Self>) -> Div {
        let mut body = div().flex().flex_col().gap_4();
        if !self.valid {
            return body.child(theme::card(cx).p_4().child(self.text(
                "設定を読み取れないため、取得済みパックを表示できません。設定ファイルは自動で変更されません。",
                "Downloaded packs cannot be displayed because settings could not be read. Settings are not changed automatically.",
            )));
        }
        if let Some((root, choices)) = &self.pending {
            let mut picker = theme::card(cx)
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .child(self.text(
                    "取り込むテーマを選択してください。",
                    "Select a theme to import.",
                ));
            for (index, choice) in choices.iter().enumerate() {
                let root = root.clone();
                let id = choice.id.clone();
                picker = picker.child(
                    theme::button(("theme", index), cx)
                        .icon(IconName::Palette)
                        .label(choice.label.clone())
                        .disabled_elevated(self.operation.busy() || !self.valid)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.import(root.clone(), id.clone(), cx)
                        })),
                );
            }
            body = body.child(
                picker.child(
                    theme::button("cancel-import", cx)
                        .small()
                        .label(self.text("取り込みをキャンセル", "Cancel import"))
                        .disabled_elevated(self.operation.busy())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pending = None;
                            cx.notify();
                        })),
                ),
            );
        }
        body = body.child(self.installed_packs(cx));
        if !self.settings.pending_pack_removals.is_empty() {
            body = body.child(
                theme::button("cleanup-packs", cx)
                    .icon(IconName::RefreshCw)
                    .label(self.text(
                        "残ったパック素材の削除を再試行",
                        "Retry removing remaining pack files",
                    ))
                    .disabled_elevated(self.operation.busy() || !self.valid)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(manager) = this._guard.clone() else {
                            return;
                        };
                        this.run(Kind::Cleanup, move || manager.cleanup_packs(), cx);
                    })),
            );
        }
        body
    }

    fn confirm_remove(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.busy() || !self.valid || self.settings.active.as_deref() == Some(id) {
            return;
        }
        let Some(pack) = self.settings.packs.iter().find(|pack| pack.id == id) else {
            return;
        };
        let title = if self.settings.language == "en" {
            format!("Remove “{}”?", pack.label)
        } else {
            format!("「{}」を削除しますか？", pack.label)
        };
        let description = self.text(
            "このアプリに取り込んだパックと変換済みアイコンを削除します。元の取り込みフォルダーは保持します。",
            "Remove this imported pack and its converted icons. The original import folder is kept.",
        );
        let cancel = self.text("キャンセル", "Cancel");
        let remove = self.text("削除する", "Remove");
        let id = id.to_owned();
        let owner = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let owner = owner.clone();
            let id = id.clone();
            dialog
                // 短い意思確認は即時表示し、操作対象を動かさない。
                .entrance(gpui_kit::component::dialog::DialogEntrance::None)
                .bg(cx.theme().colors.popover)
                .title(title.clone())
                .description(description)
                .confirm()
                .cancel_text(cancel)
                .ok_text(remove)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        // 確認中の外部変更も管理処理側の排他と再読込で検査する。
                        if this.operation.busy()
                            || !this.valid
                            || this.settings.active.as_deref() == Some(&id)
                        {
                            return;
                        }
                        let Some(manager) = this._guard.clone() else {
                            return;
                        };
                        let id = id.clone();
                        this.run(Kind::Remove, move || manager.remove(&id), cx);
                    });
                    true
                })
        });
    }

    pub(super) fn installed_packs(&self, cx: &mut Context<Self>) -> Div {
        let colors = cx.theme().colors;
        let blocked = self.operation.busy() || !self.valid;
        let mut list = div().flex().flex_col().gap_3();
        if self.settings.packs.is_empty() {
            list = list.child(
                div()
                    .py_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(
                        Icon::new(IconName::PackageOpen)
                            .size_8()
                            .text_color(colors.muted_foreground),
                    )
                    .child(self.text("最初のアイコンパックを追加", "Add your first icon pack"))
                    .child(
                        theme::button("browse-empty", cx)
                            .icon(IconName::Search)
                            .label(self.text("パックを探す", "Browse packs"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_library(true, window, cx)
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(self.text(
                            "「パックを探す」から取得するか、手元の拡張フォルダーを取り込めます。",
                            "Browse the catalog or import an extension folder.",
                        )),
                    ),
            );
        }
        for pack in &self.settings.packs {
            let id = pack.id.clone();
            let id_for_remove = id.clone();
            let detail_id = id.clone();
            let target_id = id.clone();
            let update_id = id.clone();
            let has_previous = self.settings.packs.iter().any(|source| {
                source.id != pack.id
                    && source.version != pack.version
                    && pack_family(source) == pack_family(pack)
            });
            let active = self.settings.active.as_ref() == Some(&pack.id);
            let expanded = self.selected.as_ref() == Some(&pack.id);
            let default_selection = file_icons_core::selection::Selection::default();
            let selection = self
                .settings
                .selections
                .get(&pack.id)
                .unwrap_or(&default_selection);
            let rules = pack.theme.rules();
            let preferred = ["js", "ts", "py", "rs", "html"]
                .into_iter()
                .map(|ext| (format!("sample.{ext}"), format!(".{ext}")));
            let filenames = rules
                .files
                .keys()
                .chain(rules.language_files.keys())
                .map(|name| (name.clone(), name.clone()));
            let extensions = rules
                .extensions
                .keys()
                .chain(rules.language_extensions.keys())
                .map(|ext| {
                    if let Some((parent, ext)) = ext.split_once('/') {
                        (
                            format!("{parent}/sample.{ext}"),
                            format!("{parent}/*.{ext}"),
                        )
                    } else {
                        (format!("sample.{ext}"), format!(".{ext}"))
                    }
                });
            let mut seen = std::collections::BTreeSet::new();
            let samples = preferred
                .chain(filenames)
                .chain(extensions)
                .filter_map(|(name, label)| {
                    if !seen.insert(name.clone()) {
                        return None;
                    }
                    selection
                        .resolve(&pack.theme, &name)
                        .map(|(path, _)| (name, label, path.to_path_buf()))
                })
                .take(5)
                .collect::<Vec<_>>();
            let has_samples = !samples.is_empty();
            let metadata = format!(
                "{} · {} {} · {} {}",
                pack.version,
                pack.theme.rules().files.len(),
                self.text("ファイル名", "filenames"),
                pack.theme.rules().extensions.len(),
                self.text("拡張子", "extensions")
            );
            let full_title = pack.label.clone();
            let title = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id(pack_control("title", &pack.id))
                        .truncate()
                        .aria_label(full_title.clone())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(full_title.clone())
                        .tooltip(move |window, cx| {
                            Tooltip::new(full_title.clone())
                                .max_w(rems(28.))
                                .whitespace_normal()
                                .build(window, cx)
                        }),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child(metadata),
                );
            let mut identity = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .child(
                    Icon::new(IconName::Package)
                        .size_5()
                        .flex_shrink_0()
                        .text_color(colors.muted_foreground),
                )
                .child(title)
                .when(active, |row| {
                    row.child(
                        div()
                            .text_sm()
                            .flex_shrink_0()
                            .child(self.text("適用中", "Applied")),
                    )
                });
            if has_samples {
                let mut preview = div().flex().flex_wrap().items_center().gap_2();
                for (name, label, path) in samples {
                    let sample = div()
                        .id(pack_control(&format!("sample:{name}"), &pack.id))
                        .role(Role::Image)
                        .aria_label(name.clone())
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .child(crate::icon_preview::preview(path, rems(1.5)))
                        .child(
                            div()
                                .text_xs()
                                .max_w(rems(6.))
                                .truncate()
                                .text_color(colors.muted_foreground)
                                .child(label),
                        );
                    #[cfg(feature = "test-support")]
                    let sample = sample.test_support();
                    preview = preview.child(sample);
                }
                identity = identity.child(preview);
            }
            let actions = div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(
                    theme::button(pack_control("apply", &pack.id), cx)
                        .small()
                        .primary()
                        .min_w(rems(7.))
                        .icon(if active {
                            IconName::Check
                        } else {
                            IconName::Monitor
                        })
                        .label(if active {
                            self.text("適用中", "Applied")
                        } else {
                            self.text("適用する", "Apply")
                        })
                        .disabled_elevated(blocked || active)
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(id.clone(), cx))),
                )
                .child(
                    theme::button(pack_control("details", &pack.id), cx)
                        .small()
                        .icon(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .label(self.text("プレビュー・利用条件", "Preview and licenses"))
                        .selected(expanded)
                        .toggled(expanded)
                        .disabled_elevated(self.operation.busy() && !expanded)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_details(&detail_id, window, cx)
                        })),
                )
                .child(
                    theme::button(pack_control("targets", &pack.id), cx)
                        .small()
                        .label(self.text("適用対象を選ぶ…", "Choose targets…"))
                        .disabled_elevated(blocked)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_targets(&target_id, cx);
                            this.focus.focus(window, cx);
                        })),
                )
                .child(div().flex_1())
                .child(
                    theme::button(pack_control("remove-pack", &pack.id), cx)
                        .small()
                        .icon(IconName::Trash)
                        .label(self.text("削除…", "Remove…"))
                        .tooltip(if active {
                            self.text(
                                "適用中は削除できません。",
                                "Applied packs cannot be removed.",
                            )
                        } else {
                            self.text("パックを削除します。", "Remove this pack.")
                        })
                        .disabled_elevated(blocked || active)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm_remove(&id_for_remove, window, cx)
                        })),
                );
            let mut row = theme::card(cx)
                .p_4()
                .border_color(if expanded { colors.ring } else { colors.border })
                .flex()
                .flex_col()
                .gap_3()
                .flex_shrink_0()
                .child(identity)
                .child(actions);
            if has_previous {
                row = row.child(theme::inset(cx).p_3().flex().flex_col().gap_2()
                    .child(div().text_sm().child(self.text(
                        "別バージョンの設定を引き継げます。追加・削除された対象を確認し、適用前に保存できます。",
                        "Settings from another version are available. Review added and removed targets before saving or applying.",
                    )))
                    .child(theme::button(pack_control("update-settings", &pack.id), cx).small()
                        .label(self.text("別バージョンの設定を確認…", "Review settings from another version…"))
                        .disabled_elevated(blocked)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_target_copy(&update_id, cx);
                            this.focus.focus(window, cx);
                        }))));
            }
            if expanded && let Some(details) = self.pack_details(cx) {
                row = row.child(details);
            }
            list = list.child(row);
        }
        list
    }
}

/// 版をIDへ付ける取得パックも、提供元・拡張・テーマが同じものだけ比較する。
pub(super) fn pack_family(pack: &file_icons_core::ImportedPack) -> &str {
    pack.id
        .rsplit_once('@')
        .filter(|(_, version)| *version == pack.version)
        .map_or(&pack.id, |(family, _)| family)
}
