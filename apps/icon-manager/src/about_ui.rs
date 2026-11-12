//! 製品情報と、配布物に付属する権利表示への導線。
use super::*;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;

// 公開済みの版と配布物は、この一覧で確認する。
const DISTRIBUTION_URL: &str = "https://github.com/YukiWorks432/FileHue/releases";

pub(super) struct About {
    pub(super) executable_dir: Option<PathBuf>,
    pub(super) license_text: Option<String>,
    license_error: bool,
    pub(super) scroll: ScrollHandle,
}

impl Default for About {
    fn default() -> Self {
        Self {
            executable_dir: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf)),
            license_text: None,
            license_error: false,
            scroll: ScrollHandle::new(),
        }
    }
}

impl Manager {
    pub(super) fn about_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let body = if let Some(text) = &self.about.license_text {
            div().p_4().child(
                theme::card(cx)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.text("本体のライセンス", "FileHue license")),
                    )
                    .child(div().whitespace_normal().child(text.clone())),
            )
        } else {
            div().p_4().flex().flex_col().gap_4()
            .child(theme::card(cx).p_4().flex().flex_col().gap_3()
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD)
                    .child(self.text("アプリ情報", "About FileHue")))
                .child(format!("FileHue {}", env!("CARGO_PKG_VERSION")))
                .child(div().text_color(muted).child(self.text(
                    "更新は配布ページで確認できます。自動更新は行いません。",
                    "Check the distribution page for updates. FileHue does not update automatically.",
                )))
                .child(div().flex().items_start().child(theme::button("about-distribution", cx)
                    .label(self.text("配布ページを開く", "Open distribution page"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.open_url(DISTRIBUTION_URL);
                    }))))
            )
            .child(theme::card(cx).p_4().flex().flex_col().gap_3()
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD)
                    .child(self.text("ライセンス", "Licenses")))
                .child(div().text_color(muted).child(self.text(
                    "本体と依存ライブラリの権利表示です。各パックの利用条件はパックの詳細で確認できます。",
                    "Licenses for FileHue and its dependencies. Pack-specific terms are available in each pack's details.",
                )))
                .child(self.about_license("about-license", "LICENSE", self.text("本体のライセンスを開く", "Open FileHue license"), cx))
                .child(self.about_license("about-third-party", "licenses/THIRD_PARTY_LICENSES.html", self.text("第三者ライセンスを開く", "Open third-party licenses"), cx))
                .when(self.about.license_error, |card| card.child(self.about_notice("about-license-error", self.text("ライセンスを読み取れませんでした。ファイルを確認してください。", "Could not read the license. Check the bundled file."), cx))))
        };
        let scroll = div()
            .id("about-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.about.scroll)
            .child(body);
        #[cfg(feature = "test-support")]
        let scroll = scroll.test_support();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(scroll)
            .vertical_scrollbar(&self.about.scroll)
    }

    fn about_license(
        &self,
        id: &'static str,
        relative: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let path = self
            .about
            .executable_dir
            .as_ref()
            .map(|dir| dir.join(relative));
        let available = path.as_ref().is_some_and(|path| {
            path.is_file() && (id == "about-license" || path.to_str().is_some())
        });
        div().flex().flex_col().gap_2().child(div().flex().items_start().child(
            theme::button(id, cx).label(label).disabled_elevated(!available)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if id == "about-license" {
                        match path.as_ref().and_then(|path| std::fs::read_to_string(path).ok()) {
                            Some(content) => { this.about.license_text = Some(content); this.about.license_error = false; this.about.scroll.set_offset(point(px(0.), px(0.))); }
                            None => this.about.license_error = true,
                        }
                    } else if let Some(path) = path.as_ref().filter(|path| path.is_file()).and_then(|path| path.to_str()) {
                        cx.open_url(path);
                    }
                    cx.notify();
                }))
        )).when(!available, |row| row.child(self.about_notice(
            if id == "about-license" { "about-license-missing" } else { "about-third-party-missing" },
            self.text(
                "同梱ファイルが見つかりません。開発時はライセンスの準備手順を確認してください。",
                "The bundled file was not found. For development, see the license preparation instructions.",
            ), cx)))
    }

    fn about_notice(&self, id: &'static str, text: &'static str, cx: &App) -> impl IntoElement {
        let notice = div()
            .id(id)
            .aria_label(text)
            .text_color(cx.theme().colors.muted_foreground)
            .child(text);
        #[cfg(feature = "test-support")]
        let notice = notice.test_support();
        notice
    }
}
