//! 意味別の色を一箇所でKitへ割り当てる。本体UIと素材の色は分離する。
use gpui_kit::{
    App, Div, Hsla, Styled,
    component::theme::{ActiveTheme, Theme, ThemeMode},
    div, hsla,
};

fn color(hue: f32, saturation: f32, lightness: f32) -> Hsla {
    hsla(hue / 360., saturation / 100., lightness / 100., 1.)
}

pub fn apply(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    Theme::update(cx, |theme| {
        let dark = mode.is_dark();
        let background = if dark {
            color(240., 10., 4.)
        } else {
            color(0., 0., 94.)
        };
        let surface = if dark {
            color(240., 6., 10.)
        } else {
            color(0., 0., 96.)
        };
        let foreground = if dark {
            color(0., 0., 98.)
        } else {
            color(240., 10., 4.)
        };
        let muted = if dark {
            color(240., 5., 64.)
        } else {
            color(240., 5., 40.)
        };
        let primary = if dark {
            color(246., 61., 50.)
        } else {
            color(248., 68., 61.)
        };
        let overlay = if dark {
            color(0., 0., 100.)
        } else {
            color(0., 0., 0.)
        };
        let colors = &mut theme.colors;
        colors.background = background;
        colors.foreground = foreground;
        // 面が手前へ重なるほど明度を増す。部品ごとの独立した配色を作らない。
        colors.sidebar = surface;
        colors.sidebar_foreground = foreground;
        colors.group_box = surface;
        colors.muted = if dark {
            color(240., 4., 16.)
        } else {
            color(0., 0., 97.5)
        };
        colors.popover = if dark {
            color(240., 5., 22.)
        } else {
            color(0., 0., 100.)
        };
        colors.popover_foreground = foreground;
        colors.input = overlay.alpha(0.18);
        colors.secondary = colors.muted;
        colors.secondary_foreground = foreground;
        colors.group_box_foreground = foreground;
        colors.muted_foreground = muted;
        colors.primary = primary;
        colors.primary_foreground = color(226., 100., 97.);
        colors.button_primary = primary;
        colors.button_primary_foreground = colors.primary_foreground;
        colors.button_primary_hover = primary.blend(overlay.alpha(0.06));
        colors.button_primary_active = primary.blend(overlay.alpha(0.10));
        colors.ring = primary;
        colors.border = overlay.alpha(0.12);
        colors.button = if dark {
            colors.popover
        } else {
            color(0., 0., 98.)
        };
        colors.button_foreground = foreground;
        colors.button_hover = if dark {
            colors.popover.blend(overlay.alpha(0.06))
        } else {
            color(0., 0., 99.)
        };
        colors.button_active = if dark {
            colors.popover.blend(overlay.alpha(0.10))
        } else {
            colors.popover
        };
        colors.danger = if dark {
            color(0., 80., 50.)
        } else {
            color(357., 100., 45.)
        };
        // 状態表示の文字と背景を組にし、明暗どちらでも読めるようにする。
        colors.success = if dark {
            color(149., 44., 14.7)
        } else {
            color(143., 50., 92.9)
        };
        colors.success_foreground = if dark {
            color(138., 79., 81.2)
        } else {
            color(149., 60., 21.4)
        };
        colors.warning = if dark {
            color(39., 50., 15.7)
        } else {
            color(42.5, 100., 90.6)
        };
        colors.warning_foreground = if dark {
            color(40.4, 100., 80.8)
        } else {
            color(35.9, 83.6, 23.9)
        };
        colors.info = if dark {
            color(212.3, 53., 19.2)
        } else {
            color(215., 100., 95.3)
        };
        colors.info_foreground = if dark {
            color(208.8, 100., 85.3)
        } else {
            color(212.7, 60., 32.4)
        };
        // 製品指定の単色を明示し、登録テーマの同色グラデーションを引き継がない。
        theme.tokens.button = theme.colors.button.into();
        theme.tokens.button_hover = theme.colors.button_hover.into();
        theme.tokens.button_active = theme.colors.button_active.into();
        theme.tokens.button_primary = theme.colors.button_primary.into();
        theme.tokens.button_primary_hover = theme.colors.button_primary_hover.into();
        theme.tokens.button_primary_active = theme.colors.button_primary_active.into();
        theme.tokens.switch_thumb = color(0., 0., 98.).into();
        theme.shadow = true;
        theme.font_family = "Yu Gothic UI".into();
    });
}

/// 作業単位の面。外側の間隔は配置元、内側の余白は内容が所有する。
pub fn card(cx: &App) -> Div {
    div()
        .bg(cx.theme().colors.group_box)
        .border_1()
        .border_color(cx.theme().colors.border)
        .rounded_lg()
}

/// カード内の比較・詳細用の面。カードの背景から一段上げる。
pub fn inset(cx: &App) -> Div {
    card(cx).bg(cx.theme().colors.muted).rounded_md()
}

/// 押せる面と影を共通化する。背景はKit標準の状態別トークンへ委ねる。
pub fn button(id: impl Into<gpui_kit::ElementId>, cx: &App) -> gpui_kit::component::button::Button {
    gpui_kit::component::button::Button::new(id)
        .shadow_xs()
        .border_color(cx.theme().colors.border)
}

/// ラベルも含めた操作領域を、基準文字16pxで44px以上確保する。
pub fn checkbox(id: impl Into<gpui_kit::ElementId>) -> gpui_kit::component::checkbox::Checkbox {
    gpui_kit::component::checkbox::Checkbox::new(id)
        .min_w(gpui_kit::rems(2.75))
        .min_h(gpui_kit::rems(2.75))
        .items_center()
        .px_2()
}

/// 無効な操作はKitの無効表示に合わせて影も除く。
pub trait ButtonElevation {
    fn disabled_elevated(self, disabled: bool) -> Self;
}
impl ButtonElevation for gpui_kit::component::button::Button {
    fn disabled_elevated(self, disabled: bool) -> Self {
        use gpui_kit::{component::Disableable, prelude::FluentBuilder};
        self.disabled(disabled)
            .when(disabled, |button| button.shadow_none())
    }
}
