use super::*;
use file_icons_windows::{Settings, inspect_existing_icons, protection_report};

pub(super) fn verify(
    classes: &RegKey,
    pack: &file_icons_core::ImportedPack,
    dll: &Path,
    namespace: &str,
) -> Result<()> {
    let mut before = Snapshot::new();
    snapshot(classes, "", &mut before)?;
    write(classes, ".wfi-provider-b", "", "WfiProvider.B")?;
    write(classes, "WfiProvider.B\\DefaultIcon", "", "imageres.dll,2")?;
    write(classes, ".wfi-handler-source", "", "WfiHandler.Source")?;
    write(
        classes,
        "WfiHandler.Source\\shellex\\IconHandler",
        "",
        "{00000000-0000-0000-C000-000000000046}",
    )?;
    write(
        classes,
        ".wfi-handler-source\\DefaultIcon",
        "",
        "shell32.dll,2",
    )?;
    let mut pack = pack.clone();
    let icon = pack
        .theme
        .resolve("sample.wfi-known")
        .unwrap()
        .to_path_buf();
    pack.theme = file_icons_core::CompiledTheme::new(file_icons_core::ThemeRules {
        extensions: [
            "wfi-known",
            "wfi-shared",
            "wfi-provider-b",
            "wfi-link",
            "wfi-handler-source",
        ]
        .into_iter()
        .map(|e| (e.into(), icon.clone()))
        .collect(),
        ..Default::default()
    })?;
    let mut settings = Settings {
        packs: vec![pack.clone()],
        ..Default::default()
    };
    let sources = inspect_existing_icons(&pack, &settings)?;
    let dynamic = sources
        .iter()
        .find(|r| r.registration.extension == "wfi-handler-source")
        .unwrap();
    ensure!(
        dynamic.registration.reason == file_icons_windows::ProtectionReason::IconHandler
            && dynamic.location.is_none(),
        "static extension resource hid an existing dynamic handler"
    );
    let a = sources
        .iter()
        .find(|r| r.registration.extension == "wfi-known")
        .unwrap();
    let b = sources
        .iter()
        .find(|r| r.registration.extension == "wfi-provider-b")
        .unwrap();
    ensure!(
        a.provider_id.is_some() && a.provider_id != b.provider_id,
        "providers were conflated"
    );
    let original = tempfile::tempdir()?;
    file_icons_windows::native_preview::write_original_icon(
        a.location.as_ref().unwrap(),
        &original.path().join("before.png"),
    )?;
    settings.icon_protection_rules.set_provider(
        &sources,
        a.provider_id.as_ref().unwrap(),
        Some(false),
    );
    settings.icon_protection_rules.set_provider(
        &sources,
        b.provider_id.as_ref().unwrap(),
        Some(true),
    );
    let report = protection_report(&pack, &settings)?;
    ensure!(
        !report.iter().any(|r| r.extension == "wfi-known"),
        "provider exception not applied"
    );
    ensure!(
        report.iter().any(|r| r.extension == "wfi-provider-b"),
        "other provider protection lost"
    );
    ensure!(
        report.iter().any(|r| r.extension == "wfi-link"),
        "permanent protection lost"
    );
    let shared = sources
        .iter()
        .find(|r| r.registration.extension == "wfi-shared")
        .unwrap();
    settings
        .icon_protection_rules
        .set_extension(shared, Some(true));
    let root = file_icons_windows::home()?;
    let shared_file = root.join("original.wfi-shared");
    let known_file = root.join("pack.wfi-known");
    fs::write(&shared_file, "shared original")?;
    fs::write(&known_file, "known pack")?;
    let shared_image = icon_images(&shared_file)?;
    ensure!(
        !protection_report(&pack, &settings)?
            .iter()
            .any(|r| r.extension == "wfi-known"),
        "sibling protection overrode explicit pack permission"
    );
    file_icons_windows::save(&settings)?;
    let saved_rules = settings.icon_protection_rules.clone();
    settings = file_icons_windows::load()?;
    ensure!(
        settings.icon_protection_rules == saved_rules,
        "rules lost on disk"
    );
    file_icons_windows::apply(&mut settings, &pack.id, dll)?;
    ensure!(
        icon_images(&shared_file)? == shared_image,
        "protected sibling pixels changed"
    );
    ensure!(
        query(&known_file)?.0 == icon.to_string_lossy(),
        "allowed sibling did not use pack"
    );
    ensure!(
        fs::read_to_string(&shared_file)? == "shared original",
        "protected content changed"
    );
    ensure!(
        settings
            .journal
            .iter()
            .any(|c| c.key.to_lowercase().contains("wfiprobe.file")),
        "provider A was not registered"
    );
    ensure!(
        settings
            .journal
            .iter()
            .all(|c| !c.key.to_lowercase().contains("wfiprovider.b")),
        "provider B was overwritten"
    );
    let applied_sources = inspect_existing_icons(&pack, &settings)?;
    let applied_a = applied_sources
        .iter()
        .find(|r| r.registration.extension == "wfi-known")
        .unwrap();
    ensure!(
        applied_a.fingerprint == a.fingerprint,
        "own registration contaminated identity"
    );
    file_icons_windows::native_preview::write_original_icon(
        applied_a.location.as_ref().unwrap(),
        &original.path().join("during.png"),
    )?;
    ensure!(
        fs::read(original.path().join("before.png"))?
            == fs::read(original.path().join("during.png"))?,
        "preview used pack icon instead of baseline"
    );
    file_icons_windows::restore(&mut settings)?;
    ensure!(
        icon_images(&shared_file)? == shared_image,
        "restored sibling pixels changed"
    );

    // 同じクラスの片方をテーマ化しても、もう片方の専用処理へ委譲できる。
    write(classes, ".wfi-dynamic", "", "WfiProbe.Link")?;
    let dynamic_file = root.join("original.wfi-dynamic");
    fs::copy(root.join("app.lnk"), &dynamic_file)?;
    let dynamic_before = icon_images(&dynamic_file)?;
    let contents = fs::read(&dynamic_file)?;
    let mut dynamic_pack = pack.clone();
    let mut theme = dynamic_pack.theme.rules().clone();
    theme.extensions.insert("wfi-dynamic".into(), icon.clone());
    dynamic_pack.theme.replace_rules(theme)?;
    let dynamic_sources = inspect_existing_icons(&dynamic_pack, &settings)?;
    let allowed = dynamic_sources
        .iter()
        .find(|s| s.registration.extension == "wfi-link")
        .unwrap();
    let protected = dynamic_sources
        .iter()
        .find(|s| s.registration.extension == "wfi-dynamic")
        .unwrap();
    settings
        .icon_protection_rules
        .set_extension(allowed, Some(false));
    settings
        .icon_protection_rules
        .set_extension(protected, Some(true));
    settings.packs = vec![dynamic_pack.clone()];
    file_icons_windows::save(&settings)?;
    file_icons_windows::apply(&mut settings, &dynamic_pack.id, dll)?;
    ensure!(
        query(&root.join("special.wfi-link"))?.0 == icon.to_string_lossy(),
        "allowed file type sharing a dynamic handler did not use the pack"
    );
    ensure!(
        icon_images(&dynamic_file)? == dynamic_before,
        "original dynamic handler was not delegated"
    );
    ensure!(
        fs::read(&dynamic_file)? == contents,
        "dynamic file contents changed"
    );
    file_icons_windows::restore(&mut settings)?;
    // Shellは同一プロセス内に抽出済み画像を保持する。復元後は、DLLも画像も未読の
    // 別プロセスから、変更していないネイティブクラスの同じ内容と比較する。
    let restored = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "isolated_shell_registration_probe",
            "--nocapture",
        ])
        .env("WFI_SHARED_RESTORED_READER", namespace)
        .test_output()?;
    ensure!(
        restored.status.success(),
        "restored icon reader failed: {} {}",
        String::from_utf8_lossy(&restored.stdout),
        String::from_utf8_lossy(&restored.stderr)
    );
    settings.packs = vec![pack.clone()];
    settings.icon_protection_rules.set_extension(allowed, None);
    settings
        .icon_protection_rules
        .set_extension(protected, None);
    classes.delete_subkey(".wfi-dynamic")?;
    let old = classes
        .open_subkey("WfiProbe.File\\DefaultIcon")?
        .get_raw_value("")?;
    write(classes, "WfiProbe.File\\DefaultIcon", "", "shell32.dll,77")?;
    let changed = inspect_existing_icons(&pack, &settings)?;
    let changed_a = changed
        .iter()
        .find(|r| r.registration.extension == "wfi-known")
        .unwrap();
    ensure!(
        changed_a.needs_review && changed_a.protect,
        "changed resource inherited overwrite permission"
    );
    classes
        .open_subkey_with_flags("WfiProbe.File\\DefaultIcon", KEY_WRITE)?
        .delete_value("")?;
    let removed = inspect_existing_icons(&pack, &settings)?;
    ensure!(
        removed
            .iter()
            .any(|r| r.registration.extension == "wfi-known" && r.needs_review && r.protect),
        "removed resource lost the review requirement"
    );
    ensure!(
        protection_report(&pack, &settings)?
            .iter()
            .any(|r| r.extension == "wfi-known"),
        "removed resource inherited overwrite permission"
    );
    classes
        .open_subkey_with_flags("WfiProbe.File\\DefaultIcon", KEY_WRITE)?
        .set_raw_value("", &old)?;
    classes.delete_subkey_all("WfiHandler.Source")?;
    classes.delete_subkey_all(".wfi-handler-source")?;
    classes.delete_subkey_all("WfiProvider.B")?;
    classes.delete_subkey(".wfi-provider-b")?;
    let mut after = Snapshot::new();
    snapshot(classes, "", &mut after)?;
    ensure!(
        before == after,
        "granular protection did not restore registrations"
    );
    settings.icon_protection_rules = Default::default();
    file_icons_windows::save(&settings)?;
    Ok(())
}
