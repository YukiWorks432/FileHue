#[path = "../../../test-support/process.rs"]
#[allow(dead_code)] // この試験は既定期限の入口を使わず、20秒を指定する。
mod test_process;
use std::{process::Command, time::Duration};
use test_process::CommandOutput;

#[test]
fn normal_gui_executable_initializes_and_exits_cleanly() {
    let profile = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_file-icons-manager"))
        .arg("--smoke-test")
        .env("LOCALAPPDATA", profile.path())
        .test_output_with_timeout(Duration::from_secs(20))
        .unwrap();
    assert!(
        output.status.success(),
        "GUI failed: {:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
