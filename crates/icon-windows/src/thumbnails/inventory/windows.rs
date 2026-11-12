//! 現在のWindows登録を読み取り専用で照会する。

use super::{Scope, Source};
use anyhow::{Context, Result, ensure};
use file_icons_core::registry::Value;
use std::{collections::BTreeSet, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_ASSOCIATION, RPC_E_CHANGED_MODE},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            AL_EFFECTIVE, ASSOCF_NONE, ASSOCSTR_SHELLEXTENSION, AT_FILEEXTENSION,
            ApplicationAssociationRegistration, BHID_AssociationArray,
            IApplicationAssociationRegistration, IQueryAssociations, IShellItem,
            SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, SHCreateItemFromParsingName,
        },
    },
    core::{GUID, HRESULT, PCWSTR, PWSTR, w},
};
use winreg::{RegKey, enums::*};

/// インスタンスを作成したスレッド上で使用・破棄する。
pub struct WindowsSource {
    association: Option<IApplicationAssociationRegistration>,
    uninitialize: bool,
}

impl WindowsSource {
    pub fn new() -> Result<Self> {
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let uninitialize = if status == RPC_E_CHANGED_MODE {
            false
        } else {
            status.ok()?;
            true
        };
        let association = unsafe {
            CoCreateInstance(
                &ApplicationAssociationRegistration,
                None,
                CLSCTX_INPROC_SERVER,
            )
        };
        match association {
            Ok(association) => Ok(Self {
                association: Some(association),
                uninitialize,
            }),
            Err(error) => {
                if uninitialize {
                    unsafe { CoUninitialize() }
                }
                Err(error.into())
            }
        }
    }

    /// 実在する試験ファイルについて、Shellが選ぶ提供元を読み取る。
    pub fn shell_provider(&self, probe: &Path) -> Result<Option<GUID>> {
        ensure!(probe.is_absolute(), "thumbnail probe path must be absolute");
        let mut path: Vec<u16> = probe.as_os_str().encode_wide().collect();
        ensure!(!path.contains(&0), "invalid thumbnail probe path");
        path.push(0);
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None)? };
        let query: IQueryAssociations =
            unsafe { item.BindToHandler(None, &BHID_AssociationArray)? };
        let mut result = [0u16; 128];
        let mut length = result.len() as u32;
        let status = unsafe {
            query.GetString(
                ASSOCF_NONE,
                ASSOCSTR_SHELLEXTENSION,
                w!("{e357fccd-a995-4576-b01f-234630154e96}"),
                Some(PWSTR(result.as_mut_ptr())),
                &mut length,
            )
        };
        match status {
            Ok(()) => {
                let end = result
                    .iter()
                    .position(|unit| *unit == 0)
                    .context("unterminated thumbnail provider")?;
                let text = String::from_utf16(&result[..end])?;
                let guid = text
                    .strip_prefix('{')
                    .and_then(|text| text.strip_suffix('}'))
                    .context("invalid thumbnail provider")?;
                Ok(Some(GUID::try_from(guid)?))
            }
            Err(error)
                if error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
                    || error.code() == HRESULT::from_win32(ERROR_NO_ASSOCIATION.0) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// 対象拡張子の空ファイルを一時作成し、Shellの実効提供元だけを調べる。
    pub fn provider_for_extension(&self, extension: &str) -> Result<Option<GUID>> {
        ensure!(
            super::super::valid_extension(extension),
            "invalid thumbnail extension"
        );
        let probe = tempfile::Builder::new().suffix(extension).tempfile()?;
        self.shell_provider(probe.path())
    }
}

impl Drop for WindowsSource {
    fn drop(&mut self) {
        drop(self.association.take());
        if self.uninitialize {
            unsafe { CoUninitialize() }
        }
    }
}

impl crate::thumbnails::ProviderState for super::Inspector<WindowsSource> {
    fn provider(&self, extension: &str) -> Result<Option<GUID>> {
        self.0.provider_for_extension(extension)
    }
    fn notify(&self) -> Result<()> {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        Ok(())
    }
}

impl Source for WindowsSource {
    fn extensions(&self) -> Result<Vec<String>> {
        let mut extensions = BTreeSet::new();
        for entry in RegKey::predef(HKEY_CLASSES_ROOT).enum_keys() {
            let key = entry?;
            if key.starts_with('.') {
                extensions.insert(key.to_ascii_lowercase());
            }
            ensure!(extensions.len() <= 65536, "too many registered extensions");
        }
        match RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts")
        {
            Ok(root) => {
                for entry in root.enum_keys() {
                    let key = entry?;
                    if key.starts_with('.') {
                        extensions.insert(key.to_ascii_lowercase());
                    }
                    ensure!(extensions.len() <= 65536, "too many registered extensions");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        Ok(extensions.into_iter().collect())
    }

    fn association(&self, extension: &str) -> Result<Option<String>> {
        let wide: Vec<u16> = extension.encode_utf16().chain(Some(0)).collect();
        let registration = self
            .association
            .as_ref()
            .context("association registration unavailable")?;
        let result = unsafe {
            registration.QueryCurrentDefault(PCWSTR(wide.as_ptr()), AT_FILEEXTENSION, AL_EFFECTIVE)
        };
        match result {
            Ok(value) => {
                ensure!(!value.is_null(), "association query returned no value");
                let text = unsafe { value.to_string() };
                unsafe { CoTaskMemFree(Some(value.0.cast())) };
                let text = text?;
                ensure!(!text.is_empty(), "association query returned empty value");
                Ok(Some(text))
            }
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_ASSOCIATION.0) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn curver(&self, class: &str) -> Result<Option<String>> {
        let path = format!("{class}\\CurVer");
        let value = match RegKey::predef(HKEY_CLASSES_ROOT)
            .open_subkey(path)
            .and_then(|key| key.get_raw_value(""))
        {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            value.vtype == REG_SZ
                && value.bytes.len() <= 512
                && value.bytes.len().is_multiple_of(2),
            "unsupported CurVer value"
        );
        let units: Vec<u16> = value
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let end = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        ensure!(
            end > 0 && units[end..].iter().all(|unit| *unit == 0),
            "invalid CurVer value"
        );
        Ok(Some(String::from_utf16(&units[..end])?))
    }

    fn read(&self, scope: Scope, key: &str) -> Result<Option<Value>> {
        let root = match scope {
            Scope::User => HKEY_CURRENT_USER,
            Scope::Machine => HKEY_LOCAL_MACHINE,
        };
        match RegKey::predef(root)
            .open_subkey(key)
            .and_then(|key| key.get_raw_value(""))
        {
            Ok(value) => Ok(Some(Value {
                kind: value.vtype as u32,
                bytes: value.bytes.into_owned(),
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_process::CommandOutput;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CLASSES_ROOT as HKCR, HKEY_CURRENT_USER as HKCU, HKEY_LOCAL_MACHINE as HKLM,
        RegOverridePredefKey,
    };

    #[test]
    fn reads_only_redirected_registry() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "thumbnails::inventory::windows::tests::isolated_windows_source_child",
                "--nocapture",
            ])
            .test_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "親試験から独立プロセスでレジストリを転送する"]
    fn isolated_windows_source_child() -> Result<()> {
        let source = WindowsSource::new().context("initialize association query")?;
        let temporary = tempfile::tempdir()?;
        let suffix = temporary
            .path()
            .file_name()
            .context("temporary name missing")?
            .to_string_lossy();
        let name = format!("Software\\FileHueThumbnailTest-{suffix}");
        let physical = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("", KEY_ALL_ACCESS)
            .context("open test registry root")?;
        let (user, disposition) = physical
            .create_subkey(&name)
            .context("create isolated test registry")?;
        ensure!(
            disposition == REG_CREATED_NEW_KEY,
            "isolated test key already exists"
        );
        struct Redirect {
            physical: RegKey,
            name: String,
        }
        impl Drop for Redirect {
            fn drop(&mut self) {
                unsafe {
                    let _ = RegOverridePredefKey(HKCR, None);
                    let _ = RegOverridePredefKey(HKCU, None);
                    let _ = RegOverridePredefKey(HKLM, None);
                }
                let _ = self.physical.delete_subkey_all(&self.name);
            }
        }
        let guard = Redirect { physical, name };
        let (classes, _) = user.create_subkey("Software\\Classes")?;
        let (machine, _) = user.create_subkey("Machine")?;
        let (extension, _) = classes.create_subkey(".wfitest")?;
        extension.set_value("", &"Example.Vector")?;
        let (command, _) = classes.create_subkey("Example.Vector.1\\shell\\open\\command")?;
        command.set_value("", &"\"X:\\not-executed.exe\" \"%1\"")?;
        let (curver, _) = classes.create_subkey("Example.Vector\\CurVer")?;
        curver.set_value("", &"Example.Vector.1")?;
        let (parent_provider, _) = classes.create_subkey(format!(
            "Example.Vector.1\\shellex\\{}",
            crate::thumbnails::THUMBNAIL_IID
        ))?;
        parent_provider.set_value("", &"{11111111-2222-4333-8444-555555555555}")?;
        let (extension_provider, _) = classes.create_subkey(format!(
            ".wfitest\\shellex\\{}",
            crate::thumbnails::THUMBNAIL_IID
        ))?;
        extension_provider.set_value("", &"{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}")?;
        let (other_extension, _) = classes.create_subkey(".wfiother")?;
        other_extension.set_value("", &"Other.Vector")?;
        let (other_curver, _) = classes.create_subkey("Other.Vector\\CurVer")?;
        other_curver.set_value("", &"Other.Vector.1")?;
        let (other_command, _) = classes.create_subkey("Other.Vector\\shell\\open\\command")?;
        other_command.set_value("", &"\"X:\\not-executed.exe\" \"%1\"")?;
        let (other_parent, _) = classes.create_subkey(format!(
            "Other.Vector.1\\shellex\\{}",
            crate::thumbnails::THUMBNAIL_IID
        ))?;
        other_parent.set_value("", &"{11111111-2222-4333-8444-555555555555}")?;
        let (other_provider, _) = classes.create_subkey(format!(
            ".wfiother\\shellex\\{}",
            crate::thumbnails::THUMBNAIL_IID
        ))?;
        other_provider.set_value("", &"{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}")?;
        let (svg_extension, _) = classes.create_subkey(".svg")?;
        svg_extension.set_value("", &"Example.Svg")?;
        let (svg_command, _) = classes.create_subkey("Example.Svg\\shell\\open\\command")?;
        svg_command.set_value("", &"\"X:\\not-executed.exe\" \"%1\"")?;
        let (svg_provider, _) = classes.create_subkey(format!(
            ".svg\\shellex\\{}",
            crate::thumbnails::THUMBNAIL_IID
        ))?;
        let mut svg_unterminated: Vec<u8> = "{11111111-2222-4333-8444-555555555555}"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let original_length = svg_unterminated.len();
        svg_unterminated.extend_from_slice(&[1, 1]);
        svg_provider.set_raw_value(
            "",
            &winreg::RegValue {
                vtype: REG_SZ,
                bytes: (&svg_unterminated[..original_length]).into(),
            },
        )?;
        assert_eq!(svg_provider.get_raw_value("")?.bytes.len(), original_length);
        let (svg_icon, _) = classes.create_subkey("Example.Svg\\DefaultIcon")?;
        svg_icon.set_value("", &"original.ico")?;
        let (shared_command, _) = classes.create_subkey("Example.Shared\\shell\\open\\command")?;
        shared_command.set_value("", &"\"X:\\not-executed.exe\" \"%1\"")?;
        let (shared_provider, _) = classes.create_subkey(format!(
            "Example.Shared\\shellex\\{}",
            crate::thumbnails::THUMBNAIL_IID
        ))?;
        shared_provider.set_value("", &"{11111111-2222-4333-8444-555555555555}")?;
        let (shared_icon, _) = classes.create_subkey("Example.Shared\\DefaultIcon")?;
        shared_icon.set_value("", &"shared.ico")?;
        let mut shared_keys = Vec::new();
        for extension in [".ai", ".ait", ".eps"] {
            let (key, _) = classes.create_subkey(extension)?;
            key.set_value("", &"Example.Shared")?;
            let (handler, _) = classes.create_subkey(format!(
                "{extension}\\shellex\\{}",
                crate::thumbnails::THUMBNAIL_IID
            ))?;
            if extension == ".ait" {
                handler.set_value("", &"{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}")?;
            }
            shared_keys.push((key, handler));
        }
        let probe = tempfile::Builder::new()
            .suffix(".wfitest")
            .tempfile_in(temporary.path())?;
        let missing_probe = tempfile::Builder::new()
            .suffix(".wfi-nonexistent")
            .tempfile_in(temporary.path())?;
        let other_probe = tempfile::Builder::new()
            .suffix(".wfiother")
            .tempfile_in(temporary.path())?;
        let (machine_value, _) =
            machine.create_subkey("Software\\Classes\\Example.Vector\\shellex\\test")?;
        machine_value.set_raw_value(
            "",
            &winreg::RegValue {
                vtype: REG_BINARY,
                bytes: (&[1u8, 0, 255][..]).into(),
            },
        )?;
        unsafe {
            RegOverridePredefKey(HKCU, Some(HKEY(user.raw_handle())))
                .ok()
                .context("redirect HKCU")?;
            RegOverridePredefKey(HKCR, Some(HKEY(classes.raw_handle())))
                .ok()
                .context("redirect HKCR")?;
            RegOverridePredefKey(HKLM, Some(HKEY(machine.raw_handle())))
                .ok()
                .context("redirect HKLM")?;
        }
        assert!(source.extensions()?.contains(&".wfitest".into()));
        assert_eq!(
            source.association(".wfitest")?,
            Some("Example.Vector".into())
        );
        assert_eq!(source.association(".wfi-nonexistent")?, None);
        assert_eq!(
            source.shell_provider(probe.path())?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        assert_eq!(
            source.provider_for_extension(".wfitest")?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        assert_eq!(source.shell_provider(missing_probe.path())?, None);
        assert_eq!(
            source.shell_provider(other_probe.path())?,
            Some(GUID::from_u128(0xaaaaaaaa_bbbb_4ccc_8ddd_eeeeeeeeeeee))
        );
        assert_eq!(
            super::super::classify(&source, ".wfitest", source.shell_provider(probe.path())?)?,
            super::super::Binding::SharedUser {
                class: "Example.Vector.1".into()
            }
        );
        assert_eq!(
            super::super::classify(
                &source,
                ".wfiother",
                source.shell_provider(other_probe.path())?
            )?,
            super::super::Binding::Extension
        );
        parent_provider.set_value("", &"")?;
        assert_eq!(
            source.shell_provider(probe.path())?,
            Some(GUID::from_u128(0xaaaaaaaa_bbbb_4ccc_8ddd_eeeeeeeeeeee))
        );
        assert_eq!(
            source.curver("Example.Vector")?,
            Some("Example.Vector.1".into())
        );
        assert_eq!(source.curver("Unknown.Test")?, None);
        assert_eq!(
            source.read(
                Scope::Machine,
                "Software\\Classes\\Example.Vector\\shellex\\test"
            )?,
            Some(Value {
                kind: 3,
                bytes: vec![1, 0, 255]
            })
        );
        assert_eq!(
            source.read(
                Scope::User,
                "Software\\Classes\\Example.Vector\\shellex\\test"
            )?,
            None
        );
        let group = crate::thumbnails::Group {
            class: Some("Example.Vector.1".into()),
            members: [".wfitest".into()].into_iter().collect(),
            dependencies: None,
            previous_providers: None,
            changes: vec![file_icons_core::registry::Change {
                key: format!(
                    "Software\\Classes\\Example.Vector.1\\shellex\\{}",
                    crate::thumbnails::THUMBNAIL_IID
                ),
                name: String::new(),
                before: None,
                after: Value {
                    kind: 1,
                    bytes: vec![0, 0],
                },
            }],
            created_keys: Vec::new(),
            keys_captured: false,
        };
        let inspector = super::super::Inspector(source);
        let snapshot = crate::thumbnails::DependencyInspector::inspect(&inspector, &group)?;
        assert_eq!(
            snapshot.associations.get(".wfitest").map(String::as_str),
            Some("Example.Vector")
        );
        crate::thumbnails::validate_dependencies(&group, &snapshot)?;
        struct Isolated(super::super::Inspector<WindowsSource>);
        impl crate::thumbnails::DependencyInspector for Isolated {
            fn inspect(
                &self,
                group: &crate::thumbnails::Group,
            ) -> Result<crate::thumbnails::Dependencies> {
                crate::thumbnails::DependencyInspector::inspect(&self.0, group)
            }
        }
        impl crate::thumbnails::ProviderState for Isolated {
            fn provider(&self, extension: &str) -> Result<Option<GUID>> {
                self.0.0.provider_for_extension(extension)
            }
            fn notify(&self) -> Result<()> {
                Ok(())
            }
        }
        let inspector = Isolated(inspector);
        let mut registry = crate::UserRegistry;
        let mut svg = crate::thumbnails::plan_extension_group(
            &registry,
            file_icons_thumbnail_core::format::Format::Svg,
        )?;
        svg.add_provider(
            &registry,
            file_icons_thumbnail_core::format::Format::Svg,
            Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
        )?;
        svg.capture_dependencies(&inspector)?;
        svg.capture_providers(&inspector)?;
        svg.capture_created_keys(&registry)?;
        let record = crate::thumbnails::Recovery {
            format_version: 1,
            transition: None,
            groups: vec![svg],
        };
        let recovery_path = temporary.path().join("recovery.json");
        record.apply_new(&recovery_path, &mut registry, &inspector)?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".svg")?,
            Some(GUID::from_u128(
                file_icons_thumbnail_core::format::Format::Svg.clsid()
            ))
        );
        assert_eq!(svg_icon.get_value::<String, _>("")?, "original.ico");
        crate::thumbnails::Recovery::load(&recovery_path)?
            .unwrap()
            .restore_checked_group(0, &mut registry, &inspector)?;
        assert_eq!(
            &*svg_provider.get_raw_value("")?.bytes,
            &svg_unterminated[..original_length]
        );
        record.remove_restored_group(&recovery_path, 0, &mut registry, &inspector)?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".svg")?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        assert_eq!(svg_icon.get_value::<String, _>("")?, "original.ico");
        assert!(
            matches!(classes.open_subkey("CLSID\\{C62D2AC2-0FD5-4EC7-BB81-FD245220DDA3}"), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        );
        use file_icons_thumbnail_core::format::Format;
        let no_assoc_recovery = temporary.path().join("unassociated.json");
        let selected = [".psb".into()].into_iter().collect();
        assert!(inspector.0.0.association(".psb")?.is_none());
        let unassociated = super::super::plan_selection(
            &registry,
            &inspector.0,
            &selected,
            Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll"),
        )?;
        unassociated.apply_new(&no_assoc_recovery, &mut registry, &inspector)?;
        assert!(inspector.0.0.association(".psb")?.is_none());
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".psb")?,
            Some(GUID::from_u128(Format::Psb.clsid()))
        );
        unassociated.restore_checked_group(0, &mut registry, &inspector)?;
        unassociated.remove_restored_group(&no_assoc_recovery, 0, &mut registry, &inspector)?;
        assert!(inspector.0.0.association(".psb")?.is_none());
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".psb")?,
            None
        );
        let members = [".ai".into(), ".ait".into(), ".eps".into()]
            .into_iter()
            .collect();
        let selected = [".ai".into(), ".eps".into()].into_iter().collect();
        let mut shared =
            crate::thumbnails::plan_shared_group(&registry, "Example.Shared", members, &selected)?;
        let module = Path::new("C:\\FileHue\\file_icons_thumbnail_shell.dll");
        shared.add_provider(&registry, Format::Ai, module)?;
        shared.add_provider(&registry, Format::Eps, module)?;
        shared.capture_dependencies(&inspector)?;
        shared.capture_providers(&inspector)?;
        shared.capture_created_keys(&registry)?;
        let shared_record = crate::thumbnails::Recovery {
            format_version: 1,
            transition: None,
            groups: vec![shared],
        };
        let shared_recovery = temporary.path().join("shared-recovery.json");
        shared_record.apply_new(&shared_recovery, &mut registry, &inspector)?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".ai")?,
            Some(GUID::from_u128(Format::Ai.clsid()))
        );
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".eps")?,
            Some(GUID::from_u128(Format::Eps.clsid()))
        );
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".ait")?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        assert_eq!(shared_icon.get_value::<String, _>("")?, "shared.ico");
        let shared_record = shared_record.set_shared_member(
            &shared_recovery,
            0,
            Format::Ai,
            false,
            &mut registry,
            &inspector,
        )?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".ai")?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".eps")?,
            Some(GUID::from_u128(Format::Eps.clsid()))
        );
        assert_eq!(shared_provider.get_value::<String, _>("")?, "");
        let shared_record = shared_record.set_shared_member(
            &shared_recovery,
            0,
            Format::Ai,
            true,
            &mut registry,
            &inspector,
        )?;
        assert!(shared_record.transition.is_none());
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".ai")?,
            Some(GUID::from_u128(Format::Ai.clsid()))
        );
        crate::thumbnails::Recovery::load(&shared_recovery)?
            .unwrap()
            .restore_checked_group(0, &mut registry, &inspector)?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".ai")?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        assert_eq!(
            shared_keys[1].1.get_value::<String, _>("")?,
            "{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}"
        );
        assert_eq!(shared_icon.get_value::<String, _>("")?, "shared.ico");
        let later_recovery = temporary.path().join("later-shared.json");
        let selected = [".ai".into()].into_iter().collect();
        let later = super::super::plan_selection(&registry, &inspector.0, &selected, module)?;
        later.apply_new(&later_recovery, &mut registry, &inspector)?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".eps")?,
            Some(GUID::from_u128(0x11111111_2222_4333_8444_555555555555))
        );
        let later = later.set_shared_member(
            &later_recovery,
            0,
            Format::Eps,
            true,
            &mut registry,
            &inspector,
        )?;
        assert_eq!(
            crate::thumbnails::ProviderState::provider(&inspector, ".eps")?,
            Some(GUID::from_u128(Format::Eps.clsid()))
        );
        later.restore_checked_group(0, &mut registry, &inspector)?;
        later.remove_restored_group(&later_recovery, 0, &mut registry, &inspector)?;
        let selected = [".ai".into(), ".eps".into(), ".svg".into()]
            .into_iter()
            .collect();
        let planned = super::super::plan_selection(&registry, &inspector.0, &selected, module)?;
        assert_eq!(planned.groups.len(), 2);
        let shared = planned
            .groups
            .iter()
            .find(|group| group.class.as_deref() == Some("Example.Shared"))
            .context("shared plan missing")?;
        assert_eq!(
            shared.members,
            [".ai".into(), ".ait".into(), ".eps".into()]
                .into_iter()
                .collect()
        );
        curver.set_raw_value(
            "",
            &winreg::RegValue {
                vtype: REG_BINARY,
                bytes: (&[1u8, 2][..]).into(),
            },
        )?;
        assert!(inspector.0.0.curver("Example.Vector").is_err());
        drop(inspector);
        drop(machine_value);
        drop(other_provider);
        drop(other_parent);
        drop(other_command);
        drop(other_curver);
        drop(other_extension);
        drop(svg_icon);
        drop(svg_provider);
        drop(svg_command);
        drop(svg_extension);
        drop(shared_keys);
        drop(shared_icon);
        drop(shared_provider);
        drop(shared_command);
        drop(parent_provider);
        drop(extension_provider);
        drop(command);
        drop(extension);
        drop(curver);
        drop(machine);
        drop(classes);
        drop(user);
        let cleanup_path = guard.name.clone();
        drop(guard);
        assert!(
            matches!(RegKey::predef(HKEY_CURRENT_USER).open_subkey(cleanup_path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        );
        Ok(())
    }
}
