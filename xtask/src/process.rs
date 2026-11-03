//! 検証用のプロセスだけを専用ジョブへ入れ、期限とログを管理する。
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Security::{AppLocker::*, *},
        System::{JobObjects::*, Threading::*},
    },
    core::{PCWSTR, PWSTR},
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct Attributes {
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    _storage: Vec<usize>,
}
impl Attributes {
    fn new(handles: &mut [HANDLE]) -> Result<Self> {
        unsafe {
            let mut size = 0;
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
            ensure!(size > 0, "継承ハンドルのサイズ取得に失敗しました");
            let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)?;
            let result = Self {
                list,
                _storage: storage,
            };
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast()),
                size_of_val(handles),
                None,
                None,
            )?;
            Ok(result)
        }
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    ensure!(!value.contains(&0), "引数・環境にNULを含められません");
    value.push(0);
    Ok(value)
}

// CreateProcessへ渡すCRT形式の引用。シェルの構文は解釈しない。
fn quote(value: &OsStr) -> Result<Vec<u16>> {
    let data = wide(value)?;
    let content = &data[..data.len() - 1];
    if !content.is_empty() && !content.iter().any(|c| matches!(*c, 9 | 32 | 34)) {
        return Ok(content.to_vec());
    }
    let mut out = vec![34];
    let mut slash = 0;
    for &c in &data[..data.len() - 1] {
        if c == 92 {
            slash += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            92,
            if c == 34 { slash * 2 + 1 } else { slash },
        ));
        slash = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(92, slash * 2));
    out.push(34);
    Ok(out)
}

fn executable(program: &OsStr) -> Result<PathBuf> {
    let path = Path::new(program);
    let candidates = if path.is_absolute() || path.components().count() > 1 {
        vec![path.to_path_buf()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|base| {
                base.join(if path.extension().is_none() {
                    PathBuf::from(format!("{}.exe", program.to_string_lossy()))
                } else {
                    path.to_path_buf()
                })
            })
            .collect()
    };
    for path in candidates {
        if path.is_file() {
            // Rustupなどは起動名で処理を選ぶため、リンク先の名前へ置き換えない。
            return Ok(std::path::absolute(path)?);
        }
    }
    bail!(
        "実行ファイルが見つかりません: {}",
        program.to_string_lossy()
    )
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub returncode: u32,
    pub timed_out: bool,
    pub elapsed: f64,
    pub pid: u32,
    pub logs: PathBuf,
}

fn stop(job: HANDLE) -> Result<()> {
    unsafe {
        TerminateJobObject(job, 1)?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut state = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicAccountingInformation,
                (&mut state as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of_val(&state) as u32,
                None,
            )?;
        }
        if state.ActiveProcesses == 0 {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "所有する試験プロセス群の回収が5秒以内に完了しませんでした"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn run(command: &mut Command, timeout: Duration, label: &str, logs: &Path) -> Result<Report> {
    run_inner(command, timeout, label, logs, None)
        .with_context(|| format!("検証プロセスの制御失敗: {label}, logs={}", logs.display()))
}

/// 管理者からの試験起動だけを、同じユーザーの標準権限へ制限する。
pub fn run_as_standard_user(
    command: &mut Command,
    timeout: Duration,
    label: &str,
    logs: &Path,
) -> Result<Report> {
    let token = standard_user_token().context("試験用の標準ユーザートークンを用意できません")?;
    run_inner(
        command,
        timeout,
        label,
        logs,
        token.as_ref().map(|token| token.0),
    )
    .with_context(|| {
        format!(
            "標準ユーザーの試験制御失敗: {label}, logs={}",
            logs.display()
        )
    })
}

fn standard_user_token() -> Result<Option<Handle>> {
    unsafe {
        let mut current = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut current,
        )?;
        let current = Handle(current);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0;
        GetTokenInformation(
            current.0,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size_of_val(&elevation) as u32,
            &mut size,
        )?;
        if elevation.TokenIsElevated == 0 {
            return Ok(None);
        }

        let mut level = SAFER_LEVEL_HANDLE::default();
        SaferCreateLevel(
            SAFER_SCOPEID_USER,
            SAFER_LEVELID_NORMALUSER,
            SAFER_LEVEL_OPEN,
            &mut level,
            None,
        )
        .context("SAFERの標準ユーザーレベルを開けません")?;
        let mut token = HANDLE::default();
        let computed = SaferComputeTokenFromLevel(
            level,
            Some(current.0),
            &mut token,
            SAFER_COMPUTE_TOKEN_FROM_LEVEL_FLAGS(0),
            None,
        );
        let _ = SaferCloseLevel(level);
        computed.context("SAFERの制限トークンを作成できません")?;
        let token = Handle(token);

        // 整合性ラベルだけでは管理者グループを除去できないため、SAFERと併用する。
        let mut sid_storage = [0u32; SECURITY_MAX_SID_SIZE as usize / size_of::<u32>()];
        let mut sid_size = size_of_val(&sid_storage) as u32;
        let sid = PSID(sid_storage.as_mut_ptr().cast());
        CreateWellKnownSid(WinMediumLabelSid, None, Some(sid), &mut sid_size)?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY as u32,
            },
        };
        SetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            (&label as *const TOKEN_MANDATORY_LABEL).cast(),
            size_of_val(&label) as u32 + sid_size,
        )
        .context("試験トークンをMedium整合性へ設定できません")?;
        Ok(Some(token))
    }
}

fn run_inner(
    command: &mut Command,
    timeout: Duration,
    label: &str,
    logs: &Path,
    token: Option<HANDLE>,
) -> Result<Report> {
    ensure!(
        !timeout.is_zero() && timeout.as_millis() < u32::MAX as u128,
        "待機期限は正の秒数で指定してください"
    );
    fs::create_dir_all(logs)?;
    let args: Vec<_> = command.get_args().map(OsStr::to_os_string).collect();
    fs::write(
        logs.join("child.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "command":std::iter::once(command.get_program().to_string_lossy()).chain(args.iter().map(|s|s.to_string_lossy())).collect::<Vec<_>>(),
            "cwd":command.get_current_dir(), "timeout":timeout.as_secs_f64(), "label":label,
        }))?,
    )?;
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(logs.join("stdout.log"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(logs.join("stderr.log"))?;
    let input = File::open("NUL")?;
    let mut handles = [
        HANDLE(input.as_raw_handle()),
        HANDLE(stdout.as_raw_handle()),
        HANDLE(stderr.as_raw_handle()),
    ];
    // 複製したハンドルだけを継承リストへ渡す。元の非継承ファイルハンドルは保持する。
    let mut inherited = Vec::new();
    for handle in handles.iter_mut() {
        let mut copy = HANDLE::default();
        unsafe {
            windows::Win32::Foundation::DuplicateHandle(
                GetCurrentProcess(),
                *handle,
                GetCurrentProcess(),
                &mut copy,
                0,
                true,
                windows::Win32::Foundation::DUPLICATE_SAME_ACCESS,
            )?;
        }
        inherited.push(Handle(copy));
        *handle = copy;
    }
    let attributes = Attributes::new(&mut handles)?;
    let path = executable(command.get_program())?;
    let application = wide(path.as_os_str())?;
    let mut line = quote(path.as_os_str())?;
    for arg in &args {
        line.push(32);
        line.extend(quote(arg)?);
    }
    line.push(0);
    ensure!(line.len() <= 32767, "コマンド行が長すぎます");
    let mut environment: BTreeMap<String, (OsString, OsString)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().to_uppercase(), (k, v)))
        .collect();
    for (key, value) in command.get_envs() {
        let normalized = key.to_string_lossy().to_uppercase();
        if let Some(value) = value {
            environment.insert(normalized, (key.to_owned(), value.to_owned()));
        } else {
            environment.remove(&normalized);
        }
    }
    let mut block = Vec::new();
    for (key, value) in environment.values() {
        let mut entry = key.clone();
        entry.push("=");
        entry.push(value);
        block.extend(wide(&entry)?);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    let cwd = command
        .get_current_dir()
        .map(|p| wide(p.as_os_str()))
        .transpose()?;
    let began = Instant::now();
    unsafe {
        let job = Handle(CreateJobObjectW(None, PCWSTR::null())?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        )?;
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW,
                wShowWindow: 0,
                hStdInput: handles[0],
                hStdOutput: handles[1],
                hStdError: handles[2],
                ..Default::default()
            },
            lpAttributeList: attributes.list,
        };
        let mut info = PROCESS_INFORMATION::default();
        if let Some(token) = token {
            CreateProcessAsUserW(
                Some(token),
                PCWSTR(application.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                true,
                CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                Some(block.as_ptr().cast()),
                cwd.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
                &startup.StartupInfo,
                &mut info,
            )
            .context("制限トークンで試験プロセスを生成できません")?;
        } else {
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                true,
                CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                Some(block.as_ptr().cast()),
                cwd.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
                &startup.StartupInfo,
                &mut info,
            )?;
        }
        let process = Handle(info.hProcess);
        let thread = Handle(info.hThread);
        // 対象の命令を一つも実行させる前に所有ジョブへ割り当てる。
        if let Err(error) = AssignProcessToJobObject(job.0, process.0) {
            let killed = TerminateProcess(process.0, 1);
            let _ = WaitForSingleObject(process.0, 5000);
            killed?;
            return Err(error.into());
        }
        ensure!(
            ResumeThread(thread.0) != u32::MAX,
            "試験プロセスを再開できません"
        );
        eprintln!(
            "[開始] {label} pid={} timeout={timeout:?} logs={}",
            info.dwProcessId,
            logs.display()
        );
        let wait = WaitForSingleObject(process.0, timeout.as_millis() as u32);
        let timed_out = wait == WAIT_TIMEOUT;
        let mut code = 1;
        if wait == WAIT_OBJECT_0 {
            GetExitCodeProcess(process.0, &mut code)?;
        }
        let cleanup = stop(job.0);
        cleanup
            .with_context(|| format!("試験プロセスを回収できません: logs={}", logs.display()))?;
        ensure!(
            wait == WAIT_OBJECT_0 || timed_out,
            "試験プロセスの待機に失敗しました"
        );
        if timed_out {
            GetExitCodeProcess(process.0, &mut code)?;
        }
        let result = Report {
            returncode: code,
            timed_out,
            elapsed: began.elapsed().as_secs_f64(),
            pid: info.dwProcessId,
            logs: logs.to_path_buf(),
        };
        fs::write(
            logs.join("result.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        eprintln!(
            "[終了] {label} pid={} elapsed={:.2}s code={code} timeout={timed_out}",
            result.pid, result.elapsed
        );
        Ok(result)
    }
}

pub fn detail(result: &Report) -> String {
    let mut text = format!(
        "pid={} elapsed={:.2}s logs={}\n",
        result.pid,
        result.elapsed,
        result.logs.display()
    );
    for name in ["stdout.log", "stderr.log"] {
        let read = || -> std::io::Result<Vec<u8>> {
            let mut file = File::open(result.logs.join(name))?;
            let len = file.metadata()?.len();
            file.seek(SeekFrom::Start(len.saturating_sub(65536)))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        if let Ok(bytes) = read() {
            text.push_str(&format!("{name}:\n{}\n", String::from_utf8_lossy(&bytes)));
        }
    }
    text
}

pub fn checked(
    command: &mut Command,
    timeout: Duration,
    label: &str,
    logs: &Path,
) -> Result<Vec<u8>> {
    let result = run(command, timeout, label, logs)?;
    ensure!(
        !result.timed_out && result.returncode == 0,
        "検証プロセスが失敗しました（期限超過={}）\n{}",
        result.timed_out,
        detail(&result)
    );
    Ok(fs::read(logs.join("stdout.log"))?)
}
