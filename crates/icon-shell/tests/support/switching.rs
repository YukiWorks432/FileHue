use super::{Snapshot, icon_images, query, snapshot, write};
use anyhow::{Result, ensure};
use file_icons_core::ImportedPack;
use file_icons_windows::{PreviousPackRestored, Settings, activate, save};
use std::{fs, path::Path};
use windows::{
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, HLOCAL, LocalFree},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
        System::Registry::{HKEY, RegGetKeySecurity, RegSetKeySecurity},
    },
    core::w,
};
use winreg::{
    RegKey,
    enums::{KEY_ALL_ACCESS, KEY_SET_VALUE},
};

pub(super) struct WriteDenial {
    key: RegKey,
    original: Vec<u32>,
}
impl Drop for WriteDenial {
    fn drop(&mut self) {
        unsafe {
            let _ = RegSetKeySecurity(
                HKEY(self.key.raw_handle()),
                DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR(self.original.as_mut_ptr().cast()),
            );
        }
    }
}

pub(super) fn prepare(classes: &RegKey, original_icon: &str) -> Result<WriteDenial> {
    write(
        classes,
        ".wfi-switch-conflict",
        "",
        "WfiProbe.SwitchConflict",
    )?;
    write(
        classes,
        "WfiProbe.SwitchConflict\\shellex\\IconHandler",
        "",
        "not-a-guid",
    )?;
    write(classes, ".wfi-switch-denied", "", "WfiProbe.SwitchDenied")?;
    write(
        classes,
        "WfiProbe.SwitchDenied\\DefaultIcon",
        "",
        original_icon,
    )?;
    let key =
        classes.open_subkey_with_flags("WfiProbe.SwitchDenied\\DefaultIcon", KEY_ALL_ACCESS)?;
    let mut length = 0u32;
    let result = unsafe {
        RegGetKeySecurity(
            HKEY(key.raw_handle()),
            DACL_SECURITY_INFORMATION,
            None,
            &mut length,
        )
    };
    ensure!(
        result == ERROR_INSUFFICIENT_BUFFER && length > 0,
        "cannot size fixture security descriptor"
    );
    let mut original = vec![0u32; length.div_ceil(4) as usize];
    unsafe {
        RegGetKeySecurity(
            HKEY(key.raw_handle()),
            DACL_SECURITY_INFORMATION,
            Some(PSECURITY_DESCRIPTOR(original.as_mut_ptr().cast())),
            &mut length,
        )
        .ok()?;
    }
    let guard = WriteDenial { key, original };
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        // この子プロセス専用キーだけで、値の書き込みを拒否する。読み取りと削除は保つ。
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:(D;;0x0002;;;WD)(A;;KA;;;OW)"),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let result = RegSetKeySecurity(
            HKEY(guard.key.raw_handle()),
            DACL_SECURITY_INFORMATION,
            descriptor,
        );
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result.ok()?;
    }
    ensure!(
        classes
            .open_subkey_with_flags("WfiProbe.SwitchDenied\\DefaultIcon", KEY_SET_VALUE)
            .is_err(),
        "test key did not deny writes"
    );
    Ok(guard)
}

pub(super) fn verify(
    root: &Path,
    classes: &RegKey,
    settings: &mut Settings,
    pack: &ImportedPack,
    dll: &Path,
) -> Result<()> {
    let normal = pack
        .theme
        .resolve("normal.wfi-known")
        .unwrap()
        .to_path_buf();
    let special = pack
        .theme
        .resolve("special.wfi-known")
        .unwrap()
        .to_path_buf();
    let mut second = pack.clone();
    second.id.push_str("/second");
    let mut rules = second.theme.rules().clone();
    for path in rules
        .files
        .values_mut()
        .chain(rules.extensions.values_mut())
    {
        *path = if path == &normal {
            special.clone()
        } else {
            normal.clone()
        };
    }
    rules
        .extensions
        .insert("wfi-second-only".into(), special.clone());
    second.theme.replace_rules(rules)?;
    let mut broken = second.clone();
    broken.id.push_str("/broken");
    let mut rules = broken.theme.rules().clone();
    rules
        .extensions
        .insert("wfi-switch-conflict".into(), special);
    broken.theme.replace_rules(rules)?;
    let mut denied = second.clone();
    denied.id.push_str("/denied");
    let mut rules = denied.theme.rules().clone();
    rules
        .extensions
        .insert("wfi-switch-denied".into(), normal.clone());
    denied.theme.replace_rules(rules)?;
    settings
        .packs
        .extend([second.clone(), broken.clone(), denied.clone()]);
    save(settings)?;
    let mut original_registration = Snapshot::new();
    snapshot(classes, "", &mut original_registration)?;
    let original_settings = fs::read(root.join("settings.json"))?;
    let original_recovery = fs::read(root.join("recovery.json"))?;
    let original_images = icon_images(&root.join("normal.wfi-known"))?;
    // 入力エラーは現在の設定・復元記録・登録を変えない。
    for (id, source) in [
        ("unknown-pack", dll.to_path_buf()),
        (second.id.as_str(), root.join("missing.dll")),
    ] {
        ensure!(
            activate(settings, id, &source).is_err(),
            "invalid switch unexpectedly succeeded"
        );
        let mut unchanged = Snapshot::new();
        snapshot(classes, "", &mut unchanged)?;
        ensure!(
            unchanged == original_registration,
            "input failure changed registration"
        );
        ensure!(
            fs::read(root.join("settings.json"))? == original_settings,
            "input failure changed preferences"
        );
        ensure!(
            fs::read(root.join("recovery.json"))? == original_recovery,
            "input failure changed recovery record"
        );
    }
    // 他のアプリが変更した値を見つけたら、解除自体を開始しない。
    write(
        classes,
        "WfiProbe.File\\DefaultIcon",
        "",
        "external-test-value",
    )?;
    let mut external = Snapshot::new();
    snapshot(classes, "", &mut external)?;
    ensure!(
        activate(settings, &second.id, dll).is_err(),
        "external change ignored during switch"
    );
    let mut after_rejected = Snapshot::new();
    snapshot(classes, "", &mut after_rejected)?;
    ensure!(
        external == after_rejected,
        "switch overwrote external changes"
    );
    write(classes, "WfiProbe.File\\DefaultIcon", "", "%1")?;
    // 古い登録の解除後に新しい計画が失敗しても、元のパックへ戻る。
    for candidate in [&broken, &denied] {
        let error = activate(settings, &candidate.id, dll).expect_err("invalid handler accepted");
        ensure!(
            error.is::<PreviousPackRestored>(),
            "previous pack was not restored: {error:#}"
        );
        ensure!(
            settings.active.as_ref() == Some(&pack.id),
            "failed switch left wrong active pack"
        );
        ensure!(
            icon_images(&root.join("normal.wfi-known"))? == original_images,
            "failed switch changed old icon"
        );
        let mut rolled_back = Snapshot::new();
        snapshot(classes, "", &mut rolled_back)?;
        ensure!(
            rolled_back == original_registration,
            "failed switch changed previous registration"
        );
        ensure!(
            fs::read(root.join("settings.json"))? == original_settings,
            "failed switch changed previous preferences"
        );
        ensure!(
            fs::read(root.join("recovery.json"))? == original_recovery,
            "failed switch changed original recovery record"
        );
    }
    // 成功時は実際のアイコンが切り替わり、追加の拡張子にも適用する。
    let additional = root.join("additional.wfi-second-only");
    fs::write(&additional, "switch-preserves-content")?;
    activate(settings, &second.id, dll)?;
    ensure!(
        settings.active.as_ref() == Some(&second.id),
        "wrong selected pack after switch"
    );
    for (name, bgr) in [
        ("normal.wfi-known", [255, 0, 0]),
        ("special.wfi-known", [0, 0, 255]),
        ("additional.wfi-second-only", [255, 0, 0]),
    ] {
        let path = root.join(name);
        ensure!(
            query(&path)?.0 == second.theme.resolve(name).unwrap().to_string_lossy(),
            "wrong icon location after switch"
        );
        let (small, large) = icon_images(&path)?;
        for (image, size) in [(small, 16), (large, 32)] {
            let center = (size / 2 * size + size / 2) * 4;
            ensure!(
                image[center..center + 3] == bgr,
                "wrong icon color after switch: {name}, size={size}"
            );
        }
    }
    ensure!(
        fs::read_to_string(&additional)? == "switch-preserves-content",
        "switch changed contents"
    );
    activate(settings, &pack.id, dll)?;
    let mut returned = Snapshot::new();
    snapshot(classes, "", &mut returned)?;
    ensure!(
        returned == original_registration,
        "switch back retained obsolete registration"
    );
    ensure!(
        icon_images(&root.join("normal.wfi-known"))? == original_images,
        "old icon not restored after switching back"
    );
    Ok(())
}
