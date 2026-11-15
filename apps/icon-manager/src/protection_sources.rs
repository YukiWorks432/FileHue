//! 提供元の一括指定と種類ごとの例外を、保存前の下書きとして扱う。
use super::*;
use gpui_kit::{
    component::{
        WindowExt,
        button::{ButtonVariant, ButtonVariants},
    },
    prelude::FluentBuilder,
};

#[derive(PartialEq, Eq)]
pub(super) enum SampleDisplay {
    Original,
    Pack(std::path::PathBuf),
    ByName,
    Unknown,
}

impl Draft {
    pub(crate) fn dirty(&self, settings: &Settings) -> bool {
        self.enabled != settings.preserve_existing_icons
            || self.rules != settings.icon_protection_rules
    }
}

impl Editor {
    fn provider_override_count(&self, provider: &str) -> usize {
        self.inspection
            .groups
            .get(provider)
            .into_iter()
            .flatten()
            .map(|index| &self.inspection.sources[*index])
            .filter(|row| {
                row.provider_id.as_deref() == Some(provider)
                    && self
                        .draft
                        .rules
                        .extensions
                        .contains_key(&row.registration.extension)
            })
            .count()
    }

    fn set_provider(&mut self, provider: &str, protect: Option<bool>) {
        // 独立表示前に保存された拡張子なしの提供元指定も、別グループの操作では保持する。
        let extensionless = self
            .draft
            .rules
            .providers
            .get(provider)
            .and_then(|rules| rules.get(""))
            .cloned();
        let sources = self
            .inspection
            .sources
            .iter()
            .filter(|row| !row.registration.extension.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        self.draft.rules.set_provider(&sources, provider, protect);
        if let Some(rule) = extensionless {
            self.draft
                .rules
                .providers
                .entry(provider.into())
                .or_default()
                .insert(String::new(), rule);
        }
    }
}

impl Manager {
    /// 拡張子を代表するファイル名で、適用中または保存後の設定を解決する。
    pub(super) fn protection_sample(
        &self,
        source: &ExistingIcon,
        after_save: bool,
    ) -> SampleDisplay {
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
        let Some(sample) = sample_name(pack, extension) else {
            return SampleDisplay::ByName;
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
            if !self
                .protection
                .inspection
                .active_extensions
                .contains(extension)
                && !unknown
            {
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
                    .inspection
                    .active_excluded_extensions
                    .contains(&sample[offset + 1..])
            }) || (self
                .protection
                .inspection
                .active_excluded_extensions
                .contains("")
                && (extension.is_empty()
                    || source.registration.class.eq_ignore_ascii_case("unknown")
                    || (source.registration.class.starts_with('.')
                        && self.settings.fallback(&source.registration.class).is_none())));
            if protected {
                return SampleDisplay::Original;
            }
            let class = source.registration.class.to_lowercase();
            if !self.protection.inspection.active_classes.contains(&class)
                && !(class.starts_with('.')
                    && self
                        .protection
                        .inspection
                        .active_classes
                        .contains("unknown"))
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
        large: bool,
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
                    .inspection.previews
                    .as_ref()
                    .and_then(|p| p.paths.get(extension))
                    .cloned(),
                self.text(
                    "見本を取得できません。専用の処理やファイル固有の画像は表示できない場合があります。",
                    "Preview unavailable. Dynamic handlers and per-file images may not provide a preview.",
                ),
            ),
            SampleDisplay::Pack(path) => (self.text("パックのアイコン", "Pack icon"), Some(path), ""),
            SampleDisplay::ByName => (
                self.text("ファイル名で確認", "Check a filename"),
                None,
                self.text(
                    "名前パターンによって結果が変わります。「適用対象を選ぶ」で具体的なファイル名を確認してください。",
                    "The result depends on the filename pattern. Check a specific filename in Choose targets.",
                ),
            ),
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
            self.text("保存後\nパック未適用", "After saving\nNo active pack")
        } else if after_save
            && self.protection.inspection.pack.as_ref() != self.settings.active.as_ref()
        {
            self.text("保存後\n適用中パック", "After saving\nActive pack")
        } else if after_save {
            self.text("保存後の見込み", "After saving")
        } else {
            self.text("現在の見本", "Current preview")
        };
        let two_line_prefix = self.settings.active.is_none()
            || self.protection.inspection.pack.as_ref() != self.settings.active.as_ref();
        let sample = self.settings.active.as_ref().and_then(|active| {
            self.settings
                .packs
                .iter()
                .find(|pack| &pack.id == active)
                .and_then(|pack| sample_name(pack, extension))
        });
        let visible_name = if extension.is_empty() {
            sample
                .as_ref()
                .map_or_else(|| name.to_owned(), |sample| format!("{sample}\n{name}"))
        } else {
            name.to_owned()
        };
        let label = if large {
            if let Some(sample) = sample {
                format!("{sample}: {name}")
            } else {
                name.to_string()
            }
        } else if let Some(sample) = sample {
            format!("{prefix} ({sample}): {name}")
        } else {
            format!("{prefix}: {name}")
        };
        let has_preview = path.is_some();
        let icon = if let Some(path) = path {
            div()
                .size(if large { px(48.) } else { px(32.) })
                .child(crate::icon_preview::preview(
                    path,
                    if large { px(48.) } else { px(32.) },
                ))
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!(
                    "protection-preview-fallback-{}-{extension}",
                    if after_save { "after" } else { "current" }
                )))
                .size(if large { px(48.) } else { px(32.) })
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(if large { px(48.) } else { px(32.) })
                        .text_color(colors.muted_foreground),
                )
                .tooltip(move |window, cx| {
                    Tooltip::new(missing_tip)
                        .max_w(rems(28.))
                        .whitespace_normal()
                        .build(window, cx)
                })
                .into_any_element()
        };
        let label = if !has_preview {
            format!("{label}\n{missing_tip}")
        } else {
            label
        };
        let icon = div()
            .id(SharedString::from(format!(
                "protection-preview-image-{}-{extension}",
                if after_save { "after" } else { "current" }
            )))
            .child(icon);
        #[cfg(feature = "test-support")]
        let icon = icon.test_support();
        let result = div()
            .id(SharedString::from(format!(
                "protection-result-{}-{extension}",
                if after_save { "after" } else { "current" }
            )))
            .role(Role::Status)
            .aria_label(label.clone())
            .flex()
            .flex_col()
            .when(large, |p| p.items_start())
            .when(!large, |p| p.items_center().w(rems(7.)))
            .gap_1()
            .min_w_0()
            .text_sm()
            .when(!large, |p| {
                p.child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .when(two_line_prefix, |heading| heading.h(rems(2.5)))
                        .child(prefix),
                )
            })
            .child(icon)
            .child(
                div()
                    .min_w_0()
                    .when(!large, |p| p.text_xs())
                    .child(if large { label } else { visible_name }),
            );
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
        let blocked = self.operation.busy()
            || self.protection.inspection.loading
            || self.protection.inspection.error.is_some();
        let mut area = div().flex().flex_col().gap_2().min_w_0();
        let previous = ix
            .checked_sub(1)
            .and_then(|i| self.protection_rows().get(i))
            .and_then(|r| self.protection.inspection.source(&r.extension));
        if previous.is_none_or(|p| display_group(p) != display_group(source)) {
            let group_id = display_group(source);
            let expanded = !self.protection.presentation.collapsed.contains(&group_id);
            let count = self
                .protection
                .presentation
                .group_counts
                .get(&group_id)
                .copied()
                .unwrap_or(0);
            let group = self.protection.inspection.groups.get(&group_id);
            let total = group.map_or(0, Vec::len);
            let protected = group
                .into_iter()
                .flatten()
                .filter(|index| self.protection.inspection.sources[**index].protect)
                .count();
            let mut heading = div().flex().flex_wrap().items_center().gap_2().min_w_0();
            if let Some(path) = self
                .protection
                .inspection
                .previews
                .as_ref()
                .and_then(|p| p.provider_paths.get(&group_id))
            {
                let icon = div()
                    .id(SharedString::from(format!("provider-icon-{group_id}")))
                    .role(Role::Image)
                    .aria_label(self.text("ソフトのアイコン", "Software icon"))
                    .child(crate::icon_preview::preview(path.clone(), rems(2.)));
                #[cfg(feature = "test-support")]
                let icon = icon.test_support();
                heading = heading.child(icon);
            }
            let extensionless = source.registration.extension.is_empty();
            let name = if extensionless {
                self.text(
                    "拡張子のないファイルなど",
                    "Extensionless files and patterns",
                )
                .into()
            } else {
                source
                    .provider_name
                    .clone()
                    .unwrap_or_else(|| self.text("提供元不明", "Unknown provider").into())
            };
            let provider_setting = source
                .provider_id
                .as_ref()
                .and_then(|provider| self.protection.draft.rules.providers.get(provider));
            let default_label = match provider_setting {
                Some(rules)
                    if group.into_iter().flatten().any(|index| {
                        let row = &self.protection.inspection.sources[*index];
                        rules
                            .get(&row.registration.extension)
                            .is_some_and(|rule| rule.fingerprint != row.fingerprint)
                    }) =>
                {
                    self.text("要確認", "Needs review")
                }
                Some(rules) if rules.values().all(|rule| rule.protect) => {
                    self.text("元のアイコン", "Original icon")
                }
                Some(rules) if rules.values().all(|rule| !rule.protect) => {
                    self.text("パックを許可", "Allow pack")
                }
                Some(_) => self.text("個別指定", "Overrides"),
                None => self.text("全体に従う", "Global"),
            };
            let overrides = source.provider_id.as_ref().map_or(0, |provider| {
                self.protection.provider_override_count(provider)
            });
            let mut setting_summary = div()
                .flex()
                .gap_1()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!(
                    "{}: {default_label}",
                    self.text("基本設定", "Default")
                ));
            if overrides > 0
                && let Some(provider) = &source.provider_id
            {
                let label = format!("{}: {overrides}", self.text("個別設定", "Overrides"));
                let notice = div()
                    .id(SharedString::from(format!("provider-overrides-{provider}")))
                    .role(Role::Status)
                    .aria_label(label.clone())
                    .child(format!("· {label}"));
                #[cfg(feature = "test-support")]
                let notice = notice.test_support();
                setting_summary = setting_summary.child(notice);
            }
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
                        .when(!extensionless, |name| name.truncate())
                        .child(name.clone())
                        .when(!extensionless, |name| name.child(setting_summary)),
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
                    if this.operation.busy() || this.protection.inspection.loading {
                        return;
                    }
                    if !this.protection.presentation.collapsed.remove(&group_id) {
                        this.protection
                            .presentation
                            .collapsed
                            .insert(group_id.clone());
                    }
                    let scroll = this
                        .protection
                        .presentation
                        .list
                        .as_ref()
                        .map(|list| list.logical_scroll_top());
                    let anchor = scroll.and_then(|offset| {
                        this.protection
                            .presentation
                            .visible_rows
                            .get(offset.item_ix)
                            .copied()
                            .map(|ix| (ix, offset.offset_in_item))
                    });
                    this.reset_protection_list();
                    if let Some((anchor, offset)) = anchor
                        && let Some(ix) = this
                            .protection
                            .presentation
                            .visible_rows
                            .iter()
                            .position(|ix| *ix == anchor)
                        && let Some(list) = &this.protection.presentation.list
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
                && !source.registration.extension.is_empty()
                && expanded
            {
                let actions = div().bg(colors.muted).p_3().flex().flex_wrap().items_center().gap_2()
                    .child(div().min_w_40().flex_1().flex().flex_col().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD)
                            .child(self.text("このソフトの基本設定", "Software defaults")))
                        .child(div().text_xs().text_color(colors.muted_foreground)
                            .child(self.text("個別指定した種類は、その指定を優先します。", "File type overrides take priority."))))
                    .child(div().flex().flex_wrap().gap_1()
                    .child(self.protection_choice(source, "provider-original",
                        (self.text("元のアイコン", "Original icon"),
                        self.text("このソフトのアイコンをまとめて保護します。種類ごとの個別指定は優先されます。", "Protect this software's icons. File type overrides take priority.")), cx))
                    .child(self.protection_choice(source, "provider-pack",
                        (self.text("パックを許可", "Allow pack"),
                        self.text("このソフトにパックのアイコンをまとめて許可します。種類ごとの個別指定と常時保護は優先されます。", "Allow pack icons for this software. File type overrides and mandatory protection take priority.")), cx))
                    .child(self.protection_choice(source, "provider-default",
                        (self.text("全体に従う", "Global default"),
                        self.text("基本設定を共通の保護設定へ戻します。個別指定は保持します。", "Use the global protection setting. File type overrides are kept.")), cx)));
                let overrides = self.protection.provider_override_count(provider);
                let mut tools = div().flex().flex_wrap().items_center().gap_3();
                let reset_source = source.clone();
                tools = tools.child(
                    theme::button(SharedString::from(format!("provider-reset-{provider}")), cx)
                        .small()
                        .icon(gpui_kit::assets::IconName::RotateCcw)
                        .accessibility_label(
                            self.text("このソフトの設定を初期化", "Reset software settings"),
                        )
                        .tooltip(self.text("このソフトの設定を初期化…", "Reset software settings…"))
                        .disabled_elevated(
                            blocked
                                || (overrides == 0
                                    && !self
                                        .protection
                                        .draft
                                        .rules
                                        .providers
                                        .get(provider)
                                        .is_some_and(|rules| {
                                            rules.keys().any(|extension| !extension.is_empty())
                                        })),
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
        use gpui_kit::component::tag::Tag;
        let colors = cx.theme().colors;
        let extension = source.registration.extension.clone();
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
        let changed = saved_source.protect != source.protect
            || saved_rule != self.protection.draft.rules.extensions.get(&extension);
        let results = div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.protection_result(source, false, false, cx))
            .child(
                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::ArrowRight)
                    .size_4()
                    .text_color(colors.muted_foreground),
            )
            .child(self.protection_result(source, true, false, cx));
        let mut area = div().flex().flex_col().gap_2().min_w_0();
        if !self
            .protection
            .inspection
            .pack_extensions
            .contains(&extension)
            && !(extension.is_empty() && self.protection.inspection.pack_unknown)
        {
            let text = if self.protection.inspection.pack.is_none() {
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
        let choices = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .child(self.protection_choice(
                source,
                "original",
                (
                    self.text("元のアイコン", "Original icon"),
                    self.text(
                        "元のアイコンを維持します（保護）",
                        "Keep the original icon (protected)",
                    ),
                ),
                cx,
            ))
            .child(self.protection_choice(
                source,
                "pack",
                (
                    self.text("パックを許可", "Allow pack"),
                    self.text(
                        "パックのアイコンへの変更を許可します",
                        "Allow the icon pack to replace this icon",
                    ),
                ),
                cx,
            ))
            .child(self.protection_choice(
                source,
                "default",
                (
                    if extension.is_empty() {
                        self.text("全体に従う", "Global default")
                    } else {
                        self.text("基本に従う", "App default")
                    },
                    self.text(
                        "個別設定を解除して基本設定に従います。",
                        "Remove the file type override and follow the default.",
                    ),
                ),
                cx,
            ));
        let extension_label = if extension.is_empty() {
            self.text("拡張子なし", "No extension").to_owned()
        } else {
            format!(".{extension}")
        };
        area = area.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .when(self.screen == Screen::Protection, |area| {
                    area.child(
                        div()
                            .min_w_32()
                            .flex_1()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(extension_label),
                            )
                            .when(source.needs_review, |row| {
                                row.child(
                                    Tag::warning()
                                        .small()
                                        .child(self.text("要確認", "Needs review")),
                                )
                            })
                            .when(source.always_protected, |row| {
                                row.child(
                                    Tag::secondary()
                                        .small()
                                        .child(self.text("常に保護", "Always protected")),
                                )
                            })
                            .when(changed, |row| {
                                row.child(
                                    Tag::info().small().child(self.text("変更あり", "Changed")),
                                )
                            }),
                    )
                })
                .child(results)
                .child(choices),
        );
        if self.protection.presentation.details || self.screen == Screen::Targets {
            area = area.child(saved);
        }
        if source.needs_review {
            area = area.child(div().text_sm().text_color(colors.warning_foreground).child(
                self.text(
                    "登録が変わりました。再指定するまで保護します。",
                    "Registration changed. Protected until reviewed.",
                ),
            ));
        }
        if source.always_protected {
            area = area.child(div().text_sm().text_color(colors.muted_foreground).child(
                self.text(
                    "常に保護する種類と関連付けを共有しているため、元のアイコンを維持します。",
                    "This type shares a protected association, so its original icon is kept.",
                ),
            ));
        }
        area
    }
    fn protection_choice(
        &self,
        source: &ExistingIcon,
        kind: &str,
        copy: (&'static str, &'static str),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::assets::IconName;
        let (title, outcome) = copy;
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
                .and_then(|id| self.protection.draft.rules.providers.get(id))
                .filter(|rules| rules.keys().any(|extension| !extension.is_empty()));
            match choice {
                None => assigned.is_none(),
                Some(protect) => self
                    .protection
                    .inspection
                    .groups
                    .get(&display_group(source))
                    .into_iter()
                    .flatten()
                    .map(|index| &self.protection.inspection.sources[*index])
                    .all(|r| {
                        assigned
                            .and_then(|rules| rules.get(&r.registration.extension))
                            .is_some_and(|rule| {
                                rule.fingerprint == r.fingerprint && rule.protect == protect
                            })
                    }),
            }
        } else {
            let rule = self
                .protection
                .draft
                .rules
                .extensions
                .get(extension)
                .or_else(|| {
                    extension
                        .is_empty()
                        .then(|| {
                            source
                                .provider_id
                                .as_ref()
                                .and_then(|provider| {
                                    self.protection.draft.rules.providers.get(provider)
                                })
                                .and_then(|rules| rules.get(extension))
                        })
                        .flatten()
                });
            match rule {
                Some(rule) if rule.fingerprint != source.fingerprint => false,
                Some(rule) => choice == Some(rule.protect),
                None => choice.is_none(),
            }
        };
        let blocked = self.operation.busy()
            || self.protection.inspection.loading
            || self.protection.inspection.error.is_some()
            || (!provider_scope && source.always_protected);
        let tip = outcome;
        let row = source.clone();
        let id = if provider_scope {
            format!("provider-{kind}-{}", display_group(source))
        } else {
            format!("{kind}-icon-{extension}")
        };
        let preview = theme::button(SharedString::from(id), cx)
            .small()
            .w(rems(10.))
            .min_w(rems(10.))
            .toggled(selected)
            .selected(selected)
            .disabled_elevated(blocked)
            .accessibility_label(format!("{title}: {tip}"))
            .label(title)
            .when(selected, |button| button.icon(IconName::Check))
            .tooltip(tip)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.operation.busy()
                    || this.protection.inspection.loading
                    || (!provider_scope && row.always_protected)
                {
                    return;
                }
                this.pin_visible_protection_rows();
                if provider_scope {
                    if let Some(provider) = &row.provider_id {
                        this.protection.set_provider(provider, choice);
                    }
                } else {
                    this.protection.draft.rules.set_extension(&row, choice);
                    if row.registration.extension.is_empty() && choice.is_none() {
                        for rules in this.protection.draft.rules.providers.values_mut() {
                            rules.remove("");
                        }
                        this.protection
                            .draft
                            .rules
                            .providers
                            .retain(|_, rules| !rules.is_empty());
                    }
                }
                this.refresh_protection_policy(cx);
            }));
        preview.into_any_element()
    }

    fn reset_provider_protection(&mut self, provider: &str, cx: &mut Context<Self>) {
        self.pin_visible_protection_rows();
        self.protection.set_provider(provider, None);
        for row in &self.protection.inspection.sources {
            if row.provider_id.as_deref() == Some(provider)
                && !row.registration.extension.is_empty()
            {
                self.protection.draft.rules.set_extension(row, None);
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
                .ok_variant(ButtonVariant::Primary)
                .on_ok(move |_, _, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if !this.operation.busy()
                            && !this.protection.inspection.loading
                            && this.protection.inspection.error.is_none()
                        {
                            this.reset_provider_protection(&provider, cx);
                        }
                    });
                    true
                })
        });
    }
}

fn sample_name(pack: &file_icons_core::ImportedPack, extension: &str) -> Option<String> {
    if !extension.is_empty() {
        return Some(format!("sample.{extension}"));
    }
    let rules = pack.theme.rules();
    rules
        .files
        .keys()
        .chain(rules.language_files.keys())
        .find(|name| {
            std::path::Path::new(name.as_str()).extension().is_none()
                && pack.theme.resolve(name).is_some()
        })
        .cloned()
}
