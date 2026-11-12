//! 同一の専用プロファイル・入力で、GUIのCPU描画と調査を比較する手動計測。
use super::*;
use crate::performance::{live_bytes, measure, write};
use file_icons_core::{CompiledTheme, ThemeRules};
mod registration;
mod workflows;

fn process_memory() -> serde_json::Value {
    use windows::Win32::System::{ProcessStatus::*, Threading::GetCurrentProcess};
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            std::mem::size_of_val(&counters) as u32,
        )
        .unwrap();
    }
    serde_json::json!({"private_bytes":counters.PrivateUsage,"working_set_bytes":counters.WorkingSetSize,"peak_working_set_bytes":counters.PeakWorkingSetSize})
}

#[test]
#[ignore = "just perf-gui <未作成のJSON> から明示実行する"]
fn gui_performance() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::performance::gui_performance_in_child",
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
#[ignore = "親が専用プロファイルを作る"]
fn gui_performance_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let registry = super::target_protection::fixture();
    let mut cases = if std::env::var_os("WFI_PERF_FOCUS").is_some() {
        Vec::new()
    } else {
        registration::measure(&registry)
    };
    let mut settings = load().unwrap();
    let icon = settings.packs[0]
        .theme
        .resolve("sample.json")
        .unwrap()
        .to_path_buf();
    let mut rules = ThemeRules::default();
    for index in 0..6000 {
        rules
            .extensions
            .insert(format!("perf-{index:04}"), icon.clone());
    }
    settings.packs[0].theme = CompiledTheme::new(rules).unwrap();
    // Windowsの調査は、パック内外が同じ関連付けを共有する登録で比較する。
    for root in [&registry._classes, &registry._effective] {
        for index in 0..400 {
            root.create_subkey(format!(".perf-{index:04}"))
                .unwrap()
                .0
                .set_value("", &"Fixture.Performance")
                .unwrap();
        }
        root.create_subkey("Fixture.Performance\\DefaultIcon")
            .unwrap()
            .0
            .set_value("", &"shell32.dll,2")
            .unwrap();
    }
    let mut inspection_settings = settings.clone();
    // 登録調査の条件を400対象に限定する。GUIの6000対象とは別に記録する。
    let mut small = settings.packs[0].theme.rules().clone();
    small.extensions.retain(|key, _| key.as_str() < "perf-0400");
    inspection_settings.packs[0].theme = CompiledTheme::new(small).unwrap();
    let sources = file_icons_windows::inspect_existing_icons(
        &inspection_settings.packs[0],
        &inspection_settings,
    )
    .unwrap();
    inspection_settings.icon_protection_rules.set_provider(
        &sources,
        sources[0].provider_id.as_deref().unwrap(),
        Some(false),
    );
    if std::env::var_os("WFI_PERF_FOCUS").is_none() {
        cases.push(measure("registry_report_400", || {
            let result = file_icons_windows::protection_report(
                &inspection_settings.packs[0],
                &inspection_settings,
            )
            .unwrap();
            assert!(result.is_empty());
            result.len() as u64
        }));
        cases.push(measure("original_preview_400", || {
            let previews = crate::original_icons::generate(&sources).unwrap();
            assert_eq!(previews.paths.len(), sources.len());
            previews.paths.len() as u64
        }));
    }
    // 全パックの解析を含む読み込みを、GUIの描画区間から分ける。
    let mut library = settings.clone();
    library.packs.clear();
    for index in 0..8 {
        let mut pack = settings.packs[0].clone();
        pack.version = format!("{index}.0.0");
        pack.id = format!("performance/theme@{}", pack.version);
        let mut selection = file_icons_core::selection::Selection::default();
        for target in pack.theme.targets() {
            selection.set(&target.id, Some(false)).unwrap();
        }
        library.selections.insert(pack.id.clone(), selection);
        library.packs.push(pack);
    }
    file_icons_windows::save(&library).unwrap();
    let library_bytes = std::fs::metadata(home().unwrap().join("settings.library.json"))
        .unwrap()
        .len();
    cases.push(measure("settings_load_8x6000", || {
        let loaded = load().unwrap();
        assert_eq!(loaded.packs.len(), 8);
        assert!(!loaded.selections["performance/theme@0.0.0"].enabled("sample.perf-0000"));
        loaded
            .packs
            .iter()
            .map(|p| p.theme.rules().extensions.len() as u64)
            .sum()
    }));
    let startup = workflows::measure(&mut cases, &library);
    let memory_before_open = process_memory();
    let opened = std::time::Instant::now();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1120.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::open,
        )
        .unwrap()
    });
    cx.update_window(window, |_, w, cx| w.render_frame(cx))
        .unwrap();
    let open_first_frame_ns = opened.elapsed().as_nanos() as u64;
    let memory_after_open = process_memory();
    manager.update(cx, |this, _| this.settings = library);
    cases.push(measure("pack_preview_render_6000_overrides", || {
        manager.update(cx, |this, _| {
            this.selected = Some("performance/theme@0.0.0".into());
        });
        cx.update_window(window, |_, w, cx| {
            manager.update(cx, |_, cx| cx.notify());
            w.render_frame(cx);
            assert!(w.find("example-0").label().unwrap().contains("Cargo.toml"));
        })
        .unwrap();
        8
    }));
    manager.update(cx, |this, cx| {
        this.selected = None;
        let pack = &this.settings.packs[0];
        this.targets.performance_seed(pack.id.clone(), &pack.theme);
        this.screen = crate::Screen::Targets;
        cx.notify();
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-copy-open", cx);
        w.render_frame(cx);
    })
    .unwrap();
    cases.push(measure("copy_panel_render_7x6000", || {
        cx.update_window(window, |_, w, cx| {
            manager.update(cx, |_, cx| cx.notify());
            w.render_frame(cx);
            assert!(w.find("target-copy-performance/theme@1.0.0").visible());
            assert!(w.find("version-summary-performance/theme@1.0.0").visible());
        })
        .unwrap();
        7
    }));
    cx.update_window(window, |_, w, cx| w.press("alt-left", cx))
        .unwrap();
    // コピーを実行して戻す費用も測り、描画からクリックへ費用を移した影響を分ける。
    cases.push(measure("copy_click_and_undo_6000", || {
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click("target-copy-open", cx);
            w.render_frame(cx);
            w.click("target-copy-performance/theme@1.0.0", cx);
            w.render_frame(cx);
            assert_eq!(
                w.find("target-toggle-extension:perf-0000").checked(),
                Some(false)
            );
            w.click("target-undo", cx);
            w.render_frame(cx);
            assert_eq!(
                w.find("target-toggle-extension:perf-0000").checked(),
                Some(true)
            );
        })
        .unwrap();
        6000
    }));
    manager.update(cx, |this, cx| {
        this.settings = settings;
        let id = this.settings.packs[0].id.clone();
        this.targets
            .performance_seed(id, &this.settings.packs[0].theme);
        this.screen = crate::Screen::Targets;
        this.protection.inspection.sources = sources.clone();
        this.protection.inspection.loading = false;
        this.refresh_protection_policy(cx);
    });
    cases.push(measure("inspection_completion_400", || {
        manager.update(cx, |this, _| {
            this.accept_protection_sources(sources.clone());
            assert_eq!(this.protection.inspection.sources.len(), sources.len());
        });
        sources.len() as u64
    }));
    let memory_ready = process_memory();
    cases.push(measure("targets_render_6000", || {
        cx.update_window(window, |_, w, cx| {
            manager.update(cx, |_, cx| cx.notify());
            w.render_frame(cx);
            assert_eq!(
                w.find("target-toggle-extension:perf-0000").checked(),
                Some(true)
            );
        })
        .unwrap();
        6000
    }));
    cases.push(measure("search_6000", || {
        cx.update_window(window, |_, w, cx| {
            w.click("target-search", cx);
            w.input("perf-0599", cx);
            w.render_frame(cx);
            assert!(w.find("target-toggle-extension:perf-0599").visible());
            w.press("ctrl-a", cx);
            w.press("backspace", cx);
            w.render_frame(cx);
        })
        .unwrap();
        1
    }));
    manager.update(cx, |this, _| {
        this.targets.performance_category(Some("other"))
    });
    cases.push(measure("selected_category_render_6000", || {
        cx.update_window(window, |_, w, cx| {
            manager.update(cx, |_, cx| cx.notify());
            w.render_frame(cx);
            assert_eq!(w.find("target-category-toggle-other").checked(), Some(true));
        })
        .unwrap();
        6000
    }));
    cases.push(measure("category_click_and_undo_6000", || {
        cx.update_window(window, |_, w, cx| {
            w.click("target-category-toggle-other", cx);
            w.render_frame(cx);
            assert_eq!(
                w.find("target-toggle-extension:perf-0000").checked(),
                Some(false)
            );
            w.click("target-undo", cx);
            w.render_frame(cx);
            assert_eq!(
                w.find("target-toggle-extension:perf-0000").checked(),
                Some(true)
            );
        })
        .unwrap();
        6000
    }));
    manager.update(cx, |this, _| this.targets.performance_category(None));
    manager.update(cx, |this, cx| {
        this.performance_protection(false, cx);
    });
    cases.push(measure("protection_render_400", || {
        cx.update_window(window, |_, w, cx| {
            manager.update(cx, |_, cx| cx.notify());
            w.render_frame(cx);
        })
        .unwrap();
        400
    }));
    cases.push(measure("protection_collapse_expand_400", || {
        manager.update(cx, |this, cx| {
            this.performance_protection(true, cx);
        });
        400
    }));
    let before = live_bytes();
    let memory_before_history = process_memory();
    manager.update(cx, |this, _| {
        this.targets.performance_edit_32();
    });
    let retained = live_bytes() as i64 - before as i64;
    let memory_after_history = process_memory();
    write(
        cases,
        serde_json::json!({"targets":6000,"inspected_extensions":400,"viewport":[1120,780],
        "font":16,"language":"ja","mode":"dark","renderer":"GPUI test-support CPU layout; excludes GPU presentation",
            "focus_only":std::env::var_os("WFI_PERF_FOCUS").is_some(),"startup":startup,
            "child_cases":"startup_process and cli cases include process execution to exit; child allocations are excluded",
            "library_packs":8,"library_bytes":library_bytes,"open_first_frame_ns":open_first_frame_ns,
            "process_before_open":memory_before_open,"process_after_open":memory_after_open,
            "undo_32_retained_bytes":retained,"allocation_tracking":true,
            "process_ready":memory_ready,"process_before_history":memory_before_history,"process_after_history":memory_after_history}),
    );
}
