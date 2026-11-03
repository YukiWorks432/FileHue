use std::{
    fs,
    io::Read,
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use xtask::process;

fn fixture(mode: &str, value: &Path, ready: &Path) -> Command {
    let mut command = Command::new("pwsh");
    command
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/process.ps1"))
        .arg(mode)
        .arg(value)
        .arg(ready);
    command
}
fn unlocked(path: &Path) {
    // 終了通知の直後にファイルシステム側のハンドル解放が遅れる場合も、期限内に確認する。
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut file = loop {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
        {
            Ok(file) => break file,
            Err(error) if error.raw_os_error() == Some(32) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => panic!("所有子孫のロックが解除されません: {error}"),
        }
    };
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    assert_eq!(text, "x");
}

#[test]
fn cargo_proxy_keeps_its_invocation_name() {
    let logs = tempfile::tempdir().unwrap();
    let bytes = process::checked(
        Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
                "--locked",
            ])
            .current_dir(xtask::root()),
        Duration::from_secs(30),
        "Cargoの起動名",
        logs.path(),
    )
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let actual = Path::new(metadata["workspace_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert_eq!(actual, xtask::root().canonicalize().unwrap());
}

#[test]
fn unicode_arguments_environment_large_output_and_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    let value = Path::new("引数 with space \\\"quoted\\\" and trailing\\");
    let mut command = fixture("output", value, Path::new("unused"));
    command
        .env("WFI_FIXTURE_TEXT", "環境")
        .env_remove("WFI_FIXTURE_REMOVE");
    let result = process::run(
        &mut command,
        Duration::from_secs(20),
        "unicode",
        &root.path().join("normal"),
    )
    .unwrap();
    assert_eq!(result.returncode, 0, "{}", process::detail(&result));
    assert!(!result.timed_out);
    let expected = format!("環境|{}|{}", value.display(), "X".repeat(200000));
    assert_eq!(
        fs::read(root.path().join("normal/stdout.log")).unwrap(),
        expected.as_bytes()
    );
    assert_eq!(
        fs::read(root.path().join("normal/stderr.log")).unwrap(),
        vec![b'Y'; 200000]
    );
    let result = process::run(
        &mut fixture("failure", Path::new("unused"), Path::new("unused")),
        Duration::from_secs(20),
        "exit",
        &root.path().join("exit"),
    )
    .unwrap();
    assert_eq!(result.returncode, 23);
    assert!(!result.timed_out);
    let error = process::checked(
        &mut fixture("failure", Path::new("unused"), Path::new("unused")),
        Duration::from_secs(20),
        "failure",
        &root.path().join("checked"),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("failure-marker"));
}

#[test]
fn timeout_reaps_descendants_and_retains_partial_logs_without_touching_other_processes() {
    let root = tempfile::tempdir().unwrap();
    let held = root.path().join("held");
    let ready = root.path().join("ready");
    let mut unrelated = Command::new("pwsh")
        .args(["-NoProfile", "-Command", "Start-Sleep 60"])
        .creation_flags(0x08000000)
        .spawn()
        .unwrap();
    let began = Instant::now();
    let result = process::run(
        &mut fixture("timeout", &held, &ready),
        Duration::from_secs(4),
        "timeout",
        &root.path().join("logs"),
    );
    let alive = unrelated.try_wait().unwrap().is_none();
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    let result = result.unwrap();
    assert!(result.timed_out, "{}", process::detail(&result));
    assert!(began.elapsed() < Duration::from_secs(12));
    assert!(ready.exists());
    assert!(alive);
    assert_eq!(
        fs::read_to_string(root.path().join("logs/stdout.log")).unwrap(),
        "準備完了"
    );
    assert_eq!(
        fs::read(root.path().join("logs/stderr.log")).unwrap(),
        b"partial-error"
    );
    unlocked(&held);
}

#[test]
fn successful_parent_also_reaps_lingering_descendants() {
    let root = tempfile::tempdir().unwrap();
    let held = root.path().join("held");
    let ready = root.path().join("ready");
    let result = process::run(
        &mut fixture("linger", &held, &ready),
        Duration::from_secs(20),
        "linger",
        &root.path().join("logs"),
    )
    .unwrap();
    assert_eq!(result.returncode, 0, "{}", process::detail(&result));
    assert!(!result.timed_out);
    assert!(ready.exists());
    unlocked(&held);
}
