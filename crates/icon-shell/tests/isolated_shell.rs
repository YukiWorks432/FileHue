//! すべての登録を子プロセス専用キーへ振り向け、実ユーザーの関連付けを保つ。
#[path = "../../../test-support/process.rs"]
mod test_process;
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use test_process::CommandOutput;
use windows::{
    Win32::{
        Foundation::HANDLE,
        Graphics::Gdi::*,
        Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
        System::{
            ApplicationInstallationAndServicing::*,
            Com::*,
            Registry::{
                HKEY, HKEY_CLASSES_ROOT as HKCR, HKEY_CURRENT_USER as HKCU, RegOverridePredefKey,
            },
        },
        UI::{
            Shell::*,
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx},
        },
    },
    core::{Interface, PCWSTR},
};
use winreg::{RegKey, enums::*};

#[path = "support/casing.rs"]
mod casing;
#[path = "support/icon_policy.rs"]
mod icon_policy;
#[path = "support/protection.rs"]
mod protection;
#[path = "support/switching.rs"]
mod switching;

struct Redirect {
    physical: RegKey,
    name: String,
    owned: bool,
}
impl Redirect {
    fn new() -> Result<(Self, RegKey)> {
        // 疑似ハンドルとは別に、変更前の実ハンドルを保持する。
        let physical =
            RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("", KEY_ALL_ACCESS)?;
        let name = format!(
            "Software\\WindowsFileIconsDev\\Tests\\{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        ensure!(
            physical.open_subkey(&name).is_err(),
            "test key already exists"
        );
        eprintln!("試験用HKCUキー: {name}");
        let (user, _) = physical.create_subkey(&name)?;
        let (classes, _) = user.create_subkey("Software\\Classes")?;
        unsafe {
            RegOverridePredefKey(HKCU, Some(HKEY(user.raw_handle()))).ok()?;
        }
        let guard = Self {
            physical,
            name,
            owned: true,
        };
        unsafe {
            RegOverridePredefKey(HKCR, Some(HKEY(classes.raw_handle()))).ok()?;
        }
        Ok((guard, classes))
    }
}
impl Redirect {
    fn attach(name: String) -> Result<Self> {
        ensure!(
            name.starts_with("Software\\WindowsFileIconsDev\\Tests\\"),
            "invalid test namespace"
        );
        let physical =
            RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("", KEY_ALL_ACCESS)?;
        let user = physical.open_subkey(&name)?;
        let classes = user.open_subkey("Software\\Classes")?;
        unsafe {
            RegOverridePredefKey(HKCU, Some(HKEY(user.raw_handle()))).ok()?;
        }
        let guard = Self {
            physical,
            name,
            owned: false,
        };
        unsafe {
            RegOverridePredefKey(HKCR, Some(HKEY(classes.raw_handle()))).ok()?;
        }
        Ok(guard)
    }
}

impl Drop for Redirect {
    fn drop(&mut self) {
        unsafe {
            let _ = RegOverridePredefKey(HKCR, None);
            let _ = RegOverridePredefKey(HKCU, None);
        }
        // このプロセスが新規作成した専用キーだけを除去する。
        if self.owned {
            let _ = self.physical.delete_subkey_all(&self.name);
        }
    }
}

fn write(classes: &RegKey, key: &str, name: &str, value: &str) -> Result<()> {
    classes.create_subkey(key)?.0.set_value(name, &value)?;
    Ok(())
}

struct Activation {
    handle: HANDLE,
    cookie: usize,
}
impl Activation {
    fn new(root: &Path, dll: &Path) -> Result<Self> {
        if root.canonicalize()?.join("file_icons_shell.dll") != dll.canonicalize()? {
            fs::copy(dll, root.join("file_icons_shell.dll"))?;
        }
        let manifest = root.join("probe.manifest");
        fs::write(
            &manifest,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><assemblyIdentity type="win32" name="FileIcons.IsolatedProbe" version="1.0.0.0"/><file name="file_icons_shell.dll"><comClass clsid="{}" threadingModel="Apartment"/></file></assembly>"#,
                file_icons_windows::CLSID_TEXT
            ),
        )?;
        let path = manifest
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let context = ACTCTXW {
            cbSize: std::mem::size_of::<ACTCTXW>() as u32,
            lpSource: PCWSTR(path.as_ptr()),
            ..Default::default()
        };
        let handle = unsafe { CreateActCtxW(&context)? };
        let mut cookie = 0;
        if let Err(error) = unsafe { ActivateActCtx(Some(handle), &mut cookie) } {
            unsafe {
                ReleaseActCtx(handle);
            }
            return Err(error.into());
        }
        Ok(Self { handle, cookie })
    }
}
impl Drop for Activation {
    fn drop(&mut self) {
        unsafe {
            let _ = DeactivateActCtx(0, self.cookie);
            ReleaseActCtx(self.handle);
        }
    }
}

fn query(path: &Path) -> Result<(String, i32)> {
    let wide = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICONLOCATION,
        )
    };
    ensure!(result != 0, "SHGetFileInfo failed: {}", path.display());
    let end = info
        .szDisplayName
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(info.szDisplayName.len());
    Ok((String::from_utf16(&info.szDisplayName[..end])?, info.iIcon))
}

fn direct_query(handler: &IPersistFile, path: &Path) -> Result<(String, i32)> {
    let input = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut output = [0u16; 4096];
    let (mut index, mut flags) = (0, 0);
    unsafe {
        handler.Load(PCWSTR(input.as_ptr()), STGM_READ)?;
        handler
            .cast::<IExtractIconW>()?
            .GetIconLocation(0, &mut output, &mut index, &mut flags)?;
    }
    let end = output
        .iter()
        .position(|value| *value == 0)
        .context("unterminated icon location")?;
    Ok((String::from_utf16(&output[..end])?, index))
}

type Snapshot = std::collections::BTreeMap<String, Vec<(String, u32, Vec<u8>)>>;
fn snapshot(key: &RegKey, prefix: &str, output: &mut Snapshot) -> Result<()> {
    let mut values = key
        .enum_values()
        .map(|item| item.map(|(name, value)| (name, value.vtype as u32, value.bytes.into_owned())))
        .collect::<std::io::Result<Vec<_>>>()?;
    values.sort();
    output.insert(prefix.into(), values);
    for name in key.enum_keys() {
        let name = name?;
        snapshot(
            &key.open_subkey(&name)?,
            &format!("{prefix}\\{name}"),
            output,
        )?;
    }
    Ok(())
}

fn pixels(path: &Path, small: bool) -> Result<Vec<u8>> {
    let wide = path
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut info = SHFILEINFOW::default();
    let flags = SHGFI_ICON
        | if small {
            SHGFI_SMALLICON
        } else {
            SHGFI_LARGEICON
        };
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    ensure!(
        result != 0 && !info.hIcon.is_invalid(),
        "Shell did not return an icon"
    );
    let dc = unsafe { CreateCompatibleDC(None) };
    let result = (|| -> Result<Vec<u8>> {
        ensure!(!dc.is_invalid(), "cannot create drawing context");
        let size = if small { 16 } else { 32 };
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = unsafe {
            CreateDIBSection(Some(dc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)?
        };
        let original = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        let result = (|| -> Result<Vec<u8>> {
            ensure!(!bits.is_null(), "bitmap memory missing");
            let bytes = unsafe {
                std::slice::from_raw_parts_mut(bits.cast::<u8>(), (size * size * 4) as usize)
            };
            for pixel in bytes.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&[17, 34, 51, 255]);
            }
            unsafe {
                DrawIconEx(dc, 0, 0, info.hIcon, size, size, 0, None, DI_NORMAL)?;
                GdiFlush().ok()?;
            }
            Ok(bytes.to_vec())
        })();
        unsafe {
            let _ = SelectObject(dc, original);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        result
    })();
    unsafe {
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(info.hIcon);
    }
    result
}

fn icon_images(path: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
    Ok((pixels(path, true)?, pixels(path, false)?))
}

fn fixture(root: &Path) -> Result<file_icons_core::ImportedPack> {
    let extension = root.join("extension");
    fs::create_dir_all(&extension)?;
    fs::write(
        extension.join("package.json"),
        r#"{"name":"probe","publisher":"tests","version":"1","contributes":{"iconThemes":[{"id":"probe","label":"Probe","path":"theme.json"}]}}"#,
    )?;
    fs::write(
        extension.join("theme.json"),
        r#"{"iconDefinitions":{"normal":{"iconPath":"normal.svg"},"special":{"iconPath":"special.svg"}},"fileNames":{"special.wfi-known":"special","special.wfi-unknown":"special","Dockerfile":"special","special.wfi-link":"special"},"fileExtensions":{"wfi-known":"normal","wfi-unknown":"normal"}}"#,
    )?;
    fs::write(
        extension.join("normal.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##,
    )?;
    fs::write(
        extension.join("special.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#0000ff"/></svg>"##,
    )?;
    file_icons_core::import(&extension, "probe", &root.join("packs"))
}

/// Shellからの呼び出しと、別プロセスの復元時の読み込みを両実行方式で照合する。
fn initialize_com() -> Result<()> {
    let apartment = match std::env::var("WFI_PROBE_APARTMENT").ok().as_deref() {
        Some("mta") => COINIT_MULTITHREADED,
        Some("sta") | None => COINIT_APARTMENTTHREADED,
        _ => anyhow::bail!("unknown probe apartment"),
    };
    unsafe { CoInitializeEx(None, apartment).ok()? };
    Ok(())
}

/// DLLと設定キャッシュを一度も読み込んでいない、別プロセスで通常表示を確認する。
fn fresh_recovery_reader() -> Result<()> {
    let _redirect = Redirect::attach(std::env::var("WFI_RECOVERY_READER")?)?;
    initialize_com()?;
    let root = file_icons_windows::home()?;
    let reader = root.join("fresh-reader");
    fs::create_dir_all(&reader)?;
    let registered: String = RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!(
            "CLSID\\{}\\InprocServer32",
            file_icons_windows::CLSID_TEXT
        ))?
        .get_value("")?;
    let _activation = Activation::new(&reader, Path::new(&registered))?;
    let expected: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join("fresh-expected.json"))?)?;
    let fallback = file_icons_windows::SettingsCache::default().get(&root.join("settings.json"))?;
    ensure!(
        fallback.active.is_none() && !fallback.journal.is_empty(),
        "fresh reader lacks recovery state"
    );
    for (name, reference) in expected {
        ensure!(
            icon_images(&root.join(&name))? == icon_images(&root.join(&reference))?,
            "fresh process changed original icon: {name}; location={:?}",
            query(&root.join(&name))?
        );
    }
    unsafe {
        CoUninitialize();
    }
    Ok(())
}

#[test]
fn isolated_restore_scope() -> Result<()> {
    use file_icons_core::registry::{Change, Value};
    if std::env::var_os("WFI_SCOPE_PROBE").is_none() {
        let profile = tempfile::tempdir()?;
        let output = Command::new(std::env::current_exe()?)
            .args(["--exact", "isolated_restore_scope", "--nocapture"])
            .env("WFI_SCOPE_PROBE", "1")
            .env("LOCALAPPDATA", profile.path())
            .test_output()?;
        ensure!(
            output.status.success(),
            "scope probe failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let (_redirect, classes) = Redirect::new()?;
    let root = file_icons_windows::home()?;
    fs::create_dir_all(&root)?;
    let valid = Change {
        key: r"Software\Classes\.lnk\DefaultIcon".into(),
        name: String::new(),
        before: Some(Value {
            kind: 3,
            bytes: vec![9, 0, 255],
        }),
        after: Value {
            kind: 3,
            bytes: vec![1, 2, 3],
        },
    };
    classes
        .create_subkey(r".lnk\DefaultIcon")?
        .0
        .set_raw_value(
            "",
            &winreg::RegValue {
                vtype: REG_BINARY,
                bytes: valid.after.bytes.clone().into(),
            },
        )?;
    classes.create_subkey(r"Unrelated\Empty")?;
    let mut before = Snapshot::new();
    snapshot(&classes, "", &mut before)?;
    let mut invalid = Vec::new();
    for (key, name) in [
        (r"Software\Classes\Demo\shell\open\command", ""),
        (r"Software\Classes\CLSID\{OTHER}\InprocServer32", ""),
        (r"Software\Classes\Demo\DefaultIcon", "command"),
        (r"Software\Classes\clsid\DefaultIcon", ""),
    ] {
        let mut change = valid.clone();
        change.key = key.into();
        change.name = name.into();
        invalid.push((vec![valid.clone(), change], Vec::new()));
    }
    let duplicate = valid.clone();
    invalid.push((vec![valid.clone(), duplicate], Vec::new()));
    invalid.push((
        vec![valid.clone()],
        vec![r"Software\Classes\Unrelated\Empty".into()],
    ));
    for (journal, created_keys) in invalid {
        let mut settings = file_icons_windows::Settings {
            journal,
            created_keys,
            ..Default::default()
        };
        file_icons_windows::save(&settings)?;
        let settings_bytes = fs::read(root.join("settings.json"))?;
        let library_bytes = fs::read(root.join("settings.library.json"))?;
        // 先頭の有効な項目も含めて、変更前に全件を拒否する。
        ensure!(
            file_icons_windows::restore(&mut settings).is_err(),
            "direct restore accepted invalid scope"
        );
        ensure!(
            file_icons_windows::restore_registration().is_err(),
            "normal restore accepted invalid scope"
        );
        fs::write(
            root.join("recovery.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "active": null, "fallbacks": {}, "original_handlers": {},
                "journal": settings.journal, "created_keys": settings.created_keys,
            }))?,
        )?;
        let recovery_bytes = fs::read(root.join("recovery.json"))?;
        ensure!(
            file_icons_windows::restore_registration().is_err(),
            "independent restore accepted invalid scope"
        );
        ensure!(
            fs::read(root.join("settings.json"))? == settings_bytes,
            "rejected restore changed settings"
        );
        ensure!(
            fs::read(root.join("settings.library.json"))? == library_bytes,
            "rejected restore changed library"
        );
        fs::write(root.join("settings.json"), b"broken")?;
        ensure!(
            file_icons_windows::restore_registration().is_err(),
            "damaged restore accepted invalid scope"
        );
        ensure!(
            fs::read(root.join("settings.json"))? == b"broken",
            "rejected damaged restore changed original"
        );
        ensure!(
            fs::read(root.join("recovery.json"))? == recovery_bytes,
            "rejected restore changed recovery"
        );
        let mut after = Snapshot::new();
        snapshot(&classes, "", &mut after)?;
        ensure!(after == before, "rejected restore changed registry");
        fs::remove_file(root.join("recovery.json"))?;
        fs::write(root.join("settings.json"), &settings_bytes)?;
    }
    // 新規適用の除外対象でも、旧登録は生データを保持して解除する。
    let mut settings = file_icons_windows::Settings {
        journal: vec![valid.clone()],
        ..Default::default()
    };
    file_icons_windows::save(&settings)?;
    file_icons_windows::restore(&mut settings)?;
    let raw = classes
        .open_subkey(r".lnk\DefaultIcon")?
        .get_raw_value("")?;
    ensure!(
        raw.vtype == REG_BINARY && raw.bytes.as_ref() == [9, 0, 255],
        "legacy restore lost raw value"
    );
    // 退避に失敗した場合は、正常な復元記録があっても登録へ進まない。
    let pending = file_icons_windows::Settings {
        journal: vec![valid.clone()],
        ..Default::default()
    };
    file_icons_windows::save(&pending)?;
    classes
        .open_subkey_with_flags(r".lnk\DefaultIcon", KEY_ALL_ACCESS)?
        .set_raw_value(
            "",
            &winreg::RegValue {
                vtype: REG_BINARY,
                bytes: valid.after.bytes.clone().into(),
            },
        )?;
    let recovery = serde_json::to_vec(&serde_json::json!({
        "version":1,"active":null,"fallbacks":{},"original_handlers":{},
        "journal":pending.journal,"created_keys":[],
    }))?;
    fs::write(root.join("recovery.json"), &recovery)?;
    let runtime = fs::read(root.join("settings.json"))?;
    let library = fs::read(root.join("settings.library.json"))?;
    let mut pending_registry = Snapshot::new();
    snapshot(&classes, "", &mut pending_registry)?;
    let backups_before = fs::read_dir(&root)?.count();
    for name in ["settings.json", "settings.library.json"] {
        let original = fs::File::create(root.join(name))?;
        original.set_len(16 * 1024 * 1024 + 1)?;
        drop(original);
        let damaged = fs::read(root.join(name))?;
        ensure!(
            file_icons_windows::restore_registration().is_err(),
            "oversized backup accepted"
        );
        ensure!(
            fs::read(root.join(name))? == damaged,
            "oversized original changed"
        );
        ensure!(
            fs::read(root.join("recovery.json"))? == recovery,
            "backup failure lost recovery"
        );
        ensure!(
            fs::read_dir(&root)?.count() == backups_before,
            "backup failure left partial files"
        );
        let mut actual = Snapshot::new();
        snapshot(&classes, "", &mut actual)?;
        ensure!(
            actual == pending_registry,
            "backup failure changed registration"
        );
        fs::write(root.join("settings.json"), &runtime)?;
        fs::write(root.join("settings.library.json"), &library)?;
    }
    {
        use std::os::windows::fs::OpenOptionsExt;
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("settings.library.json"))?;
        ensure!(
            file_icons_windows::restore_registration().is_err(),
            "backup read failure ignored"
        );
        let mut actual = Snapshot::new();
        snapshot(&classes, "", &mut actual)?;
        ensure!(
            actual == pending_registry,
            "backup read failure changed registry"
        );
        ensure!(
            fs::read(root.join("recovery.json"))? == recovery,
            "backup read failure lost recovery"
        );
        drop(locked);
    }
    fs::write(root.join("settings.json"), b"broken runtime")?;
    fs::write(root.join("settings.library.json"), b"broken library")?;
    file_icons_windows::restore_registration()?;
    let preserved: Vec<_> = fs::read_dir(&root)?.collect::<std::io::Result<Vec<_>>>()?;
    for (prefix, expected) in [
        ("settings-damaged-", b"broken runtime".as_slice()),
        ("settings-library-damaged-", b"broken library".as_slice()),
    ] {
        let copies: Vec<_> = preserved
            .iter()
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .collect();
        ensure!(
            copies.len() == 1 && fs::read(copies[0].path())? == expected,
            "explicit recovery lacks full backup"
        );
    }
    ensure!(
        file_icons_windows::load()?.journal.is_empty(),
        "recovery did not finish"
    );
    Ok(())
}

#[test]
fn isolated_shell_registration_probe() -> Result<()> {
    if std::env::var_os("WFI_STANDARD_USER_PROBE").is_none() {
        let result = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "isolated_shell_registration_probe",
                "--nocapture",
            ])
            .env("WFI_STANDARD_USER_PROBE", "1")
            .test_output_as_standard_user(std::time::Duration::from_secs(420))?;
        println!("標準ユーザー: {}", String::from_utf8_lossy(&result.stdout));
        ensure!(
            result.status.success(),
            "標準ユーザーの子試験に失敗しました: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        return Ok(());
    }
    ensure!(
        !unsafe { IsUserAnAdmin().as_bool() },
        "Shell結合試験には管理者でないトークンが必要です"
    );
    if let Ok(namespace) = std::env::var("WFI_SHARED_RESTORED_READER") {
        let _redirect = Redirect::attach(namespace)?;
        initialize_com()?;
        let root = file_icons_windows::home()?;
        ensure!(
            icon_images(&root.join("original.wfi-dynamic"))?
                == icon_images(&root.join("reference-app.lnk.wfi-native"))?,
            "fresh process did not restore the original dynamic icon"
        );
        unsafe {
            CoUninitialize();
        }
        return Ok(());
    }
    if std::env::var_os("WFI_RECOVERY_READER").is_some() {
        return fresh_recovery_reader();
    }
    if std::env::var_os("WFI_ISOLATED_PROBE").is_none() {
        for apartment in ["sta", "mta"] {
            let profile = tempfile::tempdir()?;
            let result = Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "isolated_shell_registration_probe",
                    "--nocapture",
                ])
                .env("WFI_ISOLATED_PROBE", "1")
                .env("WFI_PROBE_APARTMENT", apartment)
                .env("LOCALAPPDATA", profile.path())
                .test_output()?;
            println!("{apartment}: {}", String::from_utf8_lossy(&result.stdout));
            ensure!(
                result.status.success(),
                "isolated child failed ({apartment}): {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        return Ok(());
    }
    let root = file_icons_windows::home()?;
    fs::create_dir_all(&root)?;
    let mut pack = fixture(&root)?;
    let mut rules = pack.theme.rules().clone();
    let themed_icon = pack
        .theme
        .resolve("special.wfi-known")
        .unwrap()
        .to_path_buf();
    rules.extensions.insert("lnk".into(), themed_icon.clone());
    rules.extensions.insert("exe".into(), themed_icon.clone());
    rules.files.insert("app.exe".into(), themed_icon);
    pack.theme.replace_rules(rules)?;
    let (redirect, classes) = Redirect::new()?;
    initialize_com()?;
    let dll = std::env::current_exe()?
        .parent()
        .context("missing executable parent")?
        .join("file_icons_shell.dll")
        .canonicalize()?;
    ensure!(dll.is_file(), "build Shell DLL before probe");
    let distribution = root.join("distribution");
    fs::create_dir_all(&distribution)?;
    let source = distribution.join("file_icons_shell.dll");
    fs::copy(&dll, &source)?;
    let dll = source;
    let installed = file_icons_windows::stage_shell(&dll)?;
    let _activation = Activation::new(
        installed.parent().context("staged DLL parent missing")?,
        &installed,
    )?;
    write(&classes, ".wfi-known", "", "WfiProbe.File")?;
    write(&classes, ".wfi-shared", "", "WFIPROBE.FILE")?;
    write(
        &classes,
        "WfiProbe.File\\shell\\open\\command",
        "",
        "notepad.exe \"%1\"",
    )?;
    classes.create_subkey("WfiProbe.File\\shellex")?;
    let original_icon = format!("{}\\System32\\shell32.dll,0", std::env::var("SystemRoot")?);
    write(&classes, "WfiProbe.File\\DefaultIcon", "", &original_icon)?;
    write(&classes, "Unknown\\DefaultIcon", "", &original_icon)?;
    let original_handler = "{00021401-0000-0000-C000-000000000046}";
    write(&classes, ".wfi-link", "", "WfiProbe.Link")?;
    write(&classes, "WfiProbe.Link\\DefaultIcon", "", "%1")?;
    write(
        &classes,
        "WfiProbe.Link\\shellex\\IconHandler",
        "",
        original_handler,
    )?;
    write(&classes, ".lnk", "", "lnkfile")?;
    write(
        &classes,
        "lnkfile\\shellex\\IconHandler",
        "",
        original_handler,
    )?;
    write(&classes, ".exe", "", "exefile")?;
    write(&classes, "exefile\\DefaultIcon", "", "%1")?;
    let executable = root.join("app.exe");
    fs::copy(std::env::current_exe()?, &executable)?;
    let executable_baseline = icon_images(&executable)?;
    let executable_contents = fs::read(&executable)?;
    let linked_names = [
        "normal.wfi-link",
        "second.wfi-link",
        "special.wfi-link",
        "app.lnk",
        "second.LNK",
    ];
    let mut link_baseline = std::collections::BTreeMap::new();
    let mut link_contents = std::collections::BTreeMap::new();
    for name in linked_names {
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
        let icon = pack
            .theme
            .resolve(if name.starts_with("second") {
                "special.wfi-known"
            } else {
                "normal.wfi-known"
            })
            .unwrap();
        let icon = icon
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let path = root.join(name);
        let wide = path
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        unsafe {
            link.SetIconLocation(PCWSTR(icon.as_ptr()), 0)?;
            link.cast::<IPersistFile>()?
                .Save(PCWSTR(wide.as_ptr()), true)?;
        }
        link_baseline.insert(name, icon_images(&path)?);
        link_contents.insert(name, fs::read(&path)?);
    }
    ensure!(
        link_baseline[linked_names[0]] != link_baseline[linked_names[1]],
        "native handler did not resolve per-file icons"
    );
    // 別プロセスでの描画比較は、同じプロセス内の未変更クラスを参照にする。
    write(&classes, ".wfi-reference", "", "WfiProbe.Reference")?;
    write(
        &classes,
        "WfiProbe.Reference\\DefaultIcon",
        "",
        &original_icon,
    )?;
    fs::write(root.join("original.wfi-reference"), "unchanged")?;
    write(&classes, ".wfi-native", "", "WfiProbe.Native")?;
    write(&classes, "WfiProbe.Native\\DefaultIcon", "", "%1")?;
    write(
        &classes,
        "WfiProbe.Native\\shellex\\IconHandler",
        "",
        original_handler,
    )?;
    for name in linked_names {
        fs::copy(
            root.join(name),
            root.join(format!("reference-{name}.wfi-native")),
        )?;
    }
    println!(
        "case-only association: known={:?}, shared={:?}",
        file_icons_windows::association("wfi-known")?,
        file_icons_windows::association("wfi-shared")?
    );
    protection::verify(&root, &classes, &pack, &dll)?;
    icon_policy::verify(&classes, &pack, &dll, &redirect.name)?;
    casing::verify(&classes, &pack, &dll, &original_icon)?;
    let _switch_write_denial = switching::prepare(&classes, &original_icon)?;
    let mut before = Snapshot::new();
    snapshot(&classes, "", &mut before)?;
    let mut settings = file_icons_windows::Settings {
        // この試験では、明示的に保護を無効にした場合のテーマ優先と委譲を検証する。
        preserve_existing_icons: false,
        packs: vec![pack.clone()],
        ..Default::default()
    };
    file_icons_windows::save(&settings)?;
    let baseline_files = ["unthemed.wfi-shared", "UnthemedName", "unthemed.wfi-other"];
    let mut baseline = std::collections::BTreeMap::new();
    for name in baseline_files {
        let file = root.join(name);
        fs::write(&file, "unchanged")?;
        baseline.insert(name, icon_images(&file)?);
    }
    // 復元できない識別子と自己参照は、書き込み前の計画で拒否する。
    for invalid in ["not-a-guid", file_icons_windows::CLSID_TEXT] {
        write(&classes, "WfiProbe.Link\\shellex\\IconHandler", "", invalid)?;
        let mut invalid_before = Snapshot::new();
        snapshot(&classes, "", &mut invalid_before)?;
        ensure!(
            file_icons_windows::plan_with_protection(&pack, &dll, false).is_err(),
            "unsafe handler identifier accepted"
        );
        let mut invalid_after = Snapshot::new();
        snapshot(&classes, "", &mut invalid_after)?;
        ensure!(
            invalid_before == invalid_after,
            "rejected plan changed registration"
        );
    }
    write(
        &classes,
        "WfiProbe.Link\\shellex\\IconHandler",
        "",
        original_handler,
    )?;
    fs::write(root.join("recovery.json"), b"broken previous record")?;
    ensure!(
        file_icons_windows::apply(&mut settings, &pack.id, &dll).is_err(),
        "previous record overwritten"
    );
    ensure!(
        fs::read(root.join("recovery.json"))? == b"broken previous record",
        "previous recovery data changed"
    );
    let mut rejected = Snapshot::new();
    snapshot(&classes, "", &mut rejected)?;
    ensure!(before == rejected, "failed backup changed registry");
    fs::remove_file(root.join("recovery.json"))?;
    // 独立記録の保存後に設定保存が失敗しても、登録値は変えず復元を再試行できる。
    let settings_path = root.join("settings.json");
    fs::remove_file(&settings_path)?;
    fs::create_dir(&settings_path)?;
    ensure!(
        file_icons_windows::apply(&mut settings, &pack.id, &dll).is_err(),
        "settings write failure ignored"
    );
    ensure!(
        root.join("recovery.json").is_file(),
        "recovery record not saved before preferences"
    );
    let mut failed_save = Snapshot::new();
    snapshot(&classes, "", &mut failed_save)?;
    ensure!(
        before == failed_save,
        "failed preferences save changed registration"
    );
    fs::remove_dir(&settings_path)?;
    file_icons_windows::restore_registration()?;
    settings = file_icons_windows::Settings {
        preserve_existing_icons: false,
        packs: vec![pack.clone()],
        ..Default::default()
    };
    file_icons_windows::save(&settings)?;
    file_icons_windows::apply(&mut settings, &pack.id, &dll)?;
    ensure!(
        settings.original_handler("WfiProbe.Link") == Some(original_handler),
        "original handler not saved"
    );
    ensure!(
        icon_images(&executable)? == executable_baseline,
        "executable icon changed"
    );
    ensure!(
        fs::read(&executable)? == executable_contents,
        "executable content changed"
    );
    ensure!(
        !settings
            .journal
            .iter()
            .any(|change| change.key.contains("lnkfile") || change.key.contains("exefile")),
        "protected class registered"
    );
    for name in linked_names {
        let path = root.join(name);
        let image = icon_images(&path)?;
        if name.starts_with("special") {
            ensure!(
                image != link_baseline[name],
                "theme did not override existing handler"
            );
            ensure!(
                query(&path)?.0 == pack.theme.resolve(name).unwrap().to_string_lossy(),
                "wrong themed icon over original handler"
            );
        } else {
            ensure!(
                image == link_baseline[name],
                "original per-file handler changed: {name}"
            );
        }
        ensure!(
            fs::read(path)? == link_contents[name],
            "native file contents changed"
        );
    }
    ensure!(
        !settings.created_keys.is_empty(),
        "new registration keys not journaled"
    );
    ensure!(
        !settings
            .created_keys
            .iter()
            .any(|key| key == "Software\\Classes\\WfiProbe.File\\shellex"),
        "preexisting empty key marked for deletion"
    );
    // この子プロセスだけに登録したDLLをCOM経由で生成できることを先に確認する。
    let handler: IPersistFile =
        unsafe { CoCreateInstance(&file_icons_windows::CLSID, None, CLSCTX_INPROC_SERVER)? };
    let registered: String = classes
        .open_subkey(format!(
            "CLSID\\{}\\InprocServer32",
            file_icons_windows::CLSID_TEXT
        ))?
        .get_value("")?;
    ensure!(
        Path::new(&registered) == installed,
        "registration points to distribution file"
    );
    ensure!(
        fs::OpenOptions::new().write(true).open(&installed).is_err(),
        "mapped DLL unexpectedly writable"
    );
    ensure!(
        file_icons_windows::stage_shell(&dll)? == installed,
        "mapped DLL was not reused"
    );
    // 実DLL内で関連付けの照会結果を再利用した後、変更通知で取り直せることを確認する。
    let linked = root.join("normal.wfi-link");
    let native_before = direct_query(&handler, &linked)?;
    ensure!(
        Path::new(&native_before.0) == pack.theme.resolve("normal.wfi-known").unwrap(),
        "unexpected delegated icon before association change"
    );
    ensure!(
        direct_query(&handler, &linked)? == native_before,
        "cached association changed native icon"
    );
    write(&classes, ".wfi-link", "", "WFIPROBE.LINK")?;
    file_icons_windows::notify();
    ensure!(
        direct_query(&handler, &linked)? == native_before,
        "case-only ProgID change lost the original handler"
    );
    write(&classes, ".wfi-link", "", "wfiprobe.file")?;
    file_icons_windows::notify();
    ensure!(
        direct_query(&handler, &linked)?
            == (original_icon.rsplit_once(',').unwrap().0.to_owned(), 0),
        "case-only ProgID change lost the original icon"
    );
    write(&classes, ".wfi-link", "", "WfiProbe.File")?;
    file_icons_windows::notify();
    let changed = direct_query(&handler, &linked)?;
    let original_path = original_icon.rsplit_once(',').unwrap().0;
    ensure!(
        changed == (original_path.to_owned(), 0),
        "association cache kept the previous class"
    );
    write(&classes, ".wfi-link", "", "WfiProbe.Link")?;
    file_icons_windows::notify();
    ensure!(
        direct_query(&handler, &linked)? == native_before,
        "association change was not rearmed"
    );
    for name in [
        "normal.wfi-known",
        "special.wfi-known",
        "normal.wfi-unknown",
        "special.wfi-unknown",
        "Dockerfile",
    ] {
        let file = root.join(name);
        fs::write(&file, "unchanged")?;
        let icon = query(&file)?;
        println!("{name}: {icon:?}");
        let expected = pack.theme.resolve(name).context("missing probe rule")?;
        ensure!(
            Path::new(&icon.0) == expected,
            "registered icon mismatch for {name}"
        );
        for (image, size) in [(pixels(&file, true)?, 16), (pixels(&file, false)?, 32)] {
            let center = ((size / 2 * size + size / 2) * 4) as usize;
            let expected = if name.starts_with("normal") {
                [0, 0, 255]
            } else {
                [255, 0, 0]
            };
            ensure!(
                image[center..center + 3] == expected,
                "wrong actual icon pixels: {name}, size={size}"
            );
        }
        ensure!(
            fs::read_to_string(&file)? == "unchanged",
            "file content changed"
        );
    }
    // 同じクラスに到達しても、無効対象だけが元の画像へ戻り、固有名の例外を保つ。
    let previous_selection = settings.active_selection.clone();
    settings
        .active_selection
        .set("extension:wfi-known", Some(false))?;
    settings
        .active_selection
        .set("file:special.wfi-known", Some(true))?;
    file_icons_windows::save(&settings)?;
    file_icons_windows::notify();
    ensure!(
        Path::new(&direct_query(&handler, &root.join("normal.wfi-known"))?.0)
            != pack.theme.resolve("normal.wfi-known").unwrap(),
        "disabled extension still returned a themed icon"
    );
    ensure!(
        Path::new(&direct_query(&handler, &root.join("special.wfi-known"))?.0)
            == pack.theme.resolve("special.wfi-known").unwrap(),
        "filename exception lost its themed icon"
    );
    ensure!(
        icon_images(&root.join("normal.wfi-known"))? == baseline["unthemed.wfi-shared"],
        "disabled extension did not restore original pixels"
    );
    settings
        .active_selection
        .set("file:special.wfi-known", Some(false))?;
    file_icons_windows::save(&settings)?;
    file_icons_windows::notify();
    ensure!(
        Path::new(&direct_query(&handler, &root.join("special.wfi-known"))?.0)
            != pack.theme.resolve("special.wfi-known").unwrap(),
        "disabled filename fell through to extension artwork"
    );
    settings.active_selection = previous_selection;
    file_icons_windows::save(&settings)?;
    file_icons_windows::notify();
    switching::verify(&root, &classes, &mut settings, &pack, &dll)?;
    for name in baseline_files {
        ensure!(
            icon_images(&root.join(name))? == baseline[name],
            "unsupported icon pixels changed: {name}"
        );
    }
    let nested = root.join("nested");
    fs::create_dir_all(&nested)?;
    let original_name = nested.join("normal.wfi-known");
    fs::write(&original_name, "rename-preserves-content")?;
    let normal_before = icon_images(&original_name)?;
    fs::rename(&original_name, nested.join("special.wfi-known"))?;
    let renamed = nested.join("special.wfi-known");
    ensure!(
        icon_images(&renamed)? != normal_before,
        "rename did not change icon"
    );
    ensure!(
        query(&renamed)?.0
            == pack
                .theme
                .resolve("special.wfi-known")
                .unwrap()
                .to_string_lossy(),
        "second directory needed extra registration"
    );
    ensure!(
        fs::read_to_string(&renamed)? == "rename-preserves-content",
        "rename changed contents"
    );
    // レジストリを戻した後に設定保存が失敗しても、復元記録を失わず再実行できる。
    fs::remove_file(root.join("settings.json"))?;
    fs::create_dir(root.join("settings.json"))?;
    ensure!(
        file_icons_windows::restore(&mut settings).is_err(),
        "restore settings failure ignored"
    );
    ensure!(
        root.join("recovery.json").is_file(),
        "failed restoration lost recovery record"
    );
    let mut interrupted_restore = Snapshot::new();
    snapshot(&classes, "", &mut interrupted_restore)?;
    ensure!(
        interrupted_restore == before,
        "partial restoration did not restore registry"
    );
    fs::remove_dir(root.join("settings.json"))?;
    file_icons_windows::restore_registration()?;
    settings = file_icons_windows::Settings {
        packs: vec![pack.clone()],
        preserve_existing_icons: false,
        ..Default::default()
    };
    file_icons_windows::save(&settings)?;
    file_icons_windows::apply(&mut settings, &pack.id, &dll)?;
    fs::remove_file(&dll)?;
    ensure!(
        installed.is_file() && !dll.exists(),
        "distribution DLL still required after staging"
    );
    fs::write(root.join("settings.json"), "broken")?;
    ensure!(
        icon_images(&root.join("normal.wfi-known"))? == baseline["unthemed.wfi-shared"],
        "corrupt settings did not fall back to original icon"
    );
    for name in linked_names {
        ensure!(
            icon_images(&root.join(name))? == link_baseline[name],
            "corrupt settings did not retain original handler: {name}"
        );
    }
    let mut fresh_expected = std::collections::BTreeMap::new();
    fresh_expected.insert(
        "normal.wfi-known".to_owned(),
        "original.wfi-reference".to_owned(),
    );
    for name in linked_names {
        fresh_expected.insert(name.to_owned(), format!("reference-{name}.wfi-native"));
    }
    fs::write(
        root.join("fresh-expected.json"),
        serde_json::to_vec(&fresh_expected)?,
    )?;
    for missing in [false, true] {
        if missing {
            fs::remove_file(root.join("settings.json"))?;
        }
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "isolated_shell_registration_probe",
                "--nocapture",
            ])
            .env("WFI_RECOVERY_READER", &redirect.name)
            .test_output()?;
        ensure!(
            output.status.success(),
            "fresh recovery reader failed (missing={missing}): {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(root.join("settings.json"), b"broken")?;
    let command: String = classes
        .open_subkey("WfiProbe.File\\shell\\open\\command")?
        .get_value("")?;
    ensure!(
        command == "notepad.exe \"%1\"",
        "default application changed"
    );
    file_icons_windows::restore_registration()?;
    settings = file_icons_windows::load()?;
    file_icons_windows::restore(&mut settings)?;
    ensure!(
        !root.join("recovery.json").exists(),
        "completed restoration retained pending journal"
    );
    let backups = fs::read_dir(&root)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("settings-damaged-")
        })
        .collect::<Vec<_>>();
    ensure!(
        backups.len() == 1 && fs::read(backups[0].path())? == b"broken",
        "damaged settings not preserved"
    );
    let mut after = Snapshot::new();
    snapshot(&classes, "", &mut after)?;
    ensure!(
        before == after,
        "registry keys or raw values were not fully restored: before={before:?}, after={after:?}"
    );
    for name in baseline_files {
        ensure!(
            icon_images(&root.join(name))? == baseline[name],
            "restored icon changed: {name}"
        );
    }
    for name in linked_names {
        ensure!(
            icon_images(&root.join(name))? == link_baseline[name],
            "original handler not restored: {name}"
        );
    }
    ensure!(
        settings.original_handlers.is_empty(),
        "delegation state survived restoration"
    );
    drop(handler);
    unsafe {
        CoUninitialize();
    }
    Ok(())
}
