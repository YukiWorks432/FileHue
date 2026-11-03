// Rustの試験とxtaskで、同じ期限・ログ・所有ジョブの境界を使う。
use std::{
    fs, io,
    os::windows::process::ExitStatusExt,
    path::Path,
    process::{Command, Output},
    time::Duration,
};

pub trait CommandOutput {
    fn test_output(&mut self) -> io::Result<Output> {
        self.test_output_with_timeout(Duration::from_secs(180))
    }
    fn test_output_with_timeout(&mut self, timeout: Duration) -> io::Result<Output>;
    // 複数の試験実行ファイルへ共有する補助で、Shellの試験だけが使う。
    #[allow(dead_code)]
    fn test_output_as_standard_user(&mut self, timeout: Duration) -> io::Result<Output>;
}
impl CommandOutput for Command {
    fn test_output_with_timeout(&mut self, timeout: Duration) -> io::Result<Output> {
        capture(self, timeout, false)
    }
    fn test_output_as_standard_user(&mut self, timeout: Duration) -> io::Result<Output> {
        capture(self, timeout, true)
    }
}

fn capture(command: &mut Command, timeout: Duration, standard_user: bool) -> io::Result<Output> {
    let store = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/processes");
    fs::create_dir_all(&store)?;
    let logs = tempfile::Builder::new()
        .prefix("rust-probe-")
        .tempdir_in(store)?;
    let label = format!(
        "{} {}",
        command.get_program().to_string_lossy(),
        command
            .get_args()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let result = if standard_user {
        xtask::process::run_as_standard_user(command, timeout, &label, logs.path())
    } else {
        xtask::process::run(command, timeout, &label, logs.path())
    };
    match result {
        Ok(result) if !result.timed_out => Ok(Output {
            status: std::process::ExitStatus::from_raw(result.returncode),
            stdout: fs::read(logs.path().join("stdout.log"))?,
            stderr: fs::read(logs.path().join("stderr.log"))?,
        }),
        Ok(result) => {
            let detail = xtask::process::detail(&result);
            let path = logs.keep();
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("試験プロセスの期限超過: logs={}\n{detail}", path.display()),
            ))
        }
        Err(error) => {
            let path = logs.keep();
            Err(io::Error::other(format!(
                "試験プロセスの制御失敗: {error:#}, logs={}",
                path.display()
            )))
        }
    }
}
