use super::*;
use file_icons_windows::{ProtectionReason, Settings};

pub(super) fn verify(
    root: &Path,
    classes: &RegKey,
    pack: &file_icons_core::ImportedPack,
    dll: &Path,
) -> Result<()> {
    let mut protected_pack = pack.clone();
    let icon = pack
        .theme
        .resolve("normal.wfi-known")
        .unwrap()
        .to_path_buf();
    protected_pack.theme = file_icons_core::CompiledTheme::new(file_icons_core::ThemeRules {
        extensions: std::collections::BTreeMap::from([
            ("wfi-known".into(), icon.clone()),
            ("wfi-shared".into(), icon.clone()),
            ("wfi-link".into(), icon),
        ]),
        ..Default::default()
    })?;
    let pack = &protected_pack;
    // クラスの画像、共有クラス、専用ハンドラーを、実DLLと隔離レジストリで検証する。
    let mut before = Snapshot::new();
    snapshot(classes, "", &mut before)?;
    let probe = root.join("normal.wfi-known");
    fs::write(&probe, "protected contents")?;
    let original = icon_images(&probe)?;
    let mut settings = Settings {
        packs: vec![pack.clone()],
        ..Default::default()
    };
    file_icons_windows::save(&settings)?;
    let plan = file_icons_windows::plan(pack, dll)?;
    ensure!(
        plan.changes.is_empty(),
        "existing icon registrations were not excluded"
    );
    ensure!(
        plan.icon_exclusions
            .iter()
            .any(|e| e.extension == "wfi-known" && e.reason == ProtectionReason::DefaultIcon),
        "class icon reason missing"
    );
    ensure!(
        plan.icon_exclusions
            .iter()
            .any(|e| e.extension == "wfi-link" && e.reason == ProtectionReason::IconHandler),
        "handler reason missing"
    );
    file_icons_windows::apply(&mut settings, &pack.id, dll)?;
    ensure!(
        settings.journal.is_empty() && settings.active.as_ref() == Some(&pack.id),
        "all-protected pack state is invalid"
    );
    ensure!(
        icon_images(&probe)? == original && fs::read_to_string(&probe)? == "protected contents",
        "protected icon or content changed"
    );
    file_icons_windows::restore(&mut settings)?;

    // 拡張子自身の指定は、共有する別の種類へ保護を広げない。
    let existing = classes
        .open_subkey("WfiProbe.File\\DefaultIcon")?
        .get_raw_value("")?;
    classes
        .open_subkey_with_flags("WfiProbe.File\\DefaultIcon", KEY_WRITE)?
        .delete_value("")?;
    write(
        classes,
        ".wfi-known\\DefaultIcon",
        "",
        &format!("{}\\System32\\shell32.dll,4", std::env::var("SystemRoot")?),
    )?;
    let report = file_icons_windows::protection_report(pack, &settings)?;
    ensure!(
        report
            .iter()
            .any(|e| e.extension == "wfi-known" && e.source == ".wfi-known\\DefaultIcon"),
        "extension icon was ignored"
    );
    ensure!(
        !report.iter().any(|e| e.extension == "wfi-shared"),
        "extension protection leaked to a sibling"
    );
    // まだ照会していない種類で比較し、同一プロセスの関連付けキャッシュを混ぜない。
    write(classes, ".wfi-own", "", "WfiProbe.Own")?;
    write(classes, ".wfi-own-peer", "", "WfiProbe.Own")?;
    write(
        classes,
        ".wfi-own\\DefaultIcon",
        "",
        &format!("{}\\System32\\shell32.dll,4", std::env::var("SystemRoot")?),
    )?;
    let mut own_pack = pack.clone();
    let mut theme = own_pack.theme.rules().clone();
    let icon = theme.extensions["wfi-known"].clone();
    theme.extensions.insert("wfi-own".into(), icon.clone());
    theme.extensions.insert("wfi-own-peer".into(), icon);
    own_pack.theme.replace_rules(theme)?;
    settings.packs = vec![own_pack.clone()];
    let own_file = root.join("original.wfi-own");
    let peer_file = root.join("pack.wfi-own-peer");
    fs::write(&own_file, "own original")?;
    fs::write(&peer_file, "peer contents")?;
    file_icons_windows::notify();
    let own_location = query(&own_file)?;
    let own_original = icon_images(&own_file)?;
    file_icons_windows::apply(&mut settings, &own_pack.id, dll)?;
    ensure!(
        icon_images(&own_file)? == own_original,
        "extension-specific original icon changed: before={own_location:?}, after={:?}",
        query(&own_file)?
    );
    let peer_location = query(&peer_file)?;
    let expected = own_pack.theme.resolve("pack.wfi-own-peer").unwrap();
    ensure!(
        peer_location.0 == expected.to_string_lossy(),
        "unprotected sibling did not use pack icon: actual={peer_location:?}, expected={expected:?}"
    );
    file_icons_windows::restore(&mut settings)?;
    settings.packs = vec![pack.clone()];
    classes.delete_subkey_all(".wfi-own")?;
    classes.delete_subkey(".wfi-own-peer")?;
    // 復元で作成した空クラスも削除される。
    classes.delete_subkey(".wfi-known\\DefaultIcon")?;
    write(classes, ".wfi-outside", "", "WfiProbe.File")?;
    write(classes, ".wfi-outside\\DefaultIcon", "", "shell32.dll,5")?;
    let outside = file_icons_windows::protection_report(pack, &settings)?;
    ensure!(
        !outside.iter().any(|e| e.extension == "wfi-known"),
        "outside extension protection leaked into the pack"
    );
    classes.delete_subkey(".wfi-outside\\DefaultIcon")?;
    classes.delete_subkey(".wfi-outside")?;
    classes
        .open_subkey_with_flags("WfiProbe.File\\DefaultIcon", KEY_WRITE)?
        .set_raw_value("", &existing)?;

    // テーマ適用済みでも、自分の登録ではなく記録済みの適用前の値で判定する。
    settings.preserve_existing_icons = false;
    file_icons_windows::save(&settings)?;
    file_icons_windows::apply(&mut settings, &pack.id, dll)?;
    settings.preserve_existing_icons = true;
    let report = file_icons_windows::protection_report(pack, &settings)?;
    ensure!(
        report
            .iter()
            .any(|e| e.extension == "wfi-known" && e.reason == ProtectionReason::DefaultIcon),
        "original registration was not used"
    );
    file_icons_windows::activate_options(&mut settings, &pack.id, dll, None, Some(true))?;
    ensure!(
        settings.preserve_existing_icons && settings.journal.is_empty(),
        "enabling protection did not restore old registrations"
    );
    ensure!(
        icon_images(&probe)? == original,
        "enabling protection did not restore original icon pixels"
    );
    let mut after = Snapshot::new();
    snapshot(classes, "", &mut after)?;
    ensure!(
        before == after,
        "protection did not restore the complete registry"
    );
    file_icons_windows::restore(&mut settings)?;
    // 元の指定がない種類は、自前の登録を見つけても保護対象へ誤分類しない。
    write(classes, ".wfi-fresh", "", "WfiProbe.Fresh")?;
    let mut fresh = pack.clone();
    let mut rules = fresh.theme.rules().clone();
    rules
        .extensions
        .insert("wfi-fresh".into(), rules.extensions["wfi-known"].clone());
    fresh.theme.replace_rules(rules)?;
    settings.packs = vec![fresh.clone()];
    settings.preserve_existing_icons = false;
    file_icons_windows::save(&settings)?;
    file_icons_windows::apply(&mut settings, &fresh.id, dll)?;
    settings.preserve_existing_icons = true;
    let mixed = file_icons_windows::protection_report(&fresh, &settings)?;
    ensure!(
        !mixed.iter().any(|e| e.extension == "wfi-fresh"),
        "our own registration was misclassified as an existing icon"
    );
    file_icons_windows::activate_options(&mut settings, &fresh.id, dll, None, Some(true))?;
    ensure!(
        !settings.journal.is_empty(),
        "unregistered types were excluded"
    );
    ensure!(
        settings
            .journal
            .iter()
            .all(|c| !c.key.contains("WfiProbe.File")),
        "shared protected class was registered"
    );
    file_icons_windows::restore(&mut settings)?;
    classes.delete_subkey(".wfi-fresh")?;
    let mut final_state = Snapshot::new();
    snapshot(classes, "", &mut final_state)?;
    ensure!(
        before == final_state,
        "mixed protection did not restore original registry"
    );
    Ok(())
}
