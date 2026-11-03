use super::*;
use crate::test_process::CommandOutput;
use std::{
    cell::Cell,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

struct Namespace(String);
impl Drop for Namespace {
    fn drop(&mut self) {
        release();
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let _ = user.delete_subkey_all(&self.0);
    }
}
fn handles() -> u32 {
    let mut count = 0;
    unsafe {
        GetProcessHandleCount(GetCurrentProcess(), &mut count).unwrap();
    }
    count
}

#[test]
fn registry_cache_contract() {
    if std::env::var_os("WFI_ASSOC_CACHE_PROBE").is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "association::tests::registry_cache_contract",
                "--nocapture",
            ])
            .env("WFI_ASSOC_CACHE_PROBE", "1")
            .test_output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let name = format!(
        "Software\\WindowsFileIconsDev\\AssociationTests\\{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let physical = RegKey::predef(HKEY_CURRENT_USER);
    assert!(physical.open_subkey(&name).is_err());
    eprintln!("試験用HKCUキー: {name}");
    let (root, disposition) = physical.create_subkey(&name).unwrap();
    assert_eq!(disposition, winreg::enums::REG_CREATED_NEW_KEY);
    let _guard = Namespace(name.clone());
    let user_path = format!("{name}\\user");
    let machine_path = format!("{name}\\machine");
    let user = root.create_subkey("user").unwrap().0;
    let machine = root.create_subkey("machine").unwrap().0;
    user.set_value("class", &"First.File").unwrap();
    machine.set_value("other", &"Machine.File").unwrap();
    // 監視を開始したスレッドが終了しても通知待機を継続する。
    let thread_user = user_path.clone();
    let thread_machine = machine_path.clone();
    let cache = std::thread::spawn(move || {
        Cache::new(&[
            (RegKey::predef(HKEY_CURRENT_USER), &thread_user),
            (RegKey::predef(HKEY_CURRENT_USER), &thread_machine),
        ])
    })
    .join()
    .unwrap();
    *state() = Some(cache);
    let calls = Cell::new(0);
    let query = |extension: &str| -> std::io::Result<String> {
        assert_eq!(extension, "example");
        calls.set(calls.get() + 1);
        user.get_value("class")
    };
    assert_eq!(resolve("EXAMPLE", query).unwrap(), "First.File");
    assert_eq!(resolve("example", query).unwrap(), "First.File");
    assert_eq!(calls.get(), 1);
    user.set_value("class", &"Second.File").unwrap();
    assert_eq!(resolve("example", query).unwrap(), "Second.File");
    assert_eq!(calls.get(), 2);
    user.set_value("class", &"Third.File").unwrap();
    assert_eq!(resolve("example", query).unwrap(), "Third.File");
    assert_eq!(calls.get(), 3);
    // 別の監視根でも無効化する。細いキーの作成と削除も取りこぼさない。
    machine.set_value("other", &"NewMachine.File").unwrap();
    assert_eq!(resolve("example", query).unwrap(), "Third.File");
    assert_eq!(calls.get(), 4);
    user.create_subkey("FileExts\\example\\UserChoice")
        .unwrap()
        .0
        .set_value("ProgId", &"Choice.File")
        .unwrap();
    assert_eq!(resolve("example", query).unwrap(), "Third.File");
    assert_eq!(calls.get(), 5);
    user.delete_subkey_all("FileExts").unwrap();
    assert_eq!(resolve("example", query).unwrap(), "Third.File");
    assert_eq!(calls.get(), 6);
    // 照会中に通知された場合は、その照会が返した旧値を次回へ残さない。
    user.set_value("class", &"BeforeChange.File").unwrap();
    assert_eq!(
        resolve("example", |_| -> std::io::Result<String> {
            let before = user.get_value("class")?;
            user.set_value("class", &"AfterChange.File")?;
            Ok(before)
        })
        .unwrap(),
        "BeforeChange.File"
    );
    assert_eq!(resolve("example", query).unwrap(), "AfterChange.File");
    // 通知されない外部要因を表す照会結果も、保持期限後には取り直す。
    assert_eq!(
        resolve("timed", |_| Ok::<_, std::io::Error>("Old.File".into())).unwrap(),
        "Old.File"
    );
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        resolve("timed", |_| Ok::<_, std::io::Error>("New.File".into())).unwrap(),
        "New.File"
    );
    // 失敗を保持せず、同じキーで次の呼び出しを再試行する。
    assert!(resolve("failed", |_| Err::<String, _>("unavailable")).is_err());
    assert_eq!(
        resolve("failed", |_| Ok::<_, &str>("Recovered.File".into())).unwrap(),
        "Recovered.File"
    );
    // 上限を超えた入力の繰り返しも、古い最初の値を返さない。
    assert_eq!(
        resolve("first", |_| Ok::<_, &str>("First.File".into())).unwrap(),
        "First.File"
    );
    for index in 0..300 {
        resolve(&format!("other{index}"), |_| {
            Ok::<_, &str>("Other.File".into())
        })
        .unwrap();
    }
    assert_eq!(
        resolve("first", |_| Ok::<_, &str>("Fresh.File".into())).unwrap(),
        "Fresh.File"
    );
    let long = "x".repeat(1100);
    assert_eq!(
        resolve(&long, |_| Ok::<_, &str>("Long1.File".into())).unwrap(),
        "Long1.File"
    );
    assert_eq!(
        resolve(&long, |_| Ok::<_, &str>("Long2.File".into())).unwrap(),
        "Long2.File"
    );
    let oversized = "p".repeat(1100);
    assert_eq!(
        resolve("large-result", |_| Ok::<_, &str>(oversized.clone())).unwrap(),
        oversized
    );
    assert_eq!(
        resolve("large-result", |_| Ok::<_, &str>("NewSmall.File".into())).unwrap(),
        "NewSmall.File"
    );
    // 監視対象を開けない場合、結果を繰り返し照会し、期限なしで保持しない。
    release();
    *state() = Some(Cache::new(&[(
        RegKey::predef(HKEY_CURRENT_USER),
        &format!("{name}\\missing"),
    )]));
    assert_eq!(
        resolve("uncached", |_| Ok::<_, &str>("A.File".into())).unwrap(),
        "A.File"
    );
    assert_eq!(
        resolve("uncached", |_| Ok::<_, &str>("B.File".into())).unwrap(),
        "B.File"
    );
    // 再登録時のキー消失でも同じくキャッシュを捨て、元の照会を続ける。
    release();
    *state() = Some(Cache::new(&[(
        RegKey::predef(HKEY_CURRENT_USER),
        &user_path,
    )]));
    resolve("deleted", |_| Ok::<_, &str>("BeforeDelete.File".into())).unwrap();
    drop(user);
    root.delete_subkey_all("user").unwrap();
    assert_eq!(
        resolve("deleted", |_| Ok::<_, &str>("AfterDelete.File".into())).unwrap(),
        "AfterDelete.File"
    );
    assert_eq!(
        resolve("deleted", |_| Ok::<_, &str>("StillUncached.File".into())).unwrap(),
        "StillUncached.File"
    );
    release();
    // アンロード可否の確認時に、監視用のOSハンドルが残らない。
    let before = handles();
    *state() = Some(Cache::new(&[(
        RegKey::predef(HKEY_CURRENT_USER),
        &machine_path,
    )]));
    resolve("unload", |_| Ok::<_, &str>("Loaded.File".into())).unwrap();
    assert!(handles() > before);
    assert_eq!(
        super::super::DllCanUnloadNow(),
        windows::Win32::Foundation::S_OK
    );
    assert!(handles() <= before);
}
