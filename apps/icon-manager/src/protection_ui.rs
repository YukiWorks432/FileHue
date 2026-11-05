//! 共通の保護設定と、登録を共有する種類の除外理由を表示する。
use super::*;
use file_icons_windows::{ExistingIcon, IconExclusion, IconProtectionRules, ProtectionReason};
use gpui_kit::prelude::FluentBuilder;
#[path = "protection_changes.rs"]
mod changes;
#[path = "protection_recommendations.rs"]
mod recommendations;
#[path = "protection_sources.rs"]
mod sources;
use gpui_kit::component::{
    Sizable,
    checkbox::Checkbox,
    scroll::ScrollableElement,
    select::{Select, SelectEvent, SelectItem, SelectState},
    spinner::Spinner,
};

#[derive(Clone)]
struct PackOption {
    id: String,
    label: SharedString,
}
impl SelectItem for PackOption {
    type Value = String;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &String {
        &self.id
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Protected,
    Allowed,
    Review,
    Current,
}

#[derive(Default)]
pub(super) struct Editor {
    source_by_extension: std::collections::BTreeMap<String, usize>,
    classes: std::collections::BTreeMap<String, Vec<usize>>,
    groups: std::collections::BTreeMap<String, Vec<usize>>,
    group_counts: std::collections::BTreeMap<String, usize>,
    pack_extensions: std::collections::BTreeSet<String>,
    pack_unknown: bool,
    active_extensions: std::collections::BTreeSet<String>,
    active_classes: std::collections::BTreeSet<String>,
    active_excluded_extensions: std::collections::BTreeSet<String>,
    pub open: bool,
    pub draft: bool,
    pub(super) rules: IconProtectionRules,
    pub(super) sources: Vec<ExistingIcon>,
    pub(super) previews: Option<super::original_icons::Previews>,
    pub(super) previews_loading: bool,
    pub(super) pack: Option<String>,
    rows: Vec<IconExclusion>,
    current_rows: Vec<IconExclusion>,
    visible_rows: Vec<usize>,
    filter: Filter,
    pinned: std::collections::BTreeSet<String>,
    retained: std::collections::BTreeSet<String>,
    collapsed: std::collections::BTreeSet<String>,
    planned_count: usize,
    candidate_count: usize,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    list: Option<ListState>,
    text_metrics: Option<(Pixels, String)>,
    details: bool,
    previous_focus: Option<FocusHandle>,
    pub(super) inspection: u64,
    recommendation_notice: Option<String>,
    pub(super) changes_open: bool,
    changes_recommended: bool,
    recommendation_changes: Option<changes::Changes>,
    changes_scroll: ScrollHandle,
    picker: Option<Entity<SelectState<Vec<PackOption>>>>,
    picker_subscription: Option<Subscription>,
}

impl Editor {
    fn rebuild_index(&mut self) {
        self.source_by_extension.clear();
        self.classes.clear();
        self.groups.clear();
        for (index, source) in self.sources.iter().enumerate() {
            self.source_by_extension
                .insert(source.registration.extension.clone(), index);
            let class = self
                .classes
                .entry(source.registration.class.to_ascii_lowercase())
                .or_default();
            class.push(index);
            self.groups
                .entry(source.group_id())
                .or_default()
                .push(index);
        }
    }

    pub(super) fn source(&self, extension: &str) -> Option<&ExistingIcon> {
        self.source_by_extension
            .get(extension)
            .map(|index| &self.sources[*index])
    }

    pub(super) fn class_sources(&self, class: &str) -> impl Iterator<Item = &ExistingIcon> {
        self.classes
            .get(&class.to_ascii_lowercase())
            .into_iter()
            .flat_map(|indices| indices.iter())
            .map(|index| &self.sources[*index])
    }
}

#[cfg(test)]
impl Editor {
    pub(super) fn visual_sources(&mut self, rows: Vec<ExistingIcon>) {
        self.previews = super::original_icons::generate(&rows).ok();
        self.visual_rows(rows.iter().map(|r| r.registration.clone()).collect());
        self.planned_count = rows.len();
        self.candidate_count = rows.len();
        self.sources = rows;
        self.rebuild_index();
    }
    pub(super) fn visual_rows(&mut self, rows: Vec<IconExclusion>) {
        self.rows = rows;
        self.visible_rows = (0..self.rows.len()).collect();
        self.loading = false;
        self.previews_loading = false;
        self.inspection += 1;
        if let Some(list) = &self.list {
            list.reset(self.rows.len());
        }
    }
}

impl Manager {
    #[cfg(test)]
    pub(super) fn performance_protection(&mut self, collapse: bool, cx: &mut Context<Self>) {
        if self.protection.list.is_none() {
            self.protection.visual_rows(
                self.protection
                    .sources
                    .iter()
                    .map(|s| s.registration.clone())
                    .collect(),
            );
            self.protection.open = true;
            self.protection.list =
                Some(ListState::new(0, ListAlignment::Top, px(0.)).measure_all());
        }
        if collapse {
            self.protection
                .collapsed
                .extend(self.protection.sources.iter().map(ExistingIcon::group_id));
            self.reset_protection_list();
            self.protection.collapsed.clear();
        }
        self.refresh_protection_policy(cx);
    }
    pub(super) fn close_target_protection(&mut self) {
        self.targets.invalidate();
        self.protection.inspection += 1;
        self.protection.loading = false;
        self.protection.previews_loading = false;
        self.protection.previews = None;
        self.protection.sources.clear();
        self.protection.rebuild_index();
        self.protection.draft = self.settings.preserve_existing_icons;
        self.protection.rules = self.settings.icon_protection_rules.clone();
    }

    pub(super) fn prepare_target_protection(&mut self, id: &str, cx: &mut Context<Self>) {
        self.protection.open = false;
        self.protection.retained.clear();
        self.protection.pack = Some(id.into());
        self.refresh_protection_context();
        self.protection.draft = self.settings.preserve_existing_icons;
        self.protection.rules = self.settings.icon_protection_rules.clone();
        self.protection.sources.clear();
        self.protection.rebuild_index();
        self.protection.previews = None;
        self.inspect_protection(cx);
    }

    pub(super) fn open_protection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let from_targets = self.targets.pack.is_some();
        self.protection.previous_focus = window.focused(cx);
        self.focus.focus(window, cx);
        self.protection.details = false;
        self.protection.collapsed.clear();
        // 未表示行も測定し、ホイールとスクロールバーが全件の高さを参照できるようにする。
        self.protection.list = Some(ListState::new(0, ListAlignment::Top, px(0.)).measure_all());
        self.protection.open = true;
        self.protection.changes_open = false;
        self.protection.recommendation_changes = None;
        if !from_targets {
            self.protection.draft = self.settings.preserve_existing_icons;
            self.protection.rules = self.settings.icon_protection_rules.clone();
        }
        self.protection.pack = self
            .targets
            .pack
            .clone()
            .or_else(|| self.settings.active.clone())
            .or_else(|| self.selected.clone())
            .or_else(|| self.settings.packs.first().map(|p| p.id.clone()));
        self.refresh_protection_context();
        self.protection.filter = Filter::All;
        self.protection.pinned.clear();
        self.protection.retained.clear();
        let items = self
            .settings
            .packs
            .iter()
            .map(|pack| PackOption {
                id: pack.id.clone(),
                label: pack.label.clone().into(),
            })
            .collect::<Vec<_>>();
        let selected = items
            .iter()
            .position(|item| Some(&item.id) == self.protection.pack.as_ref())
            .map(|row| gpui_kit::component::IndexPath::default().row(row));
        let picker = cx.new(|cx| SelectState::new(items, selected, window, cx));
        self.protection.picker_subscription = Some(cx.subscribe(&picker, |this, _, event, cx| {
            let SelectEvent::Confirm(Some(id)) = event else {
                return;
            };
            this.protection.pack = Some(id.clone());
            this.protection.pinned.clear();
            this.protection.retained.clear();
            this.refresh_protection_context();
            this.inspect_protection(cx);
        }));
        self.protection.picker = Some(picker);

        self.inspect_protection(cx);
    }

    pub(super) fn close_protection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.busy() {
            return;
        }
        self.protection.open = false;
        // 同じ対象画面へ戻る場合は、進行中の調査をその画面で受け取る。
        if self.targets.pack.is_none() || self.targets.pack != self.protection.pack {
            self.protection.inspection += 1;
        }
        if let Some(id) = self.targets.pack.clone() {
            self.prepare_target_protection(&id, cx);
        } else {
            self.protection.previews = None;
        }
        if let Some(focus) = self.protection.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn save_protection(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy()
            || self.protection.loading
            || !self.valid
            || self.protection.error.is_some()
            || !self.protection.dirty(&self.settings)
        {
            return;
        }
        let Some(service) = self._guard.clone() else {
            return;
        };
        let enabled = self.protection.draft;
        let rules = self.protection.rules.clone();
        self.run(
            Kind::SaveProtection,
            move || service.set_icon_protection_rules(enabled, rules, false),
            cx,
        );
    }

    pub(super) fn inspect_protection(&mut self, cx: &mut Context<Self>) {
        self.protection.recommendation_notice = None;
        self.targets.invalidate();
        self.protection.inspection += 1;
        let inspection = self.protection.inspection;
        self.protection.loading = false;
        self.protection.rows.clear();
        self.protection.sources.clear();
        self.protection.rebuild_index();
        self.protection.visible_rows.clear();
        if let Some(list) = &self.protection.list {
            list.reset(0);
        }
        self.protection.error = None;
        let id = self.protection.pack.clone();
        let Some(service) = self._guard.clone() else {
            return;
        };
        let enabled = self.protection.draft;
        let rules = self.protection.rules.clone();
        self.protection.loading = true;
        self.protection.previews = None;
        self.protection.previews_loading = true;
        let executor = cx.background_executor().clone();
        let include_saved = self.protection.open;
        let mut retained: Vec<_> = self.protection.retained.iter().cloned().collect();
        if include_saved {
            retained.extend(
                self.settings
                    .icon_protection_rules
                    .extensions
                    .keys()
                    .cloned(),
            );
            retained.extend(
                self.settings
                    .icon_protection_rules
                    .providers
                    .values()
                    .flat_map(|rules| rules.keys().cloned()),
            );
            retained.sort();
            retained.dedup();
        }
        let task = executor.spawn(async move {
            service.inspect_icon_sources(id.as_deref(), enabled, rules, include_saved, retained)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let sources = this
                .update(cx, |this, cx| {
                    if this.protection.inspection != inspection {
                        return None;
                    }
                    this.protection.loading = false;
                    this.targets.invalidate();
                    let sources = match result {
                        Ok(sources) => {
                            this.accept_protection_sources(sources);
                            Some(this.protection.sources.clone())
                        }
                        Err(error) => {
                            this.protection.previews_loading = false;
                            this.protection.error = Some(format!("{error:#}"));
                            None
                        }
                    };
                    this.reset_protection_list();
                    cx.notify();
                    sources
                })
                .ok()
                .flatten();
            if let Some(sources) = sources {
                let previews = executor
                    .spawn(async move { super::original_icons::generate(&sources).ok() })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.accept_protection_previews(inspection, previews, cx)
                });
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn accept_protection_previews(
        &mut self,
        inspection: u64,
        previews: Option<super::original_icons::Previews>,
        cx: &mut Context<Self>,
    ) {
        if self.protection.inspection != inspection {
            return;
        }
        self.protection.previews_loading = false;
        self.protection.previews = previews;
        self.targets.invalidate();
        if let Some(list) = &self.protection.list {
            list.remeasure();
        }
        cx.notify();
    }
    pub(super) fn accept_protection_sources(&mut self, mut sources: Vec<ExistingIcon>) {
        self.targets.invalidate();
        for source in &mut sources {
            source.update_protection(&self.protection.rules, self.protection.draft);
        }
        sources.sort_by_cached_key(|source| {
            (source.group_id(), source.registration.extension.clone())
        });
        self.protection.rows = sources.iter().map(|r| r.registration.clone()).collect();
        self.protection.sources = sources;
        self.protection.rebuild_index();
        self.refresh_protection_context();
        self.protection.current_rows = self.settings.icon_exclusions.clone();
        let sources = &self.protection.sources;
        let by_extension = &self.protection.source_by_extension;
        self.protection.current_rows.sort_by_cached_key(|r| {
            (
                by_extension
                    .get(&r.extension)
                    .map(|i| sources[*i].group_id()),
                r.extension.clone(),
            )
        });
        let targets = self
            .settings
            .packs
            .iter()
            .find(|p| Some(&p.id) == self.protection.pack.as_ref())
            .map(|p| {
                (
                    p.theme
                        .registration_extensions()
                        .into_iter()
                        .collect::<std::collections::BTreeSet<_>>(),
                    p.theme.needs_unknown_class(),
                )
            });
        self.protection.current_rows.retain(|row| {
            targets.as_ref().is_some_and(|(extensions, unknown)| {
                extensions.contains(&row.extension) || (row.extension.is_empty() && *unknown)
            })
        });
        self.protection.candidate_count = 0;
        self.protection.planned_count = 0;
        for source in &self.protection.sources {
            if targets.as_ref().is_some_and(|(extensions, unknown)| {
                extensions.contains(&source.registration.extension)
                    || (source.registration.extension.is_empty() && *unknown)
            }) {
                self.protection.candidate_count += 1;
                self.protection.planned_count += usize::from(source.protect);
            }
        }
    }

    fn refresh_protection_context(&mut self) {
        let inspected = self
            .settings
            .packs
            .iter()
            .find(|pack| Some(&pack.id) == self.protection.pack.as_ref());
        self.protection.pack_extensions = inspected
            .map(|pack| pack.theme.registration_extensions().into_iter().collect())
            .unwrap_or_default();
        self.protection.pack_unknown =
            inspected.is_some_and(|pack| pack.theme.needs_unknown_class());
        self.protection.active_extensions = self
            .settings
            .active
            .as_ref()
            .and_then(|id| self.settings.packs.iter().find(|pack| &pack.id == id))
            .map(|pack| pack.theme.registration_extensions().into_iter().collect())
            .unwrap_or_default();
        self.protection.active_classes = self
            .settings
            .journal
            .iter()
            .filter_map(|change| {
                let key = change.key.to_lowercase();
                key.strip_prefix("software\\classes\\")
                    .and_then(|key| key.strip_suffix("\\shellex\\iconhandler"))
                    .map(str::to_owned)
            })
            .collect();
        self.protection.active_excluded_extensions = self
            .settings
            .icon_exclusions
            .iter()
            .map(|row| row.extension.to_lowercase())
            .collect();
    }
    pub(super) fn refresh_protection_policy(&mut self, cx: &mut Context<Self>) {
        self.protection.recommendation_notice = None;
        self.targets.invalidate();
        for source in &mut self.protection.sources {
            source.update_protection(&self.protection.rules, self.protection.draft);
        }
        self.protection.rebuild_index();
        if let Some(pack) = self
            .settings
            .packs
            .iter()
            .find(|p| Some(&p.id) == self.protection.pack.as_ref())
        {
            let extensions: std::collections::BTreeSet<_> =
                pack.theme.registration_extensions().into_iter().collect();
            self.protection.planned_count = self
                .protection
                .sources
                .iter()
                .filter(|r| {
                    (extensions.contains(&r.registration.extension)
                        || (r.registration.extension.is_empty()
                            && pack.theme.needs_unknown_class()))
                        && r.protect
                })
                .count();
        }
        let scroll = self
            .protection
            .list
            .as_ref()
            .map(|list| list.logical_scroll_top());
        self.reset_protection_list();
        if let Some(mut scroll) = scroll
            && !self.protection.visible_rows.is_empty()
            && let Some(list) = &self.protection.list
        {
            scroll.item_ix = scroll.item_ix.min(self.protection.visible_rows.len() - 1);
            list.scroll_to(scroll);
        }
        cx.notify();
    }

    fn protection_rows(&self) -> &[IconExclusion] {
        &self.protection.rows
    }

    fn pin_visible_protection_rows(&mut self) {
        self.protection
            .retained
            .extend(self.protection.rows.iter().filter_map(|row| {
                let extension = &row.extension;
                (!self.protection.pack_extensions.contains(extension)
                    && !(extension.is_empty() && self.protection.pack_unknown))
                    .then(|| extension.clone())
            }));
        if self.protection.filter != Filter::All {
            self.protection
                .pinned
                .extend(self.protection.rows.iter().map(|row| row.extension.clone()));
        }
    }

    fn reset_protection_list(&mut self) {
        let current = self.protection.pack.as_ref() == self.settings.active.as_ref()
            && self.settings.active.is_some();
        let excluded: std::collections::BTreeSet<_> = self
            .protection
            .current_rows
            .iter()
            .map(|row| row.extension.clone())
            .collect();
        self.protection.rows = self
            .protection
            .sources
            .iter()
            .filter(|source| {
                self.protection
                    .pinned
                    .contains(&source.registration.extension)
                    || match self.protection.filter {
                        Filter::All => true,
                        Filter::Protected => source.protect,
                        Filter::Allowed => !source.protect,
                        Filter::Review => source.needs_review,
                        Filter::Current => {
                            current && excluded.contains(&source.registration.extension)
                        }
                    }
            })
            .map(|source| source.registration.clone())
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        self.protection.group_counts = self
            .protection_rows()
            .iter()
            .filter_map(|row| self.protection.source(&row.extension))
            .fold(std::collections::BTreeMap::new(), |mut counts, source| {
                *counts.entry(source.group_id()).or_default() += 1;
                counts
            });
        self.protection.visible_rows = self
            .protection_rows()
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| {
                let source = self.protection.source(&row.extension);
                let visible = source.is_none_or(|source| {
                    let group = source.group_id();
                    let first = seen.insert(group.clone());
                    first || !self.protection.collapsed.contains(&group)
                });
                visible.then_some(ix)
            })
            .collect();
        if let Some(list) = &self.protection.list {
            list.reset(self.protection.visible_rows.len());
        }
    }

    pub(super) fn protection_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.protection.changes_open {
            return self.protection_changes_panel(cx);
        }
        let metrics = (window.rem_size(), self.settings.language.clone());
        if self.protection.text_metrics.as_ref() != Some(&metrics) {
            if let Some(list) = &self.protection.list {
                list.remeasure();
            }
            self.protection.text_metrics = Some(metrics);
        }
        use gpui_kit::{assets::IconName, component::button::ButtonGroup};
        let colors = cx.theme().colors;
        let blocked = self.operation.busy() || self.protection.loading || !self.valid;
        let dirty = self.protection.dirty(&self.settings);
        let mut panel = div()
            .id("protection-panel")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_protection(window, cx);
                } else if let Some(list) = &this.protection.list {
                    match event.keystroke.key.as_str() {
                        "home" => list.scroll_to(ListOffset::default()),
                        "end" => list.scroll_to_end(),
                        "pageup" => list.scroll_by(-list.viewport_bounds().size.height),
                        "pagedown" => list.scroll_by(list.viewport_bounds().size.height),
                        _ => return,
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(self.back_button("navigate-back", cx))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.text("既存アイコンの保護", "Existing icon protection")),
                    ),
            );
        let mut settings = theme::card(cx).mx_4().mt_3().mb_2().p_3()
            .flex_shrink_0().flex().flex_col().gap_2()
            .child(div().flex().items_center().gap_2()
                .child(Checkbox::new("protection-toggle")
                    .checked(self.protection.draft).disabled(blocked)
                    .label(self.text("既定では既存アイコンを保護する", "Protect existing icons by default"))
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.pin_visible_protection_rows();
                        this.protection.draft = *checked;
                        this.refresh_protection_policy(cx);
                    })))
                .child(theme::button("protection-help", cx).small().icon(IconName::Info)
                    .accessibility_label(self.text("保護と表示見本について", "About protection and previews"))
                    .tooltip(self.text(
                        "保護設定は全パック共通で、ソフト・種類ごとに変更できます。.lnk・.exeは常に保護します。見本は代表ファイルの設定から算出します。ファイル名やExplorerの更新状況により表示が異なる場合があります。",
                        "Protection applies to all packs and can be changed by software or file type. .lnk and .exe are always protected. Previews use representative filenames and settings; filenames and Explorer refresh can affect the display.",
                    ))));
        let mut packs = div().flex().items_center().gap_2().min_w_0().child(
            div()
                .text_sm()
                .flex_shrink_0()
                .text_color(colors.muted_foreground)
                .child(self.text("確認するパック", "Inspect pack")),
        );
        if let Some(picker) = &self.protection.picker {
            packs = packs.child(
                Select::new(picker)
                    .id("protection-pack")
                    .small()
                    .w_full()
                    .min_w_0()
                    .disabled(blocked || self.settings.packs.is_empty())
                    .accessibility_label(self.text("確認するパック", "Inspect pack"))
                    .placeholder(self.text("パックがありません", "No icon packs")),
            );
        }
        packs = packs.child(
            theme::button("protection-recommended", cx)
                .small()
                .flex_shrink_0()
                .ml_2()
                .label(self.text("おすすめ設定", "Recommended settings"))
                .disabled_elevated(blocked || self.protection.error.is_some())
                .on_click(cx.listener(|this, _, _, cx| this.apply_recommended_protection(cx))),
        );
        packs = packs.child(
            theme::button("protection-recommended-changes", cx)
                .small()
                .label(self.text("変更内容を見る", "View changes"))
                .disabled_elevated(blocked || self.protection.recommendation_changes.is_none())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.protection.changes_recommended = true;
                    this.protection.changes_open = true;
                    cx.notify();
                })),
        );
        settings = settings.child(packs);
        let current = if self.protection.pack.as_ref() == self.settings.active.as_ref() {
            self.protection.current_rows.len()
        } else {
            0
        };
        let summary = if let Some(notice) = &self.protection.recommendation_notice {
            notice.clone()
        } else if self.protection.loading {
            self.text("調査中…", "Checking…").into()
        } else if self.protection.pack.is_none() {
            self.text("パックなし", "No pack").into()
        } else {
            let kept = self.protection.planned_count;
            let replaced = self.protection.candidate_count.saturating_sub(kept);
            if self.settings.language == "en" {
                format!(
                    "This pack: protect {kept} · allow pack {replaced} / Currently excluded: {current}"
                )
            } else {
                format!("このパック: 保護 {kept}・パック許可 {replaced} / 適用中の除外: {current}")
            }
        };
        let summary = format!("{} {summary}", self.text(
            "おすすめ設定は全体保護をオンにし、VS Code・開発用の個別指定をパック優先にします。",
            "Recommended settings enable default protection and prefer pack icons for VS Code and development types, replacing their overrides.",
        ));
        if self.protection.previews_loading {
            settings = settings.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(Spinner::new().small())
                    .child(self.text("アイコンの見本を読み込み中…", "Loading icon previews…")),
            );
        }
        let mut filters = ButtonGroup::new("protection-view")
            .small()
            .disabled(blocked);
        for (id, label, filter) in [
            (
                "protection-planned",
                self.text("すべて", "All"),
                Filter::All,
            ),
            (
                "protection-protected",
                self.text("保護", "Protected"),
                Filter::Protected,
            ),
            (
                "protection-allowed",
                self.text("パック許可", "Allowed"),
                Filter::Allowed,
            ),
            (
                "protection-review",
                self.text("要確認", "Review"),
                Filter::Review,
            ),
            (
                "protection-current",
                self.text("適用中の除外", "Excluded"),
                Filter::Current,
            ),
        ] {
            let selected = self.protection.filter == filter;
            filters = filters.child(
                theme::button(id, cx)
                    .label(label)
                    .selected(selected)
                    .disabled_elevated(blocked)
                    .when(selected, |b| b.icon(IconName::Check))
                    .when(selected, |b| b.font_weight(FontWeight::SEMIBOLD)),
            );
        }
        panel = panel.child(
            settings.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .id("protection-summary")
                    .role(Role::Status)
                    .aria_label(summary.clone())
                    .child(summary),
            ),
        );
        let has_expanded_group = self
            .protection
            .group_counts
            .keys()
            .any(|group| !self.protection.collapsed.contains(group));
        let has_collapsed_group = self
            .protection
            .group_counts
            .keys()
            .any(|group| self.protection.collapsed.contains(group));
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .flex_shrink_0()
                .px_4()
                .pb_2()
                .gap_2()
                .child(
                    filters.on_click(cx.listener(|this, selected: &Vec<usize>, _, cx| {
                        let Some(filter) = selected.first().and_then(|index| {
                            [
                                Filter::All,
                                Filter::Protected,
                                Filter::Allowed,
                                Filter::Review,
                                Filter::Current,
                            ]
                            .get(*index)
                            .copied()
                        }) else {
                            return;
                        };
                        this.protection.filter = filter;
                        this.protection.pinned.clear();
                        this.reset_protection_list();
                        cx.notify();
                    })),
                )
                .child(
                    theme::button("protection-details", cx)
                        .small()
                        .label(self.text("登録の詳細", "Registration details"))
                        .selected(self.protection.details)
                        .toggled(self.protection.details)
                        .when(self.protection.details, |b| b.icon(IconName::Check))
                        .tooltip(self.text(
                            "登録情報の詳細を表示・非表示",
                            "Show or hide registration details",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.protection.details = !this.protection.details;
                            if let Some(list) = &this.protection.list {
                                list.remeasure();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    theme::button("protection-refresh", cx)
                        .ml_2()
                        .small()
                        .label(self.text("再調査", "Refresh"))
                        .disabled_elevated(blocked)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.protection.retained.clear();
                            this.inspect_protection(cx);
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_72()
                        .justify_end()
                        .gap_2()
                        .child(
                            theme::button("protection-expand-all", cx)
                                .small()
                                .label(self.text("すべて開く", "Expand all"))
                                .disabled_elevated(blocked || !has_collapsed_group)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.protection.collapsed.clear();
                                    this.reset_protection_list();
                                    cx.notify();
                                })),
                        )
                        .child(
                            theme::button("protection-collapse-all", cx)
                                .small()
                                .label(self.text("すべて閉じる", "Collapse all"))
                                .disabled_elevated(blocked || !has_expanded_group)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.protection.collapsed.extend(
                                        this.protection.sources.iter().map(ExistingIcon::group_id),
                                    );
                                    this.reset_protection_list();
                                    cx.notify();
                                })),
                        ),
                ),
        );
        if !self.protection.pinned.is_empty() {
            panel = panel.child(
                div()
                    .px_4()
                    .pb_2()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(self.text(
                        "変更中の一覧を保持しています。絞り込みを選び直すと更新します。",
                        "The list stays visible while editing. Select a filter again to update it.",
                    )),
            );
        }
        let shown = self.protection_rows();
        if let Some(error) = &self.protection.error {
            panel = panel.child(
                div()
                    .id("protection-error")
                    .px_4()
                    .py_2()
                    .child(self.text(
                        "調査できませんでした。「再調査」でやり直してください。",
                        "Could not inspect registrations. Choose Refresh to retry.",
                    ))
                    .text_color(cx.theme().colors.danger)
                    .child(error.clone()),
            );
        }
        let mut rows = theme::card(cx)
            .id("protection-rows")
            .mx_4()
            .mb_3()
            .overflow_hidden()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col();
        if shown.is_empty() && !self.protection.loading && self.protection.error.is_none() {
            let message = if self.protection.pack.is_none() {
                self.text(
                    "パックを追加すると、保護する種類を確認できます。",
                    "Add an icon pack to inspect protected file types.",
                )
            } else if self.protection.filter == Filter::Current
                && (self.settings.active.is_none()
                    || self.protection.pack.as_ref() != self.settings.active.as_ref())
            {
                self.text(
                    "このパックは適用されていません。",
                    "This pack is not active.",
                )
            } else if self.protection.filter != Filter::All {
                self.text(
                    "絞り込みに一致する種類はありません。",
                    "No file types match this filter.",
                )
            } else {
                self.text(
                    "既存のアイコン指定はありません。",
                    "No existing icon registrations.",
                )
            };
            let mut empty = div()
                .px_4()
                .py_4()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(message);
            if self.protection.filter != Filter::All {
                empty = empty.child(
                    theme::button("protection-show-all", cx)
                        .small()
                        .label(self.text("すべて表示", "Show all"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.protection.filter = Filter::All;
                            this.protection.pinned.clear();
                            this.reset_protection_list();
                            cx.notify();
                        })),
                );
            }
            rows = rows.child(empty);
        }
        if self.protection.loading {
            rows = rows.child(
                div()
                    .px_4()
                    .py_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Spinner::new().small())
                    .child(self.text("登録を調査中…", "Inspecting registrations…")),
            );
        }
        if !shown.is_empty()
            && let Some(state) = &self.protection.list
        {
            let view = cx.entity().downgrade();
            rows = rows.child(
                list(state.clone(), move |ix, _, cx| {
                    view.update(cx, |this, cx| this.protection_row(ix, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .with_sizing_behavior(ListSizingBehavior::Auto)
                .w_full()
                .flex_1()
                .min_h_0(),
            );
        }
        #[cfg(feature = "test-support")]
        let rows = rows.test_support();
        panel = panel.child(if let Some(state) = &self.protection.list {
            rows.relative().vertical_scrollbar(state)
        } else {
            rows
        });
        panel = panel.child(
            div()
                .flex_shrink_0()
                .border_t_1()
                .border_color(colors.border)
                .bg(colors.sidebar)
                .px_4()
                .py_3()
                .flex()
                .items_center()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .child(
                    div()
                        .id("protection-dirty")
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child(if dirty {
                            self.text("未保存の変更", "Unsaved changes")
                        } else {
                            self.text("保存済み", "Saved")
                        }),
                )
                .child(
                    theme::button("protection-review-changes", cx)
                        .small()
                        .label(self.text("変更内容を見る", "View changes"))
                        .disabled_elevated(blocked || self.protection.error.is_some() || !dirty)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.protection.changes_recommended = false;
                            this.protection.changes_open = true;
                            cx.notify();
                        })),
                )
                .child(
                    theme::button("protection-back", cx)
                        .small()
                        .label(if dirty {
                            self.text("変更を破棄して戻る", "Discard and return")
                        } else {
                            self.text("戻る", "Back")
                        })
                        .disabled_elevated(self.operation.busy())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_protection(window, cx)),
                        ),
                )
                .child(
                    theme::button("protection-save", cx)
                        .small()
                        .label(if self.settings.active.is_some() {
                            self.text("保存して再適用", "Save and reapply")
                        } else {
                            self.text("保存", "Save")
                        })
                        .disabled_elevated(blocked || self.protection.error.is_some() || !dirty)
                        .on_click(cx.listener(|this, _, _, cx| this.save_protection(cx))),
                ),
        );
        #[cfg(feature = "test-support")]
        let panel = panel.test_support();
        panel.into_any_element()
    }
    fn protection_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let ix = self.protection.visible_rows[ix];
        let row = &self.protection_rows()[ix];
        let reason = match row.reason {
            ProtectionReason::ChangedRegistration => self.text(
                "以前のアイコン指定が変わりました",
                "The previous icon registration changed",
            ),
            ProtectionReason::DefaultIcon => self.text(
                "既存のアイコン画像が指定されています",
                "An existing icon image is registered",
            ),
            ProtectionReason::IconHandler => self.text(
                "既存の専用アイコン処理があります",
                "An existing icon handler is registered",
            ),
            ProtectionReason::ProtectedFileType => self.text(
                "常に保護する種類と関連付けを共有しています",
                "Shares an association with an always protected file type",
            ),
        };
        let extension = if row.extension.is_empty() {
            self.text(
                "拡張子なし・名前パターン",
                "Extensionless files and name patterns",
            )
            .into()
        } else {
            format!(".{}", row.extension)
        };
        let label = format!("{extension} — {reason} ({})", row.class);
        let mut row_element = div()
            .id(SharedString::from(format!(
                "protection-row-{}",
                row.extension
            )))
            .min_w_0()
            .flex_shrink_0()
            .bg(colors.group_box)
            .border_color(colors.border)
            .role(Role::Group)
            .aria_label(label)
            .text_sm();
        let group =
            |row: &IconExclusion| self.protection.source(&row.extension).map(|s| s.group_id());
        let first = ix == 0 || group(&self.protection_rows()[ix - 1]) != group(row);
        if first && ix != 0 {
            row_element = row_element.border_t_1();
        }
        if let Some(source) = self.protection.source(&row.extension) {
            row_element = row_element.child(self.protection_source_header(source, ix, cx));
            if self.protection.collapsed.contains(&source.group_id()) {
                #[cfg(feature = "test-support")]
                let row_element = row_element.test_support();
                return row_element.into_any_element();
            }
        }
        let mut content = div()
            .bg(colors.muted)
            .p_3()
            .min_w_0()
            .border_t_1()
            .border_color(colors.border);
        content = content.child(
            div()
                .flex()
                .items_start()
                .gap_3()
                .child(
                    div()
                        .w_32()
                        .flex_shrink_0()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(extension),
                )
                .child(div().flex_1().min_w_0()),
        );
        if let Some(source) = self.protection.source(&row.extension) {
            content = content.child(self.protection_source_controls(source, cx));
        }
        if self.protection.details {
            let mut detail = format!(
                "{}: {}\n{}: {}",
                self.text("クラス", "Class"),
                row.class,
                self.text("登録位置", "Registration"),
                row.source
            );
            if let Some(source) = self.protection.source(&row.extension) {
                detail.push_str(&format!(
                    "\n{}: {}\n{}: {}\n{}: {}",
                    self.text("提供元の識別", "Provider identity"),
                    source.provider_id.as_deref().unwrap_or("?"),
                    self.text("元の指定", "Original reference"),
                    source.reference,
                    self.text("既定で開くアプリ", "Default open app"),
                    source.open_app.as_deref().unwrap_or("?")
                ));
                content = content.child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .truncate()
                        .child(format!(
                            "{}: {}",
                            self.text("元の指定", "Original reference"),
                            source.reference
                        )),
                );
            }
            content = content.child(
                div()
                    .mt_2()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(div().truncate().child(format!(
                                "{}: {}",
                                self.text("クラス", "Class"),
                                row.class
                            )))
                            .child(div().truncate().child(format!(
                                "{}: {}",
                                self.text("登録位置", "Registration"),
                                row.source
                            ))),
                    )
                    .child(
                        theme::button(
                            SharedString::from(format!("protection-copy-{}", row.extension)),
                            cx,
                        )
                        .small()
                        .label(self.text("詳細をコピー", "Copy details"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(detail.clone()));
                        }),
                    ),
            );
        }
        let row_element = row_element.child(content);
        #[cfg(feature = "test-support")]
        let row_element = row_element.test_support();
        row_element.into_any_element()
    }
}
