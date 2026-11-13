use super::*;
use gpui_kit::component::{
    IndexPath,
    list::{List, ListDelegate, ListItem, ListState},
};
use gpui_kit::prelude::FluentBuilder;

#[derive(PartialEq, Eq)]
struct CategoryRow {
    key: Option<&'static str>,
    label: &'static str,
    count: String,
    active: bool,
}

pub(super) struct CategoryList {
    rows: Vec<CategoryRow>,
    focus: Option<FocusHandle>,
    selected: Option<IndexPath>,
    owner: WeakEntity<Manager>,
}

// Listの移動位置と、Enterで確定した絞り込みを別の表示状態にする。
#[derive(IntoElement)]
pub(super) struct CategoryItem {
    item: ListItem,
    cursor: bool,
    focus: Option<FocusHandle>,
}

impl Selectable for CategoryItem {
    fn selected(mut self, selected: bool) -> Self {
        self.cursor = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.cursor
    }
}

impl RenderOnce for CategoryItem {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        self.item.secondary_selected(
            self.cursor && self.focus.is_some_and(|focus| focus.is_focused(window)),
        )
    }
}

impl ListDelegate for CategoryList {
    type Item = CategoryItem;
    fn items_count(&self, _: usize, _: &App) -> usize {
        self.rows.len()
    }
    fn set_selected_index(
        &mut self,
        selected: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = selected;
    }
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let key = self
            .selected
            .and_then(|ix| self.rows.get(ix.row))
            .map(|row| row.key);
        if let Some(key) = key {
            let _ = self.owner.update(cx, |this, cx| {
                this.targets.category = key;
                this.targets.auxiliary = None;
                this.targets.reset_page();
                if window.viewport_size().width < window.rem_size() * 64. {
                    this.targets.search.focus_handle(cx).focus(window, cx);
                }
                cx.notify();
            });
        }
    }
    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let row = self.rows.get(ix.row)?;
        let id = row
            .key
            .map(|key| format!("target-category-{key}"))
            .unwrap_or_else(|| "target-all-categories".into());
        let item = ListItem::new(SharedString::from(id))
            .h_12()
            .w_full()
            .min_w_0()
            .when(ix.row + 1 < self.rows.len(), |this| {
                this.border_b_1().border_color(cx.theme().colors.border)
            })
            .selected(row.active)
            .accessibility_label(format!(
                "{}: {}{}",
                row.label,
                row.count,
                if row.active { " ✓" } else { "" }
            ))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().text_sm().line_height(relative(1.25)).child(row.label))
                    .child(
                        div()
                            .text_xs()
                            .line_height(relative(1.25))
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(row.count.clone()),
                    ),
            );
        Some(CategoryItem {
            item,
            cursor: false,
            focus: self.focus.clone(),
        })
    }
}

pub(super) fn create(
    window: &mut Window,
    cx: &mut Context<Manager>,
) -> Entity<ListState<CategoryList>> {
    let owner = cx.weak_entity();
    let list = cx.new(|cx| {
        let mut list = ListState::new(
            CategoryList {
                rows: Vec::new(),
                focus: None,
                selected: None,
                owner,
            },
            window,
            cx,
        );
        list.delegate_mut().focus = Some(list.focus_handle(cx));
        list
    });
    let focus = list.focus_handle(cx);
    for subscription in [
        cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
        cx.on_blur(&focus, window, |_, _, cx| cx.notify()),
    ] {
        subscription.detach();
    }
    list
}

impl Manager {
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn prepare_category_visual(
        &mut self,
        view: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.targets.categories_visible = view != "collapsed";
        self.targets.auxiliary = (view == "categories").then_some(Auxiliary::Categories);
        if view == "focus" {
            self.targets.categories.update(cx, |state, cx| {
                state.set_selected_index(Some(IndexPath::new(1)), window, cx);
            });
            self.targets.categories.focus_handle(cx).focus(window, cx);
        }
    }

    pub(super) fn target_categories(
        &self,
        all: &[Target],
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(test)]
        let _categories = crate::performance::section("target_categories");
        let count =
            |enabled, total| format!("{} {enabled} / {total}", self.text("対象", "Included"));
        let mut rows = vec![CategoryRow {
            key: None,
            label: self.text("すべてのカテゴリ", "All categories"),
            count: count(
                self.targets
                    .results
                    .as_ref()
                    .unwrap()
                    .counts
                    .values()
                    .map(|count| count.enabled)
                    .sum::<usize>(),
                all.len(),
            ),
            active: self.targets.category.is_none(),
        }];
        for &(key, ja, en) in CATEGORIES {
            if let Some(counts) = self.targets.results.as_ref().unwrap().counts.get(key) {
                rows.push(CategoryRow {
                    key: Some(key),
                    label: self.text(ja, en),
                    count: count(counts.enabled, counts.total),
                    active: self.targets.category == Some(key),
                });
            }
        }
        self.targets.categories.update(cx, |state, cx| {
            let changed = state
                .delegate()
                .rows
                .iter()
                .map(|r| r.key)
                .ne(rows.iter().map(|r| r.key));
            let active = rows.iter().position(|r| r.active).unwrap_or(0);
            if state.delegate().rows != rows {
                state.delegate_mut().rows = rows;
                cx.notify();
            }
            if changed || state.selected_index().is_none() {
                state.set_selected_index(Some(IndexPath::new(active)), window, cx);
            }
        });
        let focus = self.targets.categories.focus_handle(cx);
        theme::card(cx)
            .id("target-categories")
            .w_64()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .p_2()
            .when(compact, |this| this.w_full())
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::ListFilter)
                            .size_4(),
                    )
                    .child(self.text("カテゴリで絞り込み", "Browse categories")),
            )
            .child(
                div()
                    .id("target-category-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        focus.focus(window, cx)
                    })
                    .child(List::new(&self.targets.categories).p_0()),
            )
            .into_any_element()
    }
}
