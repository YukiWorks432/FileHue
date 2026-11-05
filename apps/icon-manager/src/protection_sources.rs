//! 提供元の一括指定と種類ごとの例外を、保存前の下書きとして扱う。
use super::*;
use gpui_kit::{
    component::{
        WindowExt,
        button::{ButtonVariant, ButtonVariants},
    },
    prelude::FluentBuilder,
};

enum SampleDisplay {
    Original,
    Pack(std::path::PathBuf),
    Unknown,
}

impl Editor {
    pub(crate) fn dirty(&self, settings: &Settings) -> bool {
        self.draft != settings.preserve_existing_icons
            || self.rules != settings.icon_protection_rules
    }

    fn provider_override_count(&self, provider: &str) -> usize {
        self.groups
            .get(provider)
            .into_iter()
            .flatten()
            .map(|index| &self.sources[*index])
            .filter(|row| {
                row.provider_id.as_deref() == Some(provider)
                    && self
                        .rules
                        .extensions
                        .contains_key(&row.registration.extension)
            })
            .count()
    }
}

impl Manager {
    /// 拡張子を代表するファイル名で、適用中または保存後の設定を解決する。
    fn protection_sample(&self, source: &ExistingIcon, after_save: bool) -> SampleDisplay {
        if self.conflict_attention || !self.valid {
            return SampleDisplay::Unknown;
        }
        let Some(active) = &self.settings.active else {
            return SampleDisplay::Original;
        };
        let Some(pack) = self.settings.packs.iter().find(|pack| &pack.id == active) else {
            return SampleDisplay::Unknown;
        };
        let extension = &source.registration.extension;
        let sample = if extension.is_empty() {
            "sample".to_owned()
        } else {
            format!("sample.{extension}")
        };
        let default_selection = file_icons_core::selection::Selection::default();
        let selection = if after_save {
            self.settings
                .selections
                .get(active)
                .unwrap_or(&default_selection)
        } else {
            &self.settings.active_selection
        };
        let Some((path, _)) = selection.resolve(&pack.theme, &sample) else {
            return SampleDisplay::Original;
        };
        if source.always_protected {
            return SampleDisplay::Original;
        }
        if after_save {
            let unknown = extension.is_empty() && pack.theme.needs_unknown_class();
            if !self.protection.active_extensions.contains(extension) && !unknown {
                // パック外の種類が共有クラス経由の名前パターンに一致するかは、
                // この一覧の調査だけでは保存後の登録計画を確定できない。
                return SampleDisplay::Unknown;
            }
            if source.protect {
                return SampleDisplay::Original;
            }
        } else {
            let protected = sample.match_indices('.').any(|(offset, _)| {
                self.protection
                    .active_excluded_extensions
                    .contains(&sample[offset + 1..])
            }) || (self.protection.active_excluded_extensions.contains("")
                && (extension.is_empty()
                    || source.registration.class.eq_ignore_ascii_case("unknown")
                    || (source.registration.class.starts_with('.')
                        && self.settings.fallback(&source.registration.class).is_none())));
            if protected {
                return SampleDisplay::Original;
            }
            let class = source.registration.class.to_lowercase();
            if !self.protection.active_classes.contains(&class)
                && !(class.starts_with('.') && self.protection.active_classes.contains("unknown"))
            {
                return SampleDisplay::Original;
            }
        }
        SampleDisplay::Pack(path.to_path_buf())
    }

    pub(super) fn protection_result(
        &self,
        source: &ExistingIcon,
        after_save: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::{
            assets::IconName,
            component::{Icon, tooltip::Tooltip},
        };
        let colors = cx.theme().colors;
        let extension = &source.registration.extension;
        let (name, path, missing_tip) = match self.protection_sample(source, after_save) {
            SampleDisplay::Original => (
                self.text("元の表示", "Original appearance"),
                self.protection
                    .previews
                    .as_ref()
                    .and_then(|p| p.paths.get(extension))
                    .cloned(),
                self.text(
                    "見本を取得できません。専用の処理やファイル固有の画像は表示できない場合があります。",
                    "Preview unavailable. Dynamic handlers and per-file images may not provide a preview.",
                ),
            ),
            SampleDisplay::Pack(path) => (self.text("パックのアイコン", "Pack icon"), Some(path), ""),
            SampleDisplay::Unknown => (
                self.text("確認できません", "Unavailable"),
                None,
                self.text(
                    "登録や設定を確認できないため、見本を確定できません。",
                    "The preview cannot be determined from the available registration and settings.",
                ),
            ),
        };
        let prefix = if after_save && self.settings.active.is_none() {
            self.text("保存後（パック未適用）", "After saving (no active pack)")
        } else if after_save && self.protection.pack.as_ref() != self.settings.active.as_ref() {
            self.text("保存後（適用中パック）", "After saving (active pack)")
        } else if after_save {
            self.text("保存後の見込み", "After saving")
        } else {
            self.text("現在の見本", "Current preview")
        };
        let sample = if extension.is_empty() {
            "sample".into()
        } else {
            format!("sample.{extension}")
        };
        let label = format!("{prefix} ({sample}): {name}");
        let icon = if let Some(path) = path {
            div().size_5().child(img(path).size_5()).into_any_element()
        } else {
            div()
                .id(SharedString::from(format!(
                    "protection-preview-fallback-{}-{extension}",
                    if after_save { "after" } else { "current" }
                )))
                .size_5()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_5()
                        .text_color(colors.muted_foreground),
                )
                .tooltip(move |window, cx| Tooltip::new(missing_tip).build(window, cx))
                .into_any_element()
        };
        let result = div()
            .id(SharedString::from(format!(
                "protection-result-{}-{extension}",
                if after_save { "after" } else { "current" }
            )))
            .role(Role::Status)
            .aria_label(label.clone())
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .text_sm()
            .child(icon)
            .child(div().min_w_0().child(label));
        #[cfg(feature = "test-support")]
        let result = result.test_support();
        result.into_any_element()
    }

    pub(super) fn protection_source_header(
        &self,
        source: &ExistingIcon,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = cx.theme().colors;
        let blocked =
            self.operation.busy() || self.protection.loading || self.protection.error.is_some();
        let mut area = div().flex().flex_col().gap_2().min_w_0();
        let previous = ix
            .checked_sub(1)
            .and_then(|i| self.protection_rows().get(i))
            .and_then(|r| self.protection.source(&r.extension));
        if previous.is_none_or(|p| p.group_id() != source.group_id()) {
            let group_id = source.group_id();
            let expanded = !self.protection.collapsed.contains(&group_id);
            let count = self
                .protection
                .group_counts
                .get(&group_id)
                .copied()
                .unwrap_or(0);
            let group = self.protection.groups.get(&group_id);
            let total = group.map_or(0, Vec::len);
            let protected = group
                .into_iter()
                .flatten()
                .filter(|index| self.protection.sources[**index].protect)
                .count();
            let mut heading = div().flex().flex_wrap().items_center().gap_2().min_w_0();
            if let Some(path) = self
                .protection
                .previews
                .as_ref()
                .and_then(|p| p.provider_paths.get(&group_id))
            {
                let icon = div()
                    .id(SharedString::from(format!("provider-icon-{group_id}")))
                    .role(Role::Image)
                    .aria_label(self.text("ソフトのアイコン", "Software icon"))
                    .child(img(path.clone()).size_8());
                #[cfg(feature = "test-support")]
                let icon = icon.test_support();
                heading = heading.child(icon);
            }
            let name = source
                .provider_name
                .clone()
                .unwrap_or_else(|| self.text("提供元不明", "Unknown provider").into());
            use gpui_kit::{assets::IconName, component::Icon};
            heading = heading
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size_4(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_left()
                        .truncate()
                        .child(name.clone()),
                );
            let count_label = div()
                .id(SharedString::from(format!("provider-count-{group_id}")))
                .flex_shrink_0()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(if self.settings.language == "en" {
                    format!(
                        "Types: {count}/{total} · protected {protected} · allowed {}",
                        total - protected
                    )
                } else {
                    format!(
                        "種類: {count}/{total} · 保護 {protected} · パック許可 {}",
                        total - protected
                    )
                });
            #[cfg(feature = "test-support")]
            let count_label = count_label.test_support();
            heading = heading.child(count_label);
            let trigger = Button::new(SharedString::from(format!("provider-toggle-{group_id}")))
                .ghost()
                .w_full()
                .h_auto()
                .min_h_12()
                .p_2()
                .tooltip(name.clone())
                .accessibility_label(format!(
                    "{}: {name} ({count}/{total})",
                    self.text(
                        if expanded {
                            "折りたたむ"
                        } else {
                            "展開する"
                        },
                        if expanded { "Collapse" } else { "Expand" }
                    )
                ))
                .toggled(expanded)
                .disabled(blocked)
                .child(heading.w_full())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.operation.busy() || this.protection.loading {
                        return;
                    }
                    if !this.protection.collapsed.remove(&group_id) {
                        this.protection.collapsed.insert(group_id.clone());
                    }
                    let scroll = this
                        .protection
                        .list
                        .as_ref()
                        .map(|list| list.logical_scroll_top());
                    let anchor = scroll.and_then(|offset| {
                        this.protection
                            .visible_rows
                            .get(offset.item_ix)
                            .copied()
                            .map(|ix| (ix, offset.offset_in_item))
                    });
                    this.reset_protection_list();
                    if let Some((anchor, offset)) = anchor
                        && let Some(ix) = this
                            .protection
                            .visible_rows
                            .iter()
                            .position(|ix| *ix == anchor)
                        && let Some(list) = &this.protection.list
                    {
                        list.scroll_to(ListOffset {
                            item_ix: ix,
                            offset_in_item: offset,
                        });
                    }
                    cx.notify();
                }));
            let mut group = div().flex().flex_col().child(div().p_2().child(trigger));
            if let Some(provider) = &source.provider_id
                && expanded
            {
                let original = self
                    .protection
                    .previews
                    .as_ref()
                    .and_then(|p| p.provider_paths.get(&source.group_id()))
                    .cloned();
                let pack = self
                    .settings
                    .packs
                    .iter()
                    .find(|p| Some(&p.id) == self.protection.pack.as_ref())
                    .and_then(|p| {
                        p.theme
                            .resolve(&format!("sample.{}", source.registration.extension))
                    })
                    .map(|p| p.to_path_buf());
                let inherited = if self.protection.draft {
                    original.clone()
                } else {
                    pack.clone().or_else(|| original.clone())
                };
                let actions = div().bg(colors.muted).p_3().flex().flex_col().gap_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD)
                        .child(self.text("このソフトの基本設定", "Software defaults")))
                    .child(div().flex().flex_wrap().gap_2()
                    .child(self.protection_preview(source, "provider-original", original,
                        (self.text("元のアイコン", "Original icon"),
                        self.text("このソフトのアイコンをまとめて保護します。種類ごとの個別指定は優先されます。", "Protect this software's icons. File type overrides take priority."),
                        self.text("ソフトの代表アイコンを取得できません。", "Software preview unavailable.")), cx))
                    .child(self.protection_preview(source, "provider-pack", pack,
                        (self.text("パックのアイコン", "Pack icon"),
                        self.text("このソフトにパックのアイコンをまとめて許可します。種類ごとの個別指定と常時保護は優先されます。", "Allow pack icons for this software. File type overrides and mandatory protection take priority."),
                        self.text("代表画像です。各種類にはそれぞれ対応するパックのアイコンを使用します。", "Each file type uses its own corresponding pack icon.")), cx))
                    .child(self.protection_preview(source, "provider-default", inherited,
                        (self.text("全体設定に従う", "Use global setting"),
                        self.text("基本設定を共通の保護設定へ戻します。個別指定は保持します。画像は代表例です。", "Use the global protection setting. File type overrides are kept. The image is representative."),
                        self.text("継承結果の代表画像を取得できません。", "Inherited representative preview unavailable.")), cx)));
                let overrides = self.protection.provider_override_count(provider);
                let mut tools = div().flex().flex_wrap().items_center().gap_3();
                if overrides > 0 {
                    let label = format!(
                        "{}: {overrides}",
                        self.text("個別設定", "File type overrides")
                    );
                    let notice = div()
                        .id(SharedString::from(format!("provider-overrides-{provider}")))
                        .role(Role::Status)
                        .aria_label(label.clone())
                        .text_sm()
                        .child(label);
                    #[cfg(feature = "test-support")]
                    let notice = notice.test_support();
                    tools = tools.child(notice);
                }
                let reset_source = source.clone();
                tools = tools.child(
                    theme::button(SharedString::from(format!("provider-reset-{provider}")), cx)
                        .small()
                        .self_start()
                        .label(self.text("このソフトの設定を初期化…", "Reset software settings…"))
                        .disabled_elevated(
                            blocked
                                || (overrides == 0
                                    && !self.protection.rules.providers.contains_key(provider)),
                        )
                        .on_click(cx.listener(move |this, event, window, cx| {
                            this.confirm_reset_provider(&reset_source, event, window, cx);
                        })),
                );
                group = group.child(actions.child(tools));
            }
            area = area.child(group);
        }
        area
    }

    pub(crate) fn protection_source_controls(
        &self,
        source: &ExistingIcon,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = cx.theme().colors;
        let extension = source.registration.extension.clone();
        let original = self
            .protection
            .previews
            .as_ref()
            .and_then(|p| p.paths.get(&extension))
            .cloned();
        let pack = self
            .settings
            .packs
            .iter()
            .find(|p| Some(&p.id) == self.protection.pack.as_ref())
            .and_then(|p| p.theme.resolve(&format!("sample.{extension}")))
            .map(|p| p.to_path_buf());
        let inherited = self
            .protection
            .rules
            .inherited_protection(source, self.protection.draft);
        let inherited_path = if inherited {
            original.clone()
        } else {
            pack.clone().or_else(|| original.clone())
        };
        let inherits_original = inherited || pack.is_none();
        let saved_rule = self
            .settings
            .icon_protection_rules
            .extensions
            .get(&extension);
        let mut saved_source = source.clone();
        saved_source.update_protection(
            &self.settings.icon_protection_rules,
            self.settings.preserve_existing_icons,
        );
        let previous_provider_rule = if saved_rule.is_none() && saved_source.needs_review {
            self.settings
                .icon_protection_rules
                .providers
                .values()
                .find_map(|rules| rules.get(&extension))
        } else {
            None
        };
        let provider_rule = source
            .provider_id
            .as_ref()
            .and_then(|id| self.settings.icon_protection_rules.providers.get(id))
            .and_then(|rules| rules.get(&extension));
        let saved_choice = match (saved_rule, previous_provider_rule, provider_rule) {
            (Some(rule), _, _) if rule.protect => {
                self.text("個別・元のアイコン", "File type: original icon")
            }
            (Some(_), _, _) => self.text("個別・パックを許可", "File type: allow pack"),
            (_, Some(rule), _) if rule.protect => self.text(
                "以前のソフト指定・元のアイコン",
                "Previous software choice: original icon",
            ),
            (_, Some(_), _) => self.text(
                "以前のソフト指定・パックを許可",
                "Previous software choice: allow pack",
            ),
            (_, _, Some(rule)) if rule.protect => {
                self.text("基本設定・元のアイコン", "Software setting: original icon")
            }
            (_, _, Some(_)) => self.text("基本設定・パックを許可", "Software setting: allow pack"),
            _ if self.settings.preserve_existing_icons => self.text(
                "基本設定・全体の保護に従う",
                "Software setting: use global protection",
            ),
            _ => self.text(
                "基本設定・全体のパック許可に従う",
                "Software setting: use global pack permission",
            ),
        };
        let saved_label = format!(
            "{}: {}{}",
            self.text("保存済みの指定", "Saved choice"),
            saved_choice,
            if saved_source.needs_review {
                self.text("（要確認）", " (review needed)")
            } else {
                ""
            }
        );
        let saved = div()
            .id(SharedString::from(format!("protection-saved-{extension}")))
            .role(Role::Status)
            .aria_label(saved_label.clone())
            .text_sm()
            .text_color(colors.muted_foreground)
            .child(saved_label);
        #[cfg(feature = "test-support")]
        let saved = saved.test_support();
        let results = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .child(saved)
            .child(self.protection_result(source, false, cx))
            .child(self.protection_result(source, true, cx));
        let mut area = div()
            .mt_2()
            .flex()
            .flex_col()
            .gap_3()
            .min_w_0()
            .child(results);
        if !self.protection.pack_extensions.contains(&extension)
            && !(extension.is_empty() && self.protection.pack_unknown)
        {
            let text = if self.protection.pack.is_none() {
                self.text(
                    "パックがありません。設定は全パック共通です。",
                    "No pack is available. This setting applies to all packs.",
                )
            } else {
                self.text(
                    "このパックの対象外です。設定は全パック共通です。",
                    "Not covered by this pack. This setting applies to all packs.",
                )
            };
            let notice = div()
                .id(SharedString::from(format!(
                    "protection-outside-{extension}"
                )))
                .role(Role::Status)
                .aria_label(text)
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(text);
            #[cfg(feature = "test-support")]
            let notice = notice.test_support();
            area = area.child(notice);
        }
        area = area.child(
            div().flex().flex_wrap().gap_2()
                .child(self.protection_preview(source, "original", original,
                    (self.text("元のアイコン", "Original icon"),
                    self.text("元のアイコンを維持します（保護）", "Keep the original icon (protected)"),
                    self.text("元アイコンの見本を取得できません。専用の処理やファイル固有の画像は表示できない場合があります。", "Original preview unavailable. Dynamic handlers and per-file images may not provide a preview.")), cx))
                .child(self.protection_preview(source, "pack", pack,
                    (self.text("パックのアイコン", "Pack icon"),
                    self.text("パックのアイコンへの変更を許可します", "Allow the icon pack to replace this icon"),
                    self.text("この種類に対応するパックの見本がありません。未対応のファイルには元の表示を使用します。", "No representative pack icon is available. Unsupported files keep their original appearance.")), cx))
                .child(self.protection_preview(source, "default", inherited_path,
                    (self.text("基本設定に従う", "Use software setting"),
                    if inherits_original { self.text("個別設定を解除して基本設定に従います。結果は元のアイコンです。", "Remove the file type override. The inherited result keeps the original icon.") }
                    else { self.text("個別設定を解除して基本設定に従います。結果はパックのアイコンです。", "Remove the file type override. The inherited result uses the pack icon.") },
                    self.text("継承結果の見本を取得できません。", "Inherited preview unavailable.")), cx))
        );
        if source.needs_review {
            area = area.child(div().text_sm().text_color(colors.warning).child(self.text(
                "登録が変わりました。再指定するまで保護します。",
                "Registration changed. Protected until reviewed.",
            )));
        }
        area
    }
    fn protection_preview(
        &self,
        source: &ExistingIcon,
        kind: &str,
        path: Option<std::path::PathBuf>,
        copy: (&'static str, &'static str, &'static str),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::assets::IconName;
        use gpui_kit::component::Icon;
        let colors = cx.theme().colors;
        let (title, outcome, fallback) = copy;
        let provider_scope = kind.starts_with("provider-");
        let kind = kind.strip_prefix("provider-").unwrap_or(kind);
        let extension = &source.registration.extension;
        let choice = match kind {
            "original" => Some(true),
            "pack" => Some(false),
            _ => None,
        };
        let selected = if provider_scope {
            let assigned = source
                .provider_id
                .as_ref()
                .and_then(|id| self.protection.rules.providers.get(id));
            match choice {
                None => assigned.is_none(),
                Some(protect) => self
                    .protection
                    .groups
                    .get(&source.group_id())
                    .into_iter()
                    .flatten()
                    .map(|index| &self.protection.sources[*index])
                    .all(|r| {
                        assigned
                            .and_then(|rules| rules.get(&r.registration.extension))
                            .is_some_and(|rule| {
                                rule.fingerprint == r.fingerprint && rule.protect == protect
                            })
                    }),
            }
        } else {
            self.protection
                .rules
                .extensions
                .get(extension)
                .filter(|rule| rule.fingerprint == source.fingerprint)
                .map(|rule| rule.protect)
                == choice
        };
        let blocked = self.operation.busy()
            || self.protection.loading
            || self.protection.error.is_some()
            || (!provider_scope && source.always_protected);
        let description = outcome;
        let tip = if path.is_none() && self.protection.previews_loading {
            format!(
                "{description}\n{}",
                self.text("アイコンの見本を読み込み中…", "Loading icon previews…")
            )
        } else if path.is_some() {
            description.to_owned()
        } else {
            format!("{description}\n{fallback}")
        };
        let mut image = div().size_8().opacity(if selected { 1. } else { 0.35 });
        if let Some(path) = path {
            image = image.child(img(path).size_8());
        } else if self.protection.previews_loading {
            image = image.child(super::Spinner::new().small());
        } else {
            image = image.child(
                Icon::new(if provider_scope && kind == "pack" {
                    IconName::Package
                } else {
                    IconName::TriangleAlert
                })
                .size_8()
                .text_color(colors.muted_foreground),
            );
        }
        let row = source.clone();
        let id = if provider_scope {
            format!("provider-{kind}-{}", source.group_id())
        } else {
            format!("{kind}-icon-{extension}")
        };
        let preview = theme::button(SharedString::from(id), cx)
            .h_auto()
            .toggled(selected)
            .selected(selected)
            .disabled_elevated(blocked)
            .accessibility_label(format!("{title}: {tip}"))
            .w_40()
            .min_w_40()
            .flex_shrink_0()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                colors.primary
            } else {
                colors.border
            })
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h_8()
                            .child(image)
                            .child(div().size_4().when(selected, |v| {
                                v.child(
                                    Icon::new(IconName::Check)
                                        .size_4()
                                        .text_color(colors.primary),
                                )
                            })),
                    ),
            )
            .tooltip(tip)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.operation.busy()
                    || this.protection.loading
                    || (!provider_scope && row.always_protected)
                {
                    return;
                }
                this.pin_visible_protection_rows();
                if provider_scope {
                    if let Some(provider) = &row.provider_id {
                        this.protection.rules.set_provider(
                            &this.protection.sources,
                            provider,
                            choice,
                        );
                    }
                } else {
                    this.protection.rules.set_extension(&row, choice);
                }
                this.refresh_protection_policy(cx);
            }));
        preview.into_any_element()
    }

    fn reset_provider_protection(&mut self, provider: &str, cx: &mut Context<Self>) {
        self.pin_visible_protection_rows();
        self.protection
            .rules
            .set_provider(&self.protection.sources, provider, None);
        for row in &self.protection.sources {
            if row.provider_id.as_deref() == Some(provider) {
                self.protection.rules.set_extension(row, None);
            }
        }
        self.refresh_protection_policy(cx);
    }

    fn confirm_reset_provider(
        &mut self,
        source: &ExistingIcon,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = &source.provider_id else {
            return;
        };
        let overrides = self.protection.provider_override_count(provider);
        // マウスでは部品がフォーカスを移さないため、有効な管理画面の戻り先を用意する。
        // キーボードで開いた場合はトリガーのフォーカスを維持する。
        if matches!(event, ClickEvent::Mouse(_)) {
            self.focus.focus(window, cx);
        }
        let name = source
            .provider_name
            .as_deref()
            .unwrap_or(self.text("提供元不明", "Unknown provider"));
        let title = if self.settings.language == "en" {
            format!("Reset all settings for “{name}”?")
        } else {
            format!("「{name}」の設定を初期化しますか？")
        };
        let description = if self.settings.language == "en" {
            format!(
                "Clear this software's rule and {overrides} inspected file type overrides. Other software's rules are kept. This changes the draft; save to apply it."
            )
        } else {
            format!(
                "このソフトの基本設定と、確認済みの{overrides}件の個別設定を解除し、全体設定に戻します。ほかのソフトの設定は保持します。変更は下書きに反映し、保存すると適用されます。"
            )
        };
        let cancel = self.text("キャンセル", "Cancel");
        let reset = self.text("設定を初期化", "Reset settings");
        let provider = provider.clone();
        let owner = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let owner = owner.clone();
            let provider = provider.clone();
            dialog
                .entrance(gpui_kit::component::dialog::DialogEntrance::None)
                .bg(cx.theme().colors.popover)
                .title(title.clone())
                .description(description.clone())
                .confirm()
                .cancel_text(cancel)
                .ok_text(reset)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if !this.operation.busy()
                            && !this.protection.loading
                            && this.protection.error.is_none()
                        {
                            this.reset_provider_protection(&provider, cx);
                        }
                    });
                    true
                })
        });
    }
}
