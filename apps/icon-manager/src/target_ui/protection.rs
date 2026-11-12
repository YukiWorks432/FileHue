//! 対象の選択と、全パック共通の保護方針を分けて実効結果を表示する。
use super::*;
use file_icons_windows::ExistingIcon;

pub(super) struct PreviewClass {
    extension: String,
    class: Option<Result<String, ()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Checking,
    Unavailable,
    Excluded,
    Protected,
    Pack,
    Unsupported,
    Pattern,
}

impl Manager {
    #[cfg(test)]
    pub(crate) fn prepare_target_protection_visual(
        &mut self,
        state: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.targets.category = if state == "filtered" {
            Some("language")
        } else {
            None
        };
        self.targets.filter = if state == "protected" { 5 } else { 0 };
        self.targets.protection_target = matches!(state, "edit" | "allow" | "fallback" | "pending")
            .then(|| "extension:mts".into());
        self.targets.reset_page();
        self.targets.search.update(cx, |input, cx| {
            input.set_value(
                if state == "overview" || state == "protected" {
                    ""
                } else if state == "unsupported" {
                    "not-in-this-pack"
                } else {
                    ".mts"
                },
                window,
                cx,
            )
        });
        self.protection.draft.rules = self.settings.icon_protection_rules.clone();
        if state == "allow" {
            let source = self.protection.inspection.source("mts").unwrap().clone();
            self.protection
                .draft
                .rules
                .set_extension(&source, Some(false));
        }
        if state == "fallback"
            && let Some(previews) = &mut self.protection.inspection.previews
        {
            previews.paths.remove("mts");
        }
        if state == "pending" {
            self.protection.inspection.previews_loading = true;
            self.protection.inspection.previews = None;
        }
        self.refresh_protection_policy(cx);
    }

    pub(super) fn target_source(&self, name: &str) -> Option<&ExistingIcon> {
        let name = name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(name)
            .to_lowercase();
        // 左から調べ、最も長い複合拡張子を先に選ぶ。
        name.match_indices('.')
            .filter(|(offset, _)| offset + 1 < name.len())
            .find_map(|(offset, _)| self.protection.inspection.source(&name[offset + 1..]))
            .or_else(|| {
                std::path::Path::new(&name)
                    .extension()
                    .is_none()
                    .then(|| self.protection.inspection.source(""))
                    .flatten()
            })
    }

    pub(super) fn target_icon_protected(&self, name: &str, saved: bool) -> bool {
        self.target_source(name)
            .is_some_and(|source| self.source_protected(source, saved))
    }

    fn source_protected(&self, source: &ExistingIcon, saved: bool) -> bool {
        if !saved {
            return source.protect;
        }
        let mut original = source.clone();
        original.update_protection(
            &self.settings.icon_protection_rules,
            self.settings.preserve_existing_icons,
        );
        original.protect
    }

    pub(crate) fn inspect_target_preview(&mut self, cx: &mut Context<Self>) {
        if self.targets.pack.is_none() || self.targets.auxiliary != Some(Auxiliary::Details) {
            return;
        }
        let name = self.preview.read(cx).value();
        if name.is_empty() || self.target_source(&name).is_some() {
            self.targets.preview_class = None;
            return;
        }
        let extension = std::path::Path::new(name.as_str())
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        if self
            .targets
            .preview_class
            .as_ref()
            .is_some_and(|preview| preview.extension == extension)
        {
            return;
        }
        self.targets.preview_class = Some(PreviewClass {
            extension: extension.clone(),
            class: None,
        });
        // パターンだけに一致する拡張子は登録調査に含まれない。描画中に問い合わせず、
        // 実効関連付けを確認してからUnknownの保護へフォールバックするかを決める。
        let lookup = extension.clone();
        let task = cx
            .background_executor()
            .spawn(async move { file_icons_windows::association(&lookup).map_err(|_| ()) });
        cx.spawn(async move |this, cx| {
            let class = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(preview) = &mut this.targets.preview_class
                    && preview.extension == extension
                {
                    preview.class = Some(class);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn target_preview_protected(&self, name: &str, saved: bool) -> Option<bool> {
        if let Some(source) = self.target_source(name) {
            return Some(self.source_protected(source, saved));
        }
        let class = self
            .targets
            .preview_class
            .as_ref()?
            .class
            .as_ref()?
            .as_ref()
            .ok()?;
        let pack = self
            .settings
            .packs
            .iter()
            .find(|p| Some(&p.id) == self.targets.pack.as_ref())?;
        let registered = pack.theme.registration_extensions().iter().any(|ext| {
            class
                .strip_prefix('.')
                .is_some_and(|class| class.eq_ignore_ascii_case(ext))
        });
        // 比較はこのパックの適用予定。別パックの退避情報は切り替え時に置換される。
        let class = if class.starts_with('.') && !registered {
            "Unknown"
        } else {
            class
        };
        Some(
            self.protection
                .inspection
                .class_sources(class)
                .any(|source| source.always_protected)
                || (class.eq_ignore_ascii_case("Unknown")
                    && self
                        .protection
                        .inspection
                        .source("")
                        .is_some_and(|source| self.source_protected(source, saved))),
        )
    }

    pub(super) fn target_outcome(
        &self,
        pack: &file_icons_core::ImportedPack,
        target: &Target,
        value: &Selection,
    ) -> Outcome {
        if target.protected() {
            return Outcome::Protected;
        }
        if !value.target_enabled(target) {
            return Outcome::Excluded;
        }
        if target.kind == "pattern" {
            return Outcome::Pattern;
        }
        let sample = if target.kind == "extension" {
            format!("sample.{}", target.name)
        } else {
            target.name.clone()
        };
        if value.resolve(&pack.theme, &sample).is_none() {
            return Outcome::Unsupported;
        }
        if self.protection.inspection.loading {
            return Outcome::Checking;
        }
        if self.protection.inspection.error.is_some() || !self.valid {
            return Outcome::Unavailable;
        }
        if self.target_icon_protected(&sample, false) {
            Outcome::Protected
        } else {
            Outcome::Pack
        }
    }

    pub(super) fn target_outcome_label(&self, outcome: Outcome) -> &'static str {
        match outcome {
            Outcome::Checking => self.text("保護状態を確認中", "Checking protection"),
            Outcome::Unavailable => self.text("保護状態を確認できません", "Protection unavailable"),
            Outcome::Excluded => self.text("対象外・元のアイコン", "Excluded; original icon"),
            Outcome::Protected => self.text("既存アイコンを保護中", "Original icon protected"),
            Outcome::Pack => self.text("パックのアイコンを使用", "Uses pack icon"),
            Outcome::Unsupported => self.text("対応するパック画像なし", "No matching pack icon"),
            Outcome::Pattern => self.text("結果はファイル名で確認", "Check using a filename"),
        }
    }

    pub(super) fn target_protection_editor(
        &self,
        source: &ExistingIcon,
        cx: &mut Context<Self>,
    ) -> Div {
        theme::inset(cx)
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
                if source.registration.extension.is_empty() {
                    self.text(
                        "拡張子のないファイルなどの保護（全パック共通）",
                        "Protection for extensionless files and patterns (all packs)",
                    )
                } else {
                    self.text(
                        "この拡張子の保護（全パック共通）",
                        "Protection for this extension (all packs)",
                    )
                },
            ))
            .child(self.protection_source_controls(source, cx))
    }

    pub(super) fn target_protection_footer(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.protection.draft.dirty(&self.settings) {
            return None;
        }
        let blocked = self.operation.busy()
            || self.protection.inspection.loading
            || self.protection.inspection.error.is_some()
            || !self.valid;
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w_0().text_sm().child(self.text(
                    "未保存の保護設定があります。全パック共通です。対象選択を閉じると破棄します。",
                    "Unsaved protection settings affect all packs. Closing the target editor discards them.",
                )))
                .child(
                    theme::button("target-protection-discard", cx)
                        .small()
                        .label(self.text("保護の変更を破棄", "Discard protection changes"))
                        .disabled_elevated(blocked)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.protection.draft.rules = this.settings.icon_protection_rules.clone();
                            this.protection.draft.enabled = this.settings.preserve_existing_icons;
                            this.refresh_protection_policy(cx);
                        })),
                )
                .child(
                    theme::button("target-protection-save", cx)
                        .small()
                        .label(if self.settings.active.is_some() {
                            self.text("保護を保存して再適用", "Save protection and reapply")
                        } else {
                            self.text("保護設定を保存", "Save protection")
                        })
                        .disabled_elevated(blocked)
                        .on_click(cx.listener(|this, _, _, cx| this.save_protection(cx))),
                ),
        )
    }
}
