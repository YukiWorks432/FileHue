//! 同梱されたアイコン用DLLへの更新と、その結果を一覧の近くに表示する。
use super::*;
use file_icons_manager_service::IconDllUpdateStatus;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::{Icon, Sizable, button::ButtonVariants};
use gpui_kit::prelude::FluentBuilder;

#[derive(Default)]
pub(super) enum UpdateNotice {
    #[default]
    Hidden,
    Available(String),
    Complete,
    Restored(String),
    MissingSource(String),
    MissingRegistered(String),
    NeedsReview {
        detail: String,
        recovery: bool,
    },
}

impl UpdateNotice {
    pub(super) fn requires_recovery(&self) -> bool {
        matches!(
            self,
            Self::MissingRegistered(_) | Self::NeedsReview { recovery: true, .. }
        )
    }
}

impl Manager {
    pub(super) fn refresh_dll_update(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy() || !self.valid {
            return;
        }
        let Some(service) = self._guard.clone() else {
            return;
        };
        self.dll_update_revision = self.dll_update_revision.wrapping_add(1);
        let revision = self.dll_update_revision;
        let task = cx
            .background_executor()
            .spawn(async move { service.icon_dll_update_status() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if revision != this.dll_update_revision || this.operation.busy() {
                    return;
                }
                this.dll_update = match result {
                    Ok(IconDllUpdateStatus::Available { pack_label, .. }) => {
                        UpdateNotice::Available(pack_label)
                    }
                    Ok(IconDllUpdateStatus::Current | IconDllUpdateStatus::Inactive) => {
                        UpdateNotice::Hidden
                    }
                    Err(error)
                        if matches!(
                            error.downcast_ref::<file_icons_manager_service::IconDllFileMissing>(),
                            Some(file_icons_manager_service::IconDllFileMissing::Bundled)
                        ) =>
                    {
                        UpdateNotice::MissingSource(format!("{error:#}"))
                    }
                    Err(error)
                        if matches!(
                            error.downcast_ref::<file_icons_manager_service::IconDllFileMissing>(),
                            Some(file_icons_manager_service::IconDllFileMissing::Registered)
                        ) =>
                    {
                        UpdateNotice::MissingRegistered(format!("{error:#}"))
                    }
                    Err(error) => UpdateNotice::NeedsReview {
                        recovery: matches!(
                            error.downcast_ref::<file_icons_manager_service::Failure>(),
                            Some(
                                file_icons_manager_service::Failure::Conflict
                                    | file_icons_manager_service::Failure::Recovery
                            )
                        ) || error.is::<file_icons_core::registry::Conflict>(),
                        detail: format!("{error:#}"),
                    },
                };
                if this.dll_update.requires_recovery() {
                    this.conflict_attention = true;
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn dll_update_panel(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let working = self.operation.running(Kind::UpdateIconDll);
        if matches!(self.dll_update, UpdateNotice::Hidden) && !working {
            return div();
        }
        let colors = cx.theme().colors;
        let (title, detail, icon) = match &self.dll_update {
            UpdateNotice::Available(pack) => (
                self.text("アイコン機能の更新", "Icon support update"),
                if self.settings.language == "en" {
                    format!("Keep your {pack} target choices and protection settings.")
                } else {
                    format!("{pack} の対象選択・保護設定を引き継ぎます。")
                },
                IconName::Monitor,
            ),
            UpdateNotice::Complete => (
                self.text("更新完了", "Update complete"),
                self.text(
                    "表示に反映されない場合は、エクスプローラーを再起動してください。",
                    "If the change is not reflected, restart File Explorer.",
                )
                .to_owned(),
                IconName::Check,
            ),
            UpdateNotice::Restored(_) => (
                self.text("更新できませんでした", "Update failed"),
                self.text(
                    "更新前の状態に戻しました。設定は変更していません。",
                    "Restored the previous registration. Your settings are unchanged.",
                )
                .to_owned(),
                IconName::TriangleAlert,
            ),
            UpdateNotice::MissingSource(_) => (
                self.text("更新用のファイルが見つかりません", "Update file not found"),
                self.text(
                    "FileHueを同梱ファイルごと展開し直してから、再確認してください。",
                    "Extract FileHue with its bundled files, then check again.",
                )
                .to_owned(),
                IconName::TriangleAlert,
            ),
            UpdateNotice::MissingRegistered(_) => (
                self.text(
                    "登録先のファイルが見つかりません",
                    "Registered file not found",
                ),
                self.text(
                    "アイコン設定の復旧内容を確認してください。",
                    "Review recovery for the icon settings.",
                )
                .to_owned(),
                IconName::TriangleAlert,
            ),
            UpdateNotice::NeedsReview { recovery: true, .. } => (
                self.text("更新状態を確認できません", "Cannot check update status"),
                self.text(
                    "アイコン設定と詳細を確認してください。",
                    "Review icon settings and details.",
                )
                .to_owned(),
                IconName::TriangleAlert,
            ),
            UpdateNotice::NeedsReview {
                recovery: false, ..
            } => (
                self.text("更新状態を確認できません", "Cannot check update status"),
                self.text(
                    "詳細を確認してから、もう一度確認してください。",
                    "Check the details, then try again.",
                )
                .to_owned(),
                IconName::TriangleAlert,
            ),
            UpdateNotice::Hidden => return div(),
        };
        let info = theme::button("dll-update-details", cx)
            .small()
            .icon(IconName::Info)
            .accessibility_label(self.text("技術情報", "Technical details"))
            .tooltip(self.text("技術情報", "Technical details"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.dll_update_details = !this.dll_update_details;
                cx.notify();
            }));
        let mut heading = div()
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(icon).size_4())
            .child(div().flex_1().min_w_0().font_semibold().child(title))
            .child(info);
        if matches!(self.dll_update, UpdateNotice::Complete) {
            heading = heading.child(
                theme::button("dll-update-dismiss", cx)
                    .small()
                    .icon(IconName::X)
                    .accessibility_label(self.text("閉じる", "Close"))
                    .tooltip(self.text("閉じる", "Close"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dll_update = UpdateNotice::Hidden;
                        cx.notify();
                    })),
            );
        }
        let copy = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(heading)
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(detail),
            );
        let mut action = None;
        if working {
            action = Some(
                div()
                    .min_w(rems(12.5))
                    .h_10()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .when(compact, |status| status.w_full())
                    .child(gpui_kit::component::spinner::Spinner::new().small())
                    .child(self.text("更新中", "Updating"))
                    .into_any_element(),
            );
        } else if matches!(
            self.dll_update,
            UpdateNotice::Available(_) | UpdateNotice::Restored(_)
        ) {
            action = Some(
                theme::button("dll-update", cx)
                    .primary()
                    .min_w(rems(12.5))
                    .icon(IconName::RefreshCw)
                    .label(if matches!(self.dll_update, UpdateNotice::Restored(_)) {
                        self.text("再試行", "Retry")
                    } else {
                        self.text("更新する", "Update")
                    })
                    .when(compact, |button| button.w_full())
                    .disabled_elevated(self.operation.busy() || !self.valid)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(service) = this._guard.clone() else {
                            return;
                        };
                        this.run(Kind::UpdateIconDll, move || service.update_icon_dll(), cx);
                    }))
                    .into_any_element(),
            );
        } else if self.dll_update.requires_recovery() {
            action = Some(
                theme::button("dll-update-review", cx)
                    .primary()
                    .min_w(rems(12.5))
                    .label(self.text("競合を確認", "Review conflicts"))
                    .when(compact, |button| button.w_full())
                    .disabled_elevated(self.operation.busy() || self._guard.is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_conflicts(
                            file_icons_windows::conflict::Strategy::PreserveExternal,
                            window,
                            cx,
                        )
                    }))
                    .into_any_element(),
            );
        } else if matches!(
            self.dll_update,
            UpdateNotice::NeedsReview {
                recovery: false,
                ..
            } | UpdateNotice::MissingSource(_)
        ) {
            action = Some(
                theme::button("dll-update-recheck", cx)
                    .label(self.text("再確認", "Recheck"))
                    .when(compact, |button| button.w_full())
                    .disabled_elevated(self.operation.busy() || !self.valid)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_dll_update(cx)))
                    .into_any_element(),
            );
        }
        let mut row = div()
            .flex()
            .gap_3()
            .when(compact, |row| row.flex_col())
            .when(!compact, |row| row.items_start())
            .child(copy);
        if let Some(button) = action {
            row = row.child(button);
        }
        let mut card = theme::card(cx).p_4().flex().flex_col().gap_3().child(row);
        if self.dll_update_details {
            let technical = match &self.dll_update {
                UpdateNotice::Restored(error) | UpdateNotice::MissingSource(error) | UpdateNotice::MissingRegistered(error) | UpdateNotice::NeedsReview { detail: error, .. } => error.clone(),
                _ => self.text("このアプリに同梱されたアイコン用DLLへ登録を切り替えます。ファイルのプレビュー設定は変更しません。", "Registers the icon DLL bundled with this app. File preview settings are unchanged.").to_owned(),
            };
            card = card.child(
                theme::inset(cx)
                    .id("dll-update-technical")
                    .p_3()
                    .text_sm()
                    .max_h_32()
                    .overflow_y_scroll()
                    .child(technical),
            );
        }
        div().p_4().child(card)
    }
}
