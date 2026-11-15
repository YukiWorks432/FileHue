//! 隔離したプロファイルとレジストリでDLL更新と状態ごとの描画を確認する。
use super::*;
use crate::{
    dll_update_ui::UpdateNotice,
    operation::{Kind, Operation},
    status::Status,
};
use gpui_kit::application;

#[test]
#[ignore = "専用プロファイルでDLL更新の実描画を確認する"]
fn dll_update_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let _fixture = super::target_protection::fixture();
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            cx.spawn(async move |cx| {
                for (language, mode, width, font) in [
                    ("ja", ThemeMode::Dark, 1120., 16.),
                    ("en", ThemeMode::Light, 1120., 16.),
                    ("ja", ThemeMode::Dark, 640., 18.),
                    ("en", ThemeMode::Light, 640., 18.),
                ] {
                    for state in [
                        "available",
                        "working",
                        "complete",
                        "restored",
                        "review",
                        "missing_source",
                        "missing_registered",
                    ] {
                        window
                            .update(cx, |_, w, cx| {
                                theme::apply(mode, cx);
                                gpui_kit::component::theme::Theme::update(cx, |theme| {
                                    theme.font_size = px(font)
                                });
                                manager.update(cx, |this, cx| {
                                    this.dll_update_revision =
                                        this.dll_update_revision.wrapping_add(1);
                                    this.settings.language = language.into();
                                    this.settings.active =
                                        this.settings.packs.first().map(|pack| pack.id.clone());
                                    this.operation = Operation::new(Status::Idle);
                                    this.dll_update = match state {
                                        "available" | "working" => {
                                            UpdateNotice::Available("TypeScript icons".into())
                                        }
                                        "complete" => UpdateNotice::Complete,
                                        "restored" => {
                                            UpdateNotice::Restored("fixture failure".into())
                                        }
                                        "review" => UpdateNotice::NeedsReview {
                                            detail: "fixture conflict".into(),
                                            recovery: true,
                                        },
                                        "missing_source" => {
                                            UpdateNotice::MissingSource("fixture missing".into())
                                        }
                                        "missing_registered" => UpdateNotice::MissingRegistered(
                                            "fixture missing".into(),
                                        ),
                                        _ => unreachable!(),
                                    };
                                    if state == "working" {
                                        this.operation.begin(Kind::UpdateIconDll, None);
                                    } else if state == "restored" || state == "review" {
                                        this.operation.begin(Kind::UpdateIconDll, None);
                                        this.operation.finish(Status::Error(
                                            if state == "restored" {
                                                crate::status::Failure::PreviousPackRestored
                                            } else {
                                                crate::status::Failure::RegistrationConflict
                                            },
                                            "fixture error".into(),
                                        ));
                                    }
                                    cx.notify();
                                });
                                w.resize(size(
                                    px(width),
                                    px(if width == 640. { 640. } else { 780. }),
                                ));
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(350))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                w.render_to_image()
                                    .unwrap()
                                    .save(
                                        output.join(format!("dll-{state}-{language}-{width}.png")),
                                    )
                                    .unwrap();
                            })
                            .unwrap();
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

#[test]
fn update_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();
    let copied_executable = profile.path().join("test.exe");
    std::fs::copy(&executable, &copied_executable).unwrap();
    std::fs::copy(
        executable.parent().unwrap().join("file_icons_shell.dll"),
        profile.path().join("file_icons_shell.dll"),
    )
    .unwrap();
    let output = std::process::Command::new(copied_executable)
        .args([
            "--ignored",
            "--exact",
            "ui_tests::dll_update::update_in_child",
            "--nocapture",
        ])
        .env("LOCALAPPDATA", profile.path())
        .env("WFI_GUI_PROBE", "1")
        .test_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[gpui_kit::test]
#[ignore = "親試験の専用プロファイルと隔離レジストリでのみ実行する"]
fn update_in_child(cx: &mut TestAppContext) {
    use file_icons_core::registry::Registry;
    use file_icons_manager_service::{IconDllFileMissing, IconDllUpdateStatus};
    use std::io::Write;
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let registry = super::target_protection::fixture();
    let baseline = super::registration::snapshot(&registry._classes);
    let root = home().unwrap();
    if let Some(previous_profile) = std::env::var_os("WFI_PREVIOUS_PROFILE") {
        let previous_profile = std::path::PathBuf::from(previous_profile);
        for filename in ["settings.json", "settings.library.json"] {
            std::fs::copy(previous_profile.join(filename), root.join(filename)).unwrap();
        }
        let legacy = load().unwrap();
        assert_eq!(legacy.packs[0].id, "filehue-tests.legacy-fixture/test");
        assert!(!legacy.preserve_existing_icons);
        assert_eq!(
            legacy.selections[&legacy.packs[0].id].explicit("extension:json"),
            Some(false)
        );
    }
    let source = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("file_icons_shell.dll");
    let old = root.join("previous-version.dll");
    if let Some(previous_release) = std::env::var_os("WFI_PREVIOUS_ICON_DLL") {
        std::fs::copy(previous_release, &old).unwrap();
    } else {
        std::fs::copy(&source, &old).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&old)
            .unwrap()
            .write_all(b"previous fixture")
            .unwrap();
    }
    let thumbnail = root.join("thumbnail-settings.json");
    std::fs::write(&thumbnail, b"unchanged thumbnail fixture").unwrap();
    let thumbnail_recovery = root.join("thumbnail-recovery.json");
    std::fs::write(&thumbnail_recovery, b"unchanged thumbnail recovery").unwrap();
    let service = crate::ManagerService::acquire().unwrap();
    let mut settings = load().unwrap();
    if settings.preserve_existing_icons {
        settings.preserve_existing_icons = false;
        file_icons_windows::save(&settings).unwrap();
    }
    let id = settings.packs[0].id.clone();
    service.apply(&id, &old).unwrap();
    let previous = load().unwrap();
    let previous_settings = std::fs::read(root.join("settings.json")).unwrap();
    let previous_recovery = std::fs::read(root.join("recovery.json")).unwrap();
    assert!(!previous.journal.is_empty());
    assert!(matches!(
        service.icon_dll_update_status().unwrap(),
        IconDllUpdateStatus::Available { .. }
    ));
    assert_eq!(
        std::fs::read(root.join("settings.json")).unwrap(),
        previous_settings
    );
    assert_eq!(
        std::fs::read(root.join("recovery.json")).unwrap(),
        previous_recovery
    );
    let registered = file_icons_windows::stage_shell(&old).unwrap();
    let held_registered = registered.with_extension("held");
    std::fs::rename(&registered, &held_registered).unwrap();
    let error = service.icon_dll_update_status().unwrap_err();
    assert!(matches!(
        error.downcast_ref::<IconDllFileMissing>(),
        Some(IconDllFileMissing::Registered)
    ));
    assert!(service.update_icon_dll().is_err());
    std::fs::rename(&held_registered, &registered).unwrap();
    let held_source = source.with_extension("held");
    std::fs::rename(&source, &held_source).unwrap();
    let error = service.icon_dll_update_status().unwrap_err();
    assert!(matches!(
        error.downcast_ref::<IconDllFileMissing>(),
        Some(IconDllFileMissing::Bundled)
    ));
    assert!(service.update_icon_dll().is_err());
    std::fs::rename(&held_source, &source).unwrap();
    assert_eq!(
        std::fs::read(root.join("settings.json")).unwrap(),
        previous_settings
    );
    assert_eq!(
        std::fs::read(root.join("recovery.json")).unwrap(),
        previous_recovery
    );
    let change = previous
        .journal
        .iter()
        .find(|change| !change.key.contains("CLSID"))
        .unwrap();
    let mut user = file_icons_windows::UserRegistry;
    user.write(&change.key, &change.name, None).unwrap();
    let conflict = super::registration::snapshot(&registry._classes);
    assert!(service.icon_dll_update_status().is_err());
    assert!(service.update_icon_dll().is_err());
    assert_eq!(super::registration::snapshot(&registry._classes), conflict);
    assert_eq!(
        std::fs::read(root.join("settings.json")).unwrap(),
        previous_settings
    );
    assert_eq!(
        std::fs::read(root.join("recovery.json")).unwrap(),
        previous_recovery
    );
    user.write(&change.key, &change.name, Some(&change.after))
        .unwrap();
    drop(service);
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap()
    });
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        manager.update(cx, |this, _| {
            assert!(matches!(this.dll_update, UpdateNotice::Available(_)))
        });
        w.render_frame(cx);
        w.click("dll-update-details", cx);
        manager.update(cx, |this, _| assert!(this.dll_update_details));
        w.render_frame(cx);
        w.press("escape", cx);
        manager.update(cx, |this, _| assert!(!this.dll_update_details));
        w.render_frame(cx);
        w.click("dll-update", cx);
        manager.update(cx, |this, _| {
            assert!(this.operation.running(Kind::UpdateIconDll))
        });
    })
    .unwrap();
    cx.run_until_parked();
    manager.update(cx, |this, _| {
        assert!(matches!(this.dll_update, UpdateNotice::Complete));
        assert_eq!(
            this._guard
                .as_ref()
                .unwrap()
                .icon_dll_update_status()
                .unwrap(),
            IconDllUpdateStatus::Current
        );
        assert_eq!(this.settings.active, previous.active);
        assert_eq!(
            serde_json::to_value(&this.settings.selections).unwrap(),
            serde_json::to_value(&previous.selections).unwrap()
        );
        assert_eq!(
            this.settings.preserve_existing_icons,
            previous.preserve_existing_icons
        );
        assert_eq!(
            serde_json::to_value(&this.settings.icon_protection_rules).unwrap(),
            serde_json::to_value(&previous.icon_protection_rules).unwrap()
        );
        assert_eq!(this.settings.language, previous.language);
        assert_eq!(
            std::fs::read(&thumbnail).unwrap(),
            b"unchanged thumbnail fixture"
        );
        assert_eq!(
            std::fs::read(&thumbnail_recovery).unwrap(),
            b"unchanged thumbnail recovery"
        );
        this._guard.as_ref().unwrap().restore(true).unwrap();
    });
    assert_eq!(super::registration::snapshot(&registry._classes), baseline);
}
