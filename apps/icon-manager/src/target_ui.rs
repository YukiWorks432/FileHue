//! 対象の編集は下書きに保持し、明示操作で共通管理処理へ保存する。
use super::*;
use file_icons_core::selection::{CATEGORIES, Selection, Target, TargetQuery};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{assets::IconName, component::Sizable};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
mod auxiliary;
mod categories;
mod draft;
mod list;
mod protection;
use draft::Draft;
struct Results {
    query: String,
    rows: Arc<[usize]>,
    action_ids: Arc<[String]>,
    counts: BTreeMap<&'static str, CategoryCounts>,
    dirty: usize,
}
#[derive(Default)]
struct CategoryCounts {
    enabled: usize,
    total: usize,
    selectable: usize,
}
#[derive(PartialEq)]
struct ListMetrics {
    width: Pixels,
    rem: Pixels,
    language: String,
    protection_target: Option<String>,
    selecting: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Auxiliary {
    Categories,
    Details,
    Copy,
}
pub(super) struct Editor {
    // 対象列挙の寿命を、現在開いているパックと再読み込みに揃える。
    all: Vec<Target>,
    auxiliary: Option<Auxiliary>,
    copy_changes: Option<String>,
    selecting: bool,
    list: ListState,
    list_width: Option<Pixels>,
    results: Option<Results>,
    text_metrics: Option<ListMetrics>,
    categories: Entity<gpui_kit::component::list::ListState<categories::CategoryList>>,
    detail_scroll: ScrollHandle,
    pub(super) pack: Option<String>,
    drafts: BTreeMap<String, Draft>,
    search: Entity<InputState>,
    _subscription: Subscription,
    category: Option<&'static str>,
    filter: u8,
    page: usize,
    marked: BTreeSet<String>,
    focused: Option<Target>,
    protection_target: Option<String>,
    preview_class: Option<protection::PreviewClass>,
    categories_visible: bool,
}
impl Editor {
    pub(super) fn has_changes(&self, settings: &Settings) -> bool {
        self.pack
            .as_ref()
            .and_then(|id| self.drafts.get(id).map(|draft| (id, draft)))
            .is_some_and(|(id, draft)| {
                draft
                    .value
                    .changes_from(settings.selections.get(id).unwrap_or(&Selection::default()))
                    > 0
            })
    }
    pub(super) fn is_main(&self) -> bool {
        self.auxiliary.is_none()
    }
    pub(super) fn discard(&mut self, settings: &Settings) {
        if let Some(id) = &self.pack
            && let Some(draft) = self.drafts.get_mut(id)
        {
            draft.reset(settings.selections.get(id).cloned().unwrap_or_default());
        }
        self.invalidate();
    }
    pub(super) fn invalidate(&mut self) {
        self.results = None;
        self.list.remeasure();
    }
    #[cfg(test)]
    pub(super) fn performance_seed(&mut self, id: String, theme: &file_icons_core::CompiledTheme) {
        self.all = theme.targets();
        let mut value = Selection::default();
        for index in 0..6000 {
            value
                .set(&format!("extension:perf-{index:04}"), Some(true))
                .unwrap();
        }
        self.drafts.insert(id.clone(), Draft::new(value));
        self.pack = Some(id);
    }

    #[cfg(test)]
    pub(super) fn performance_category(&mut self, category: Option<&'static str>) {
        self.category = category;
        self.reset_page();
    }
    #[cfg(test)]
    pub(super) fn performance_edit_32(&mut self) {
        for index in 0..32 {
            self.edit(&[format!("extension:perf-{index:04}")], Some(false));
        }
    }
    #[cfg(test)]
    pub(super) fn scroll_offset(&self) -> Point<Pixels> {
        self.list.scroll_px_offset_for_scrollbar()
    }
    /// 再読み込み後の定義に照合し、未保存の指定と履歴の寿命をパックに揃える。
    pub(super) fn reconcile(&mut self, settings: &Settings) {
        self.results = None;
        self.list.remeasure();
        self.drafts.retain(|id, draft| {
            let Some(pack) = settings.packs.iter().find(|pack| pack.id == *id) else {
                return false;
            };
            draft.reconcile(&pack.theme);
            true
        });
        if let Some(id) = &self.pack {
            if let Some(pack) = settings.packs.iter().find(|pack| &pack.id == id) {
                let targets = pack.theme.targets();
                self.marked
                    .retain(|id| targets.iter().any(|target| &target.id == id));
                self.focused = self.focused.as_ref().and_then(|focused| {
                    targets
                        .iter()
                        .find(|target| target.id == focused.id)
                        .cloned()
                });
                self.all = targets;
            } else {
                self.pack = None;
                self.all.clear();
                self.marked.clear();
                self.focused = None;
                self.auxiliary = None;
                self.selecting = false;
                self.reset_page();
            }
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Manager>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("json, package.json…"));
        let subscription = cx.subscribe(&search, |this, _, _: &InputEvent, cx| {
            this.targets.reset_page();
            cx.notify();
        });
        Self {
            all: Vec::new(),
            auxiliary: None,
            copy_changes: None,
            selecting: false,
            list: ListState::new(0, ListAlignment::Top, px(0.)),
            list_width: None,
            results: None,
            text_metrics: None,
            categories: categories::create(window, cx),
            detail_scroll: ScrollHandle::new(),
            pack: None,
            drafts: BTreeMap::new(),
            search,
            _subscription: subscription,
            category: None,
            filter: 0,
            page: 0,
            marked: BTreeSet::new(),
            focused: None,
            protection_target: None,
            preview_class: None,
            categories_visible: true,
        }
    }
    fn reset_page(&mut self) {
        self.page = 0;
        self.results = None;
        self.list.reset(0);
    }
    fn edit(&mut self, ids: &[String], value: Option<bool>) {
        self.invalidate();
        let Some(id) = &self.pack else { return };
        let Some(draft) = self.drafts.get_mut(id) else {
            return;
        };
        draft.edit(ids, value);
    }
}
fn matching_categories(query: &str) -> Vec<&'static str> {
    CATEGORIES
        .iter()
        .filter(|(_, ja, en)| {
            ja.to_lowercase().contains(query) || en.to_lowercase().contains(query)
        })
        .map(|(key, _, _)| *key)
        .collect()
}
fn matches_query(target: &Target, query: &str, search: &TargetQuery, categories: &[&str]) -> bool {
    search.matches(target) || target.kind.contains(query) || categories.contains(&target.category)
}
impl Manager {
    pub(super) fn open_target_copy(&mut self, id: &str, cx: &mut Context<Self>) {
        self.open_targets(id, cx);
        self.targets.auxiliary = Some(Auxiliary::Copy);
        self.targets.copy_changes = None;
    }

    #[cfg(test)]
    pub(crate) fn prepare_auxiliary_visual(&mut self, copy: bool) {
        let pack = &self.settings.packs[0];
        self.targets.all = pack.theme.targets();
        self.targets
            .drafts
            .entry(pack.id.clone())
            .or_insert_with(|| {
                Draft::new(
                    self.settings
                        .selections
                        .get(&pack.id)
                        .cloned()
                        .unwrap_or_default(),
                )
            });
        self.targets.pack = copy.then(|| pack.id.clone());
        self.screen = if copy { Screen::Targets } else { Screen::Packs };
        self.targets.auxiliary = copy.then_some(Auxiliary::Copy);
        self.selected = (!copy).then(|| pack.id.clone());
    }
    #[cfg(test)]
    pub(crate) fn prepare_guidance_visual(&mut self, page: &str) {
        self.prepare_auxiliary_visual(page == "update" || page == "planned");
        self.protection.draft.enabled = self.settings.preserve_existing_icons;
        self.protection.draft.rules = self.settings.icon_protection_rules.clone();
        self.targets.copy_changes = (page == "update").then(|| self.settings.packs[1].id.clone());
        if page == "planned" {
            self.targets.auxiliary = Some(Auxiliary::Details);
        }
        if !matches!(page, "samples" | "update" | "planned") {
            self.targets.pack = None;
            self.screen = Screen::Packs;
            self.selected = None;
        }
    }

    pub(super) fn back_from_targets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.busy() {
            return;
        }
        if self.targets.is_main() && self.targets.has_changes(&self.settings) {
            self.confirm_navigation(window, cx, |this, window, cx| {
                this.back_from_targets(window, cx)
            });
            return;
        }
        self.operation.clear_result();
        if self.targets.auxiliary.take().is_none() {
            self.targets.pack = None;
            self.screen = Screen::Packs;
            self.targets.all.clear();
            self.targets.preview_class = None;
            self.close_target_protection();
        }
        self.focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn open_targets(&mut self, id: &str, cx: &mut Context<Self>) {
        self.operation.clear_result();
        self.targets.all = self
            .settings
            .packs
            .iter()
            .find(|pack| pack.id == id)
            .map(|pack| pack.theme.targets())
            .unwrap_or_default();
        self.targets.drafts.entry(id.into()).or_insert_with(|| {
            Draft::new(
                self.settings
                    .selections
                    .get(id)
                    .cloned()
                    .unwrap_or_default(),
            )
        });
        self.targets.auxiliary = None;
        self.targets.pack = Some(id.into());
        self.screen = Screen::Targets;
        self.targets.reset_page();
        self.targets.selecting = false;
        self.targets.marked.clear();
        self.targets.focused = None;
        self.targets.protection_target = None;
        self.targets.preview_class = None;
        self.prepare_target_protection(id, cx);
        cx.notify();
    }
    pub(super) fn target_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        #[cfg(test)]
        let _panel = crate::performance::section("target_panel");
        let id = self.targets.pack.clone()?;
        let query = self.targets.search.read(cx).value().to_lowercase();
        // 入力イベントの通知が処理される前に描画されても、最新の入力へ追随する。
        if self
            .targets
            .results
            .as_ref()
            .is_some_and(|r| r.query != query)
        {
            self.targets.reset_page();
        }
        let pack = self.settings.packs.iter().find(|p| p.id == id)?;
        let empty = Selection::default();
        let saved = self.settings.selections.get(&id).unwrap_or(&empty);
        let value = &self.targets.drafts.get(&id)?.value;
        let all = &self.targets.all;
        #[cfg(test)]
        let filtering = crate::performance::section("target_filter");
        if self.targets.results.is_none() {
            let search = TargetQuery::new(&query);
            let categories = matching_categories(&query);
            let rows: Arc<[usize]> = all
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    self.targets
                        .category
                        .is_none_or(|category| t.category == category)
                        && matches_query(t, &query, &search, &categories)
                        && match self.targets.filter {
                            1 => value.target_enabled(t),
                            2 => !value.target_enabled(t),
                            3 => value.explicit(&t.id).is_some(),
                            4 => value.explicit(&t.id) != saved.explicit(&t.id),
                            5 => {
                                self.target_outcome(pack, t, value)
                                    == protection::Outcome::Protected
                            }
                            _ => true,
                        }
                })
                .map(|(index, _)| index)
                .collect();
            let action_ids = rows
                .iter()
                .map(|i| &all[*i])
                .filter(|t| !t.protected())
                .map(|t| t.id.clone())
                .collect();
            let mut counts: BTreeMap<_, CategoryCounts> = BTreeMap::new();
            for target in all {
                let entry = counts.entry(target.category).or_default();
                entry.enabled += usize::from(value.target_enabled(target));
                entry.total += 1;
                entry.selectable += usize::from(!target.protected());
            }
            let mut scroll = self.targets.list.logical_scroll_top();
            self.targets.page = self.targets.page.min(rows.len().saturating_sub(1) / 100);
            let count = rows.len().saturating_sub(self.targets.page * 100).min(100);
            // 未測定の画面外の行もスクロール範囲へ含める。高さは拘束せず、
            // 実際の折り返し・展開後の寸法はListの測定結果で置き換える。
            self.targets
                .list
                .reset_with_uniform_height(count, window.rem_size() * 4.);
            scroll.item_ix = scroll.item_ix.min(count.saturating_sub(1));
            self.targets.list.scroll_to(scroll);
            self.targets.results = Some(Results {
                query,
                rows,
                action_ids,
                counts,
                dirty: value.changes_from(saved),
            });
        }
        #[cfg(test)]
        drop(filtering);
        let results = self.targets.results.as_ref()?;
        let rows: Vec<_> = results.rows.iter().map(|i| &all[*i]).collect();
        let dirty = results.dirty;
        let metrics = ListMetrics {
            width: window.viewport_size().width,
            rem: window.rem_size(),
            language: self.settings.language.clone(),
            protection_target: self.targets.protection_target.clone(),
            selecting: self.targets.selecting,
        };
        if self.targets.text_metrics.as_ref() != Some(&metrics) {
            self.targets.list.remeasure();
            self.targets.text_metrics = Some(metrics);
        }
        // 基準文字とともに必要幅を広げ、補助欄が一覧を押し潰さないようにする。
        let compact = window.viewport_size().width < window.rem_size() * 64.;
        let header = self.target_header(&pack.label, compact, cx);
        let mut body = div().flex_1().min_h_0().min_w_0().flex().p_3().gap_3();
        if self.targets.auxiliary == Some(Auxiliary::Categories)
            || (!compact && self.targets.categories_visible && self.targets.auxiliary.is_none())
        {
            body = body.child(self.target_categories(all, compact, window, cx));
        }
        if self.targets.auxiliary.is_none() {
            body = body.child(self.target_results(pack, all, &rows, cx));
        }
        if self.targets.auxiliary == Some(Auxiliary::Details) {
            body = body.child(self.target_details(pack, saved, value, cx));
        }
        if self.targets.auxiliary == Some(Auxiliary::Copy) {
            body = body.child(self.target_copy(pack, cx));
        }
        let footer = self.target_footer(id, dirty, cx);
        Some(
            div()
                .id("target-editor")
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.back_from_targets(window, cx);
                    }
                }))
                .child(body)
                .child(footer)
                .into_any_element(),
        )
    }
    fn target_header(&self, label: &str, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let full_title = format!("{} · {label}", self.text("適用対象", "Targets"));
        let mut tools = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .child(
                div()
                    .id("target-title")
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .truncate()
                    .aria_label(full_title.clone())
                    .child(full_title.clone())
                    .tooltip(move |window, cx| {
                        Tooltip::new(full_title.clone())
                            .max_w(rems(28.))
                            .whitespace_normal()
                            .build(window, cx)
                    }),
            );
        {
            tools = tools
                .child(
                    theme::button("protection-open", cx)
                        .small()
                        .icon(IconName::ShieldCheck)
                        .accessibility_label(self.text("アイコンの保護", "Icon protection"))
                        .tooltip(self.text("アイコンの保護", "Icon protection"))
                        .disabled_elevated(self.operation.busy() || !self.valid)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_protection(window, cx)),
                        ),
                )
                .child(
                    theme::button("conflicts-open", cx)
                        .small()
                        .icon(IconName::TriangleAlert)
                        .when(self.conflict_attention, |b| b.warning())
                        .accessibility_label(if self.conflict_attention {
                            self.text("競合あり・確認", "Conflicts detected · Review")
                        } else {
                            self.text("競合を確認", "Review conflicts")
                        })
                        .tooltip(if self.conflict_attention {
                            self.text("競合あり・確認", "Conflicts detected · Review")
                        } else {
                            self.text("競合を確認", "Review conflicts")
                        })
                        .disabled_elevated(self.operation.busy() || self._guard.is_none())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_conflicts(
                                file_icons_windows::conflict::Strategy::PreserveExternal,
                                window,
                                cx,
                            )
                        })),
                );
        }
        for (id, ja, en, section) in [
            (
                "target-categories-open",
                "カテゴリ",
                "Categories",
                Auxiliary::Categories,
            ),
            (
                "target-details-open",
                "表示を確認…",
                "Check icons…",
                Auxiliary::Details,
            ),
            (
                "target-copy-open",
                "設定をコピー…",
                "Copy settings…",
                Auxiliary::Copy,
            ),
        ] {
            tools = tools.child(
                theme::button(id, cx)
                    .small()
                    .label(self.text(ja, en))
                    .when(section == Auxiliary::Categories, |button| {
                        button.icon(IconName::PanelLeft)
                    })
                    .selected(if section == Auxiliary::Categories && !compact {
                        self.targets.categories_visible
                    } else {
                        self.targets.auxiliary == Some(section)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.operation.clear_result();
                        if section == Auxiliary::Categories && !compact {
                            this.targets.categories_visible = !this.targets.categories_visible;
                            this.targets.list.remeasure();
                            cx.notify();
                            return;
                        }
                        this.targets.auxiliary = if this.targets.auxiliary == Some(section) {
                            None
                        } else {
                            Some(section)
                        };
                        if this.targets.auxiliary == Some(Auxiliary::Categories) {
                            this.targets.categories.focus_handle(cx).focus(window, cx);
                        } else {
                            this.focus.focus(window, cx);
                        }
                        if section == Auxiliary::Details {
                            this.inspect_target_preview(cx);
                        }
                        cx.notify();
                    })),
            );
        }
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(cx.theme().colors.sidebar)
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(self.header(cx))
            .child(self.toolbar(compact, cx))
            .child(tools)
            .into_any_element()
    }
    fn target_results(
        &self,
        pack: &file_icons_core::ImportedPack,
        all: &[Target],
        rows: &[&Target],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut panel = theme::card(cx)
            .id("target-body")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col();
        panel = panel.child(self.target_tools(rows, cx));
        if self.protection.inspection.loading {
            panel = panel.child(
                div()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Spinner::new().small())
                    .child(self.text("保護状態を確認中…", "Checking icon protection…")),
            );
        } else if let Some(error) = &self.protection.inspection.error {
            panel = panel.child(
                div()
                    .p_3()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .text_sm()
                            .child(format!(
                                "{}: {error}",
                                self.text(
                                    "保護状態を取得できません",
                                    "Could not inspect protection"
                                )
                            )),
                    )
                    .child(
                        theme::button("target-protection-retry", cx)
                            .small()
                            .label(self.text("再試行", "Retry"))
                            .on_click(cx.listener(|this, _, _, cx| this.inspect_protection(cx))),
                    ),
            );
        }
        let start = (self.targets.page * 100).min(rows.len().saturating_sub(1) / 100 * 100);
        let rows_count = rows.len();
        panel = panel.child(self.target_list(pack, all, rows, cx));
        panel = panel.child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .px_3()
                .py_1()
                .text_sm()
                .border_t_1()
                .border_color(cx.theme().colors.border)
                .child(
                    theme::button("target-previous", cx)
                        .small()
                        .label(self.text("前へ", "Previous"))
                        .disabled_elevated(start == 0)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.targets.page = this.targets.page.saturating_sub(1);
                            this.targets.list.reset_with_uniform_height(
                                100.min(rows_count.saturating_sub(this.targets.page * 100)),
                                window.rem_size() * 4.,
                            );
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("target-page-range")
                        .role(Role::Status)
                        .aria_label(format!(
                            "{}–{} / {}",
                            if rows.is_empty() { 0 } else { start + 1 },
                            (start + 100).min(rows.len()),
                            rows.len()
                        ))
                        .child(format!(
                            "{}–{} / {}",
                            if rows.is_empty() { 0 } else { start + 1 },
                            (start + 100).min(rows.len()),
                            rows.len()
                        )),
                )
                .child(
                    theme::button("target-next", cx)
                        .small()
                        .label(self.text("次へ", "Next"))
                        .disabled_elevated(start + 100 >= rows.len())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.targets.page += 1;
                            this.targets.list.reset_with_uniform_height(
                                100.min(rows_count.saturating_sub(this.targets.page * 100)),
                                window.rem_size() * 4.,
                            );
                            cx.notify();
                        })),
                ),
        );
        #[cfg(feature = "test-support")]
        let panel = panel.test_support();
        panel.into_any_element()
    }
    fn target_tools(&self, rows: &[&Target], cx: &mut Context<Self>) -> AnyElement {
        let blocked = self.operation.busy() || !self.valid;
        #[cfg(test)]
        let _tools = crate::performance::section("target_tools");
        let category_name = self
            .targets
            .category
            .and_then(|key| CATEGORIES.iter().find(|(id, _, _)| *id == key))
            .map(|(_, ja, en)| self.text(ja, en))
            .unwrap_or(self.text("すべての対象", "All targets"));
        let mut heading = div().flex().flex_wrap().items_center().gap_3().child(
            div()
                .flex_1()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(category_name),
        );
        if let Some(category) = self.targets.category {
            let (enabled, total) = self
                .targets
                .results
                .as_ref()
                .unwrap()
                .counts
                .get(category)
                .map(|counts| (counts.enabled, counts.selectable))
                .unwrap_or_default();
            let enable = enabled != total;
            heading = heading.child(
                theme::checkbox(SharedString::from(format!(
                    "target-category-toggle-{category}"
                )))
                .small()
                .checked(!enable && total > 0)
                .disabled(blocked || total == 0)
                .label(format!(
                    "{} ({}/{})",
                    self.text("カテゴリ全体を対象に含める", "Include category"),
                    enabled,
                    total
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let ids: Vec<_> = this
                        .targets
                        .all
                        .iter()
                        .filter(|t| t.category == category && !t.protected())
                        .map(|t| t.id.clone())
                        .collect();
                    this.targets.edit(&ids, Some(enable));
                    cx.notify();
                })),
            );
        }
        let filters = [
            self.text("すべて", "All"),
            self.text("対象に含む", "Included"),
            self.text("対象外", "Excluded"),
            self.text("個別設定あり", "Overrides"),
            self.text("変更あり", "Changed"),
            self.text("保護中", "Protected"),
        ];
        let selected = self.targets.filter as usize;
        let owner = cx.entity().downgrade();
        let filter = theme::button("target-filter-open", cx)
            .small()
            .icon(IconName::ListFilter)
            .label(if selected == 0 {
                self.text("絞り込み", "Filter")
            } else {
                filters[selected]
            })
            .accessibility_label(self.text("対象の表示を絞り込む", "Filter target list"))
            .selected(selected != 0)
            .dropdown_menu(move |mut menu, _, _| {
                for (index, label) in filters.into_iter().enumerate() {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(index == selected)
                            .on_click(move |_, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.targets.filter = index as u8;
                                    this.targets.reset_page();
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            });
        let mut tools = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(heading)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(self.text(
                        "下部の適用ボタンを押すまで、実際のアイコンは変わりません。",
                        "Actual icons stay unchanged until you use the apply button below.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .child(self.text("名前・拡張子", "Name or extension")),
                    )
                    .child(
                        Input::new(&self.targets.search)
                            .id("target-search")
                            .small()
                            .aria_label(
                                self.text("名前・拡張子を検索", "Search names and extensions"),
                            )
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(filter),
            );
        let marked: Vec<_> = self.targets.marked.iter().cloned().collect();
        let ids: Arc<[String]> = if self.targets.selecting {
            marked.clone().into()
        } else {
            self.targets.results.as_ref().unwrap().action_ids.clone()
        };
        let scope = if self.targets.selecting {
            format!(
                "{} {}",
                marked.len(),
                self.text(
                    "行を選択（非表示を含む）",
                    "selected (including hidden rows)"
                )
            )
        } else {
            format!(
                "{} {}",
                rows.len(),
                self.text("件の検索結果", "matching targets")
            )
        };
        let mut actions = div().flex().flex_wrap().items_center().gap_2().child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(scope),
        );
        for (suffix, ja, en, choice) in [
            ("enable", "対象に含める", "Include", Some(true)),
            ("disable", "対象から外す", "Exclude", Some(false)),
            ("reset", "個別指定を解除", "Reset overrides", None),
        ] {
            let ids = ids.clone();
            let control = format!(
                "target-{suffix}-{}",
                if self.targets.selecting {
                    "marked"
                } else {
                    "results"
                }
            );
            actions = actions.child(
                theme::button(SharedString::from(control), cx)
                    .small()
                    .label(self.text(ja, en))
                    .disabled_elevated(blocked || ids.is_empty())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.targets.edit(&ids, choice);
                        cx.notify();
                    })),
            );
        }
        let mut selection_tools = div().flex().items_center().gap_2().child(
            theme::button("target-selection-mode", cx)
                .small()
                .label(if self.targets.selecting {
                    self.text("複数選択を終了", "Done selecting")
                } else {
                    self.text("複数選択", "Select multiple")
                })
                .selected(self.targets.selecting)
                .toggled(self.targets.selecting)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.targets.selecting = !this.targets.selecting;
                    cx.notify();
                })),
        );
        if self.targets.selecting {
            selection_tools = selection_tools.child(
                theme::button("target-clear-marked", cx)
                    .small()
                    .label(self.text("行選択を解除", "Clear selection"))
                    .disabled_elevated(marked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.targets.marked.clear();
                        cx.notify();
                    })),
            );
        }
        tools = tools.child(actions.child(selection_tools));
        tools.into_any_element()
    }
    fn target_footer(&self, id: String, dirty: usize, cx: &mut Context<Self>) -> AnyElement {
        let blocked = self.operation.busy() || !self.valid;
        let pending_protection = self.protection.draft.dirty(&self.settings);
        let mut footer = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(cx.theme().colors.sidebar)
            .border_t_1()
            .border_color(cx.theme().colors.border);
        footer = footer.children(self.target_protection_footer(cx));
        let progress = if self.operation.running(Kind::SaveProtection) {
            Some(self.text("保護設定を保存中…", "Saving protection…"))
        } else if self.operation.running(Kind::SaveTargets) {
            Some(self.text("保存中…", "Saving…"))
        } else if self.operation.running(Kind::Apply) {
            Some(self.text("適用中…", "Applying…"))
        } else {
            None
        };
        let progress = progress.map(|label| {
            let indicator = div()
                .id("target-save-progress")
                .role(Role::Status)
                .aria_label(label)
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(Spinner::new().small())
                .child(label);
            #[cfg(feature = "test-support")]
            let indicator = indicator.test_support();
            indicator
        });
        let undo_disabled = self
            .targets
            .drafts
            .get(&id)
            .is_none_or(|draft| !draft.can_undo());
        let undo_id = id.clone();
        let discard_id = id.clone();
        let save_id = id.clone();
        let apply_id = id.clone();

        let mut commits = div()
            .flex()
            .items_center()
            .gap_2()
            .children(progress)
            .child(
                theme::button("target-save", cx)
                    .small()
                    .label(if self.settings.active.as_ref() == Some(&id) {
                        self.text("変更を適用", "Apply changes")
                    } else {
                        self.text("選択を保存", "Save selection")
                    })
                    .disabled_elevated(blocked || dirty == 0 || pending_protection)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(service) = this._guard.clone() else {
                            return;
                        };
                        let id = save_id.clone();
                        let Some(draft) = this.targets.drafts.get(&id) else {
                            return;
                        };
                        let value = draft.value.clone();
                        this.run(
                            Kind::SaveTargets,
                            move || service.select_targets(&id, value, false),
                            cx,
                        );
                    })),
            );
        if self.settings.active.as_ref() != Some(&id) {
            commits = commits.child(
                theme::button("target-apply", cx)
                    .small()
                    .label(self.text("この設定で適用", "Apply this selection"))
                    .disabled_elevated(blocked || pending_protection)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(service) = this._guard.clone() else {
                            return;
                        };
                        let id = apply_id.clone();
                        let Some(draft) = this.targets.drafts.get(&id) else {
                            return;
                        };
                        let value = draft.value.clone();
                        this.run(
                            Kind::Apply,
                            move || {
                                service.select_targets(&id, value, false)?;
                                let dll = std::env::current_exe()?
                                    .parent()
                                    .context("executable parent missing")?
                                    .join("file_icons_shell.dll");
                                service.apply(&id, &dll)
                            },
                            cx,
                        );
                    })),
            );
        }

        footer = footer.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(self.back_button("target-back", cx))
                .child(div().text_sm().child(if dirty == 0 {
                    self.text("保存済み", "Saved").to_string()
                } else { format!("{}: {dirty}", self.text("未保存の変更", "Unsaved changes")) }))
                .child(theme::button("target-restart-help", cx).small().icon(IconName::Info)
                    .accessibility_label(self.text("Explorerの表示更新について", "About refreshing Explorer icons"))
                    .tooltip(self.text("チェックを外した対象はWindowsの元の表示になります。\n反映されない場合は、ファイル操作を終えてから\nExplorerを再起動してください。", "Unchecked targets use the original Windows icons.\nIf icons do not update, finish file operations\nand restart Explorer.")))
                .child(
                    theme::button("target-undo", cx)
                        .small()

                        .icon(IconName::RotateCcw)
                        .label(self.text("取り消す", "Undo"))
                        .disabled_elevated(blocked || undo_disabled)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.targets.invalidate();
                            if let Some(draft) = this.targets.drafts.get_mut(&undo_id) {
                                draft.undo();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    theme::button("target-discard", cx)
                        .small()

                        .label(self.text("保存済みに戻す", "Revert to saved"))
                        .disabled_elevated(blocked || dirty == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.targets.invalidate();
                            if let Some(draft) = this.targets.drafts.get_mut(&discard_id) {
                                let value = this
                                    .settings
                                    .selections
                                    .get(&discard_id)
                                    .cloned()
                                    .unwrap_or_default();
                                draft.reset(value);
                            }
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(commits),
        );
        let message = self.operation.message(self.settings.language == "en");
        let has_message = !message.is_empty()
            && !self.operation.running(Kind::SaveTargets)
            && !self.operation.running(Kind::Apply);
        let status = div()
            .id("target-status")
            .role(Role::Status)
            .aria_label(message.clone())
            .text_sm()
            .max_h_16()
            .overflow_y_scroll()
            .text_color(if self.operation.failed() {
                cx.theme().colors.danger
            } else {
                cx.theme().colors.muted_foreground
            })
            .child(message);
        #[cfg(feature = "test-support")]
        let status = status.test_support();
        if has_message {
            footer = footer.child(status);
        }
        footer.into_any_element()
    }
}
