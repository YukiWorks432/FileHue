#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(test)]
#[path = "../../../test-support/process.rs"]
mod test_process;
use anyhow::{Context as _, Result};
use file_icons_core::{ThemeChoice, discover};
use file_icons_windows::{Settings, load};
use gpui_kit::{
    base::{Disableable, Selectable},
    component::{
        button::Button,
        theme::{ActiveTheme, ThemeMode},
    },
    *,
};
use std::path::PathBuf;
mod about_ui;
mod catalog_ui;
mod library_ui;
mod operation;
mod original_icons;
mod protection_ui;
mod target_ui;
mod ui_assets;
use operation::{Kind, Operation};
mod recovery_ui;
mod status;
mod theme;
mod thumbnail_ui;
use theme::ButtonElevation;
mod view;
use status::{Failure, Status};
use view::Screen;

#[cfg(all(test, feature = "test-support"))]
mod ui_tests;

#[cfg(all(test, feature = "test-support"))]
#[path = "../../../test-support/performance.rs"]
mod performance;

#[cfg(feature = "test-support")]
use gpui_kit::test::TestSupportExt;

use file_icons_manager_service::{DownloadSource, Manager as ManagerService};
type InitialSettings = (Option<std::sync::Arc<ManagerService>>, Result<Settings>);

struct Manager {
    screen: Screen,
    recovery_return: Screen,
    protection_return: Screen,
    about: about_ui::About,
    protection: protection_ui::Editor,
    recovery: recovery_ui::Editor,
    targets: target_ui::Editor,
    thumbnails: thumbnail_ui::Editor,
    browsing: bool,
    conflict_attention: bool,
    selected: Option<String>,
    rights: String,
    show_rights: bool,
    preview: Entity<gpui_kit::component::input::InputState>,
    _preview_subscription: Subscription,
    download_source: DownloadSource,
    focus: FocusHandle,
    settings: Settings,
    pending: Option<(PathBuf, Vec<ThemeChoice>)>,
    operation: Operation,
    progress_refresh: Option<Task<()>>,
    valid: bool,
    // 管理画面の同時起動による設定・復元記録の上書きを防ぐ。
    _guard: Option<std::sync::Arc<ManagerService>>,
}

impl Manager {
    fn text(&self, japanese: &'static str, english: &'static str) -> &'static str {
        if self.settings.language == "en" {
            english
        } else {
            japanese
        }
    }

    fn read_initial_settings() -> InitialSettings {
        match ManagerService::acquire() {
            Ok(guard) => (Some(std::sync::Arc::new(guard)), load()),
            Err(error) => (None, Err(error)),
        }
    }

    /// Windowsの描画基盤を初期化する間に設定を読む。画面の操作は結果の受領後に始める。
    fn prepare() -> impl FnOnce(&mut Window, &mut App) -> Entity<Self> {
        let settings = std::thread::spawn(Self::read_initial_settings);
        move |window, cx| {
            let initial = settings.join().unwrap_or_else(|_| {
                (
                    None,
                    Err(anyhow::anyhow!("settings loader terminated unexpectedly")),
                )
            });
            Self::open_with_settings(window, cx, initial)
        }
    }

    fn new(window: &mut Window, cx: &mut Context<Self>, initial: InitialSettings) -> Self {
        let preview = cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx));
        let preview_subscription = cx.subscribe(
            &preview,
            |this, _, _: &gpui_kit::component::input::InputEvent, cx| {
                this.inspect_target_preview(cx);
                cx.notify();
            },
        );
        let (guard, result) = initial;
        match result {
            Ok(mut settings) => {
                if settings.language.is_empty() {
                    settings.language = "ja".into();
                }
                Self {
                    screen: Screen::Packs,
                    recovery_return: Screen::Packs,
                    protection_return: Screen::Packs,
                    about: Default::default(),
                    protection: Default::default(),
                    recovery: Default::default(),
                    focus: cx.focus_handle(),
                    targets: target_ui::Editor::new(window, cx),
                    thumbnails: Default::default(),
                    browsing: false,
                    conflict_attention: file_icons_windows::conflict::pending().unwrap_or(true),
                    selected: None,
                    rights: String::new(),
                    show_rights: false,
                    preview,
                    _preview_subscription: preview_subscription,
                    download_source: file_icons_catalog::download,
                    settings,
                    pending: None,
                    operation: if file_icons_windows::conflict::pending().unwrap_or(true) {
                        Operation::new(Status::Error(Failure::IncompleteRecovery, String::new()))
                    } else {
                        Operation::default()
                    },
                    valid: true,
                    progress_refresh: None,
                    _guard: guard,
                }
            }
            Err(error) => {
                let settings = file_icons_windows::load_recovery()
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let recovery_available = guard.is_some() && !settings.journal.is_empty();
                Self {
                    screen: Screen::Packs,
                    recovery_return: Screen::Packs,
                    protection_return: Screen::Packs,
                    about: Default::default(),
                    protection: Default::default(),
                    recovery: Default::default(),
                    focus: cx.focus_handle(),
                    targets: target_ui::Editor::new(window, cx),
                    thumbnails: Default::default(),
                    browsing: false,
                    conflict_attention: file_icons_windows::conflict::pending().unwrap_or(true),
                    selected: None,
                    rights: String::new(),
                    show_rights: false,
                    preview,
                    _preview_subscription: preview_subscription,
                    download_source: file_icons_catalog::download,
                    settings,
                    pending: None,
                    operation: Operation::new(Status::Error(
                        if error.is::<file_icons_windows::UnsupportedSettingsFormat>() {
                            Failure::UnsupportedSettings(recovery_available)
                        } else if recovery_available {
                            Failure::RecoveryAvailable
                        } else {
                            Failure::OpenSettings
                        },
                        format!("{error:#}"),
                    )),
                    valid: false,
                    progress_refresh: None,
                    _guard: guard,
                }
            }
        }
    }

    #[cfg(test)]
    fn open(window: &mut Window, cx: &mut App) -> Entity<Self> {
        Self::open_with_settings(window, cx, Self::read_initial_settings())
    }

    fn open_with_settings(
        window: &mut Window,
        cx: &mut App,
        initial: InitialSettings,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let manager = Self::new(window, cx, initial);
            window.focus(&manager.focus, cx);
            manager
        })
    }

    fn set_language(&mut self, language: &str, cx: &mut Context<Self>) {
        if self.operation.busy() {
            return;
        }
        if !self.valid || file_icons_windows::conflict::pending().unwrap_or(true) {
            self.settings.language = language.into();
            cx.notify();
            return;
        }
        self.settings.language = language.into();
        let language = language.to_owned();
        let manager = self._guard.clone().expect("valid manager");
        self.run(Kind::Language, move || manager.language(language), cx);
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy() || !self.valid {
            return;
        }
        self.operation.begin(Kind::Choose, None);
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                self.text(
                    "展開済みVS Code拡張のフォルダーを選択",
                    "Select an unpacked VS Code extension folder",
                )
                .into(),
            ),
        });
        let guard = self._guard.clone();
        cx.spawn(async move |this, cx| {
            let root = match dialog.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                error => {
                    let _ = this.update(cx, |this, cx| {
                        this.operation
                            .finish(Status::Error(Failure::SelectFolder, format!("{error:?}")));
                        cx.notify();
                    });
                    return;
                }
            };
            let result = if let Some(root) = root {
                Some(
                    cx.background_executor()
                        .spawn(async move {
                            let _guard = guard;
                            discover(&root).map(|choices| (root, choices))
                        })
                        .await,
                )
            } else {
                None
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Some(Ok(selection)) => {
                        this.pending = Some(selection);
                        this.browsing = false;
                        this.operation.finish(Status::Idle);
                    }
                    Some(Err(error)) => this
                        .operation
                        .finish(Status::Error(Failure::ReadTheme, format!("{error:#}"))),
                    None => this.operation.finish(Status::Idle),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn import(&mut self, root: PathBuf, id: String, cx: &mut Context<Self>) {
        let Some(manager) = self._guard.clone() else {
            return;
        };
        let progress = std::sync::Arc::new(file_icons_catalog::Progress::default());
        progress
            .stage
            .store(2, std::sync::atomic::Ordering::Relaxed);
        let display_progress = progress.clone();
        self.start(
            Kind::Import,
            move || manager.import(&root, &id, &progress).map(|_| ()),
            false,
            Some(display_progress),
            cx,
        );
    }
    fn apply(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(manager) = self._guard.clone() else {
            return;
        };
        self.run(
            Kind::Apply,
            move || {
                let dll = std::env::current_exe()?
                    .parent()
                    .context("executable parent missing")?
                    .join("file_icons_shell.dll");
                manager.apply(&id, &dll)
            },
            cx,
        );
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Some(progress) = self.operation.progress() {
            progress.cancel();
        }
    }
}

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.get(1).is_some_and(|v| v == "--original-icon-worker") {
        let result = if args.len() == 4 {
            original_icons::worker(args[2].clone().into(), args[3].clone().into())
        } else {
            Err(anyhow::anyhow!("invalid preview arguments"))
        };
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    // 開発用の実起動確認。Shell登録を変更せず、ウィンドウ初期化後に終了する。
    let smoke_test = std::env::args().any(|argument| argument == "--smoke-test");
    let open_manager = Manager::prepare();
    application().with_assets(ui_assets::Assets).run(move |cx| {
        init(cx);
        theme::apply(ThemeMode::Dark, cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        open_window(
            WindowOptions {
                // OSのウィンドウ制約。内部の余白や文字はremで拡大する。
                window_min_size: Some(size(px(640.), px(640.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("FileHue".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1120.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            open_manager,
        )
        .expect("cannot open window");
        cx.activate(true);
        if smoke_test {
            let timer = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(2));
            cx.spawn(async move |cx| {
                timer.await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
}
