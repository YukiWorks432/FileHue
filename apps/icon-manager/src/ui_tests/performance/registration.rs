//! 調査と登録計画を、多数クラス・共有クラス・パック外の種類で比較する。
use super::*;
use std::hash::{Hash, Hasher};

pub(super) fn measure(
    registry: &crate::ui_tests::IsolatedRegistryProfile,
) -> Vec<serde_json::Value> {
    let template = load().unwrap().packs.remove(0);
    let icon = template.theme.resolve("sample.json").unwrap().to_path_buf();
    let dll = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/shell32.dll");
    let mut cases = Vec::new();
    for (name, class_count, outside) in
        [("distinct", 400, 0), ("shared", 4, 0), ("outside", 4, 400)]
    {
        let mut pack = template.clone();
        let mut rules = ThemeRules::default();
        for index in 0..400 + outside {
            let extension = format!("bench-{name}-{index:04}");
            let class_index = if class_count == 400 {
                index
            } else {
                index / 2 % class_count
            };
            let class = format!("WfiPerformance.{name}.{class_index}");
            if index < 400 {
                rules.extensions.insert(extension.clone(), icon.clone());
            }
            for root in [&registry._classes, &registry._effective] {
                root.create_subkey(format!(".{extension}"))
                    .unwrap()
                    .0
                    .set_value("", &class)
                    .unwrap();
                root.create_subkey(&class).unwrap();
                if class_count == 400 {
                    root.create_subkey(format!("{class}\\DefaultIcon"))
                        .unwrap()
                        .0
                        .set_value("", &"shell32.dll,2")
                        .unwrap();
                } else if index % 2 == 0 || index >= 400 {
                    // 静的画像と専用ハンドラーを交互に持つ。奇数のパック対象は許可する。
                    let (suffix, value) = if index % 4 == 0 {
                        ("DefaultIcon", "shell32.dll,3")
                    } else {
                        (
                            "shellex\\IconHandler",
                            "{00021401-0000-0000-C000-000000000046}",
                        )
                    };
                    root.create_subkey(format!(".{extension}\\{suffix}"))
                        .unwrap()
                        .0
                        .set_value("", &value)
                        .unwrap();
                }
            }
        }
        pack.theme = CompiledTheme::new(rules).unwrap();
        let settings = file_icons_windows::Settings::default();
        let report = file_icons_windows::protection_report(&pack, &settings).unwrap();
        assert_eq!(
            report.len(),
            if class_count == 400 {
                400
            } else {
                200 + outside
            }
        );
        let expected = signature(
            &file_icons_windows::plan_with_protection(&pack, &dll, class_count != 400).unwrap(),
        );
        let mut case = super::measure(&format!("registry_plan_{name}_400"), || {
            let plan =
                file_icons_windows::plan_with_protection(&pack, &dll, class_count != 400).unwrap();
            assert_eq!(
                plan.changes.len(),
                if class_count == 400 { 802 } else { 10 }
            );
            let actual = signature(&plan);
            assert_eq!(actual, expected);
            actual
        });
        case["fixture"] = serde_json::json!({"pack_extensions":400,"classes":class_count,"outside_extensions":outside,
            "protected_rows":report.len(),"preserve_existing_icons":class_count != 400});
        cases.push(case);
    }
    cases
}

fn signature(plan: &file_icons_windows::RegistrationPlan) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    // 確保を伴うJSON化を計時へ加えず、登録値・退避・除外・作成キーを照合する。
    for change in &plan.changes {
        change.key.hash(&mut hash);
        change.name.hash(&mut hash);
        change
            .before
            .as_ref()
            .map(|v| (v.kind, &v.bytes))
            .hash(&mut hash);
        change.after.kind.hash(&mut hash);
        change.after.bytes.hash(&mut hash);
    }
    for row in &plan.icon_exclusions {
        (&row.extension, &row.class, &row.source).hash(&mut hash);
        std::mem::discriminant(&row.reason).hash(&mut hash);
    }
    for (class, location) in &plan.fallbacks {
        (class, &location.path, location.index).hash(&mut hash);
    }
    plan.original_handlers.hash(&mut hash);
    plan.created_keys.hash(&mut hash);
    hash.finish()
}
