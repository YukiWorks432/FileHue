//! 設定の重い経路と、試験用CPU描画に含まれない実プロセスを分離して測る。
use super::*;
use std::{process::Command, time::Instant};

pub(super) fn measure(
    cases: &mut Vec<serde_json::Value>,
    library: &file_icons_windows::Settings,
) -> serde_json::Value {
    let source = &library.packs[1];
    let destination = &library.packs[0];
    let empty = file_icons_core::selection::Selection::default();
    cases.push(crate::performance::measure("selection_copy_6000", || {
        let (copied, unmatched) =
            library.selections[&source.id].copy_to(&source.theme, &destination.theme, &empty);
        assert!(unmatched.is_empty());
        assert!(!copied.enabled("example.perf-0000"));
        copied.changes_from(&empty) as u64
    }));
    // 別プロセスの確保量は親のアロケーターで追跡できない。JSONにも対象外と記録する。
    if let Some(cli) = std::env::var_os("WFI_PERF_CLI") {
        for (name, args) in [
            (
                "cli_edit_dry_run_8x6000",
                vec![
                    "targets",
                    "edit",
                    destination.id.as_str(),
                    "all",
                    "off",
                    "--dry-run",
                    "--json",
                ],
            ),
            (
                "cli_copy_dry_run_8x6000",
                vec![
                    "targets",
                    "copy",
                    source.id.as_str(),
                    destination.id.as_str(),
                    "--dry-run",
                    "--json",
                ],
            ),
        ] {
            cases.push(crate::performance::measure(name, || {
                let output = Command::new(&cli).args(&args).test_output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(document["ok"], true);
                assert_eq!(document["data"]["dry_run"], true);
                let selection: file_icons_core::selection::Selection =
                    serde_json::from_value(document["data"]["selection"].clone()).unwrap();
                assert_eq!(selection, library.selections[&destination.id]);
                selection.changes_from(&empty) as u64
            }));
        }
    }
    let mut startup = serde_json::Map::new();
    if std::env::var_os("WFI_PERF_STARTUP").is_some() {
        let output = tempfile::tempdir().unwrap();
        for (label, settings) in [
            ("empty", &file_icons_windows::Settings::default()),
            ("8x6000", library),
        ] {
            file_icons_windows::save(settings).unwrap();
            let mut samples = Vec::new();
            cases.push(crate::performance::measure(
                &format!("startup_process_to_exit_{label}"),
                || {
                    let result = output.path().join("startup.json");
                    let child = Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--ignored",
                            "--exact",
                            "ui_tests::performance::workflows::startup_in_child",
                            "--nocapture",
                        ])
                        .env("WFI_STARTUP_OUTPUT", &result)
                        .test_output()
                        .unwrap();
                    assert!(
                        child.status.success(),
                        "{}\n{}",
                        String::from_utf8_lossy(&child.stdout),
                        String::from_utf8_lossy(&child.stderr)
                    );
                    let sample: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&result).unwrap()).unwrap();
                    assert_eq!(
                        sample["packs"].as_u64().unwrap(),
                        settings.packs.len() as u64
                    );
                    samples.push(sample);
                    settings.packs.len() as u64
                },
            ));
            startup.insert(
                label.into(),
                serde_json::json!({"warmups":3,"samples":&samples[3..]}),
            );
        }
        file_icons_windows::save(library).unwrap();
    }
    startup.into()
}

#[test]
#[ignore = "親が専用プロファイルと出力先を用意する実DirectX起動計測"]
fn startup_in_child() {
    use gpui_kit::*;
    let output = std::path::PathBuf::from(std::env::var_os("WFI_STARTUP_OUTPUT").unwrap());
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let began = Instant::now();
    let open_manager = Manager::prepare();
    application().with_assets(crate::ui_assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        let before_open = Instant::now();
        let (window, manager) = gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1120.), px(780.)), cx))),
            ..Default::default()
        }, cx, open_manager).unwrap();
        let ready_ns = began.elapsed().as_nanos() as u64;
        let open_ns = before_open.elapsed().as_nanos() as u64;
        window.update(cx, |_, w, cx| w.render_frame(cx)).unwrap();
        let first_frame_ns = began.elapsed().as_nanos() as u64;
        let packs = manager.read(cx).settings.packs.len();
        assert!(manager.read(cx).valid);
        let report = serde_json::json!({"packs":packs,"ready_ns":ready_ns,"open_ns":open_ns,
            "first_frame_ns":first_frame_ns,"memory":process_memory(),
            "scope":"real DirectX window; timer starts at child test entry; frame submission, not display presentation"});
        std::fs::write(&output, serde_json::to_vec(&report).unwrap()).unwrap();
        cx.quit();
    });
}
