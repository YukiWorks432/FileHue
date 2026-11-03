//! 実DLLを登録せずに測る。親プロセスが専用プロファイルを作り、毎回新規の子で測定する。
use anyhow::{Context, Result, ensure};
use file_icons_core::ImportedPack;
use file_icons_windows::{IconLocation, Settings};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::windows::fs::OpenOptionsExt;
use std::{
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{FILETIME, FreeLibrary, HMODULE},
        System::{
            Com::*, LibraryLoader::*, ProcessStatus::*, Threading::*,
            WindowsProgramming::QueryThreadCycleTime,
        },
        UI::{
            Shell::*,
            WindowsAndMessaging::{DestroyIcon, HICON},
        },
    },
    core::{GUID, HRESULT, Interface, PCSTR, PCWSTR},
};

const ROUNDS: usize = 8;
const WARM: usize = 256;
type GetClassObject =
    unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

struct Com;
impl Com {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn factory(dll: &Path) -> Result<(Library, IClassFactory)> {
    let library = Library(unsafe { LoadLibraryW(PCWSTR(wide(dll).as_ptr()))? });
    let address = unsafe { GetProcAddress(library.0, PCSTR(c"DllGetClassObject".as_ptr().cast())) }
        .context("class export missing")?;
    let get: GetClassObject = unsafe { std::mem::transmute(address) };
    let mut raw = std::ptr::null_mut();
    unsafe {
        get(&file_icons_windows::CLSID, &IClassFactory::IID, &mut raw).ok()?;
    }
    Ok((library, unsafe { IClassFactory::from_raw(raw) }))
}

fn memory() -> Result<Value> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )?;
    }
    Ok(json!({"working_set_bytes":counters.WorkingSetSize, "private_bytes":counters.PrivateUsage}))
}

fn summary(values: &[u64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    assert!(!sorted.is_empty());
    let percentile = |numerator: usize| sorted[(sorted.len() * numerator).div_ceil(100) - 1];
    json!({"count":sorted.len(), "p50_ns":percentile(50), "p95_ns":percentile(95), "max_ns":sorted.last().unwrap()})
}

fn query(factory: &IClassFactory, filename: &[u16]) -> Result<(Vec<u16>, i32)> {
    let persist: IPersistFile = unsafe { factory.CreateInstance(None)? };
    query_persist(persist, filename, true)
}

fn query_persist(
    persist: IPersistFile,
    filename: &[u16],
    normalize_flags: bool,
) -> Result<(Vec<u16>, i32)> {
    unsafe {
        persist.Load(PCWSTR(filename.as_ptr()), STGM_READ)?;
    }
    location(&persist, normalize_flags)
}

fn location(persist: &IPersistFile, normalize_flags: bool) -> Result<(Vec<u16>, i32)> {
    let extract: IExtractIconW = persist.cast()?;
    let mut output = vec![0u16; 4096];
    let (mut index, mut flags) = (0, 0);
    unsafe {
        extract.GetIconLocation(0, &mut output, &mut index, &mut flags)?;
    }
    ensure!(output[0] != 0, "empty icon location");
    if normalize_flags {
        ensure!(
            flags & GIL_PERCLASS == 0 && flags & GIL_PERINSTANCE != 0,
            "unsafe class icon cache flag"
        );
    }
    output.truncate(
        output
            .iter()
            .position(|v| *v == 0)
            .context("unterminated location")?
            + 1,
    );
    Ok((output, index))
}

struct ThreadUsage {
    kernel_ns: u64,
    user_ns: u64,
    cycles: u64,
}
fn thread_usage() -> Result<ThreadUsage> {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let mut cycles = 0;
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )?;
        QueryThreadCycleTime(GetCurrentThread(), &mut cycles)?;
    }
    let ns = |t: FILETIME| ((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) * 100;
    Ok(ThreadUsage {
        kernel_ns: ns(kernel),
        user_ns: ns(user),
        cycles,
    })
}

struct MeasuredQuery {
    answer: (Vec<u16>, i32),
    wall_ns: u64,
    diagnostic: Option<Value>,
}
fn measured_query(
    factory: &IClassFactory,
    input: &[u16],
    diagnostic: bool,
) -> Result<MeasuredQuery> {
    if !diagnostic {
        let start = Instant::now();
        let answer = query(factory, input)?;
        return Ok(MeasuredQuery {
            answer,
            wall_ns: nanos(start),
            diagnostic: None,
        });
    }
    // CPU計数の取得は経過時間の外に置く。CPU計数の区間は経過時間をわずかに包含する。
    let before = thread_usage()?;
    let start = Instant::now();
    let phase = Instant::now();
    let persist: IPersistFile = unsafe { factory.CreateInstance(None)? };
    let create_ns = nanos(phase);
    let phase = Instant::now();
    unsafe {
        persist.Load(PCWSTR(input.as_ptr()), STGM_READ)?;
    }
    let load_ns = nanos(phase);
    let phase = Instant::now();
    let answer = location(&persist, true)?;
    let get_location_ns = nanos(phase);
    let phase = Instant::now();
    drop(persist);
    let release_ns = nanos(phase);
    let wall_ns = nanos(start);
    let after = thread_usage()?;
    ensure!(
        after.kernel_ns >= before.kernel_ns
            && after.user_ns >= before.user_ns
            && after.cycles >= before.cycles,
        "thread counters moved backwards"
    );
    let diagnostic = json!({"wall_ns":wall_ns,"create_ns":create_ns,"load_ns":load_ns,
        "get_location_ns":get_location_ns,"release_ns":release_ns,
        "kernel_ns":after.kernel_ns-before.kernel_ns,"user_ns":after.user_ns-before.user_ns,
        "thread_cycles":after.cycles-before.cycles});
    Ok(MeasuredQuery {
        answer,
        wall_ns,
        diagnostic: Some(diagnostic),
    })
}

fn extract(location: &[u16], index: i32) -> Result<()> {
    let (mut large, mut small) = (HICON::default(), HICON::default());
    let count = unsafe {
        ExtractIconExW(
            PCWSTR(location.as_ptr()),
            index,
            Some(&mut large),
            Some(&mut small),
            1,
        )
    };
    let valid = matches!(count, 1 | 2) && !large.is_invalid() && !small.is_invalid();
    unsafe {
        if !large.is_invalid() {
            DestroyIcon(large)?;
        }
        if !small.is_invalid() {
            DestroyIcon(small)?;
        }
    }
    ensure!(valid, "icon extraction failed: count={count}");
    Ok(())
}

fn nanos(start: Instant) -> u64 {
    start.elapsed().as_nanos().try_into().unwrap()
}

fn child(args: &[String]) -> Result<()> {
    ensure!(
        std::env::var("WFI_PERF_CHILD").as_deref() == Ok("1"),
        "child profile required"
    );
    ensure!(args.len() == 7, "child arguments required");
    let diagnostic = match args[6].as_str() {
        "1" => true,
        "0" => false,
        _ => anyhow::bail!("invalid diagnostic argument"),
    };
    let _com = Com::new()?;
    let start = Instant::now();
    let (_library, factory) = factory(Path::new(&args[1]))?;
    let activation = nanos(start);
    let input = wide(Path::new(&args[2]));
    let expected = wide(Path::new(&args[3]));
    let expected_index: i32 = args[4].parse()?;
    let before = memory()?;
    let first_measurement = measured_query(&factory, &input, diagnostic)?;
    let first_ns = first_measurement.wall_ns;
    let first = first_measurement.answer;
    ensure!(
        first == (expected.clone(), expected_index),
        "first query returned wrong icon"
    );
    // 計測の前後に、正しい素材を返すことを確認する。検証自体は計測へ含めない。
    let mut location = Vec::with_capacity(WARM);
    let mut query_diagnostics = Vec::with_capacity(if diagnostic { WARM } else { 0 });
    let mut extraction = Vec::with_capacity(WARM);
    let mut association = Vec::with_capacity(WARM);
    let mut native = Vec::with_capacity(WARM);
    let extension = Path::new(&args[2])
        .extension()
        .context("extension missing")?
        .to_str()
        .context("extension invalid")?;
    for _ in 0..WARM {
        let measured = measured_query(&factory, &input, diagnostic)?;
        let answer = measured.answer;
        location.push(measured.wall_ns);
        if let Some(detail) = measured.diagnostic {
            query_diagnostics.push(detail);
        }
        ensure!(answer == first, "warm query returned wrong icon");
        let start = Instant::now();
        extract(&answer.0, answer.1)?;
        extraction.push(nanos(start));
        let start = Instant::now();
        std::hint::black_box(file_icons_windows::association(extension)?);
        association.push(nanos(start));
        if args[5] == "native_delegation" {
            let start = Instant::now();
            let persist: IPersistFile =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
            let direct = query_persist(persist, &input, false)?;
            native.push(nanos(start));
            ensure!(direct == first, "native baseline returned wrong icon");
        }
    }
    let warm_memory = memory()?;
    let mut settings = file_icons_windows::load()?;
    settings.language = "en".into();
    file_icons_windows::save(&settings)?;
    let reloaded = measured_query(&factory, &input, diagnostic)?;
    let answer = reloaded.answer;
    let reload = reloaded.wall_ns;
    ensure!(answer == first, "reload returned wrong icon");
    println!(
        "{}",
        json!({"activation_ns":activation, "first_query_ns":first_ns,
        "reload_query_ns":reload, "location_ns":location, "extraction_ns":extraction,
        "first_query_diagnostic":first_measurement.diagnostic, "reload_query_diagnostic":reloaded.diagnostic,
        "query_diagnostics":query_diagnostics,
        "association_ns":association,"direct_native_ns":native,
        "memory_before_settings":before, "memory_after_warm":warm_memory,
        "settings_bytes":fs::metadata(file_icons_windows::home()?.join("settings.json"))?.len(),
        "recovery_bytes":fs::metadata(file_icons_windows::home()?.join("recovery.json"))?.len()})
    );
    Ok(())
}

fn make_link(path: &Path, icon: &Path) -> Result<()> {
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
    unsafe {
        link.SetIconLocation(PCWSTR(wide(icon).as_ptr()), 0)?;
        link.cast::<IPersistFile>()?
            .Save(PCWSTR(wide(path).as_ptr()), true)?;
    }
    Ok(())
}

fn samples(reports: &[Value], field: &str) -> Vec<u64> {
    reports
        .iter()
        .map(|report| report[field].as_u64().unwrap())
        .collect()
}

fn parent(args: &[String], diagnostic: bool) -> Result<()> {
    ensure!(
        args.len() >= 3,
        "usage: measure_shell [--diagnostics] <DLL> <report.json> <unpacked-extension>..."
    );
    let _com = Com::new()?;
    let dll = PathBuf::from(&args[0]).canonicalize()?;
    // 全測定中に対象DLLへの書き込み・置き換えを拒否し、同じバイト列を測る。
    let _dll_guard = fs::OpenOptions::new().read(true).share_mode(1).open(&dll)?;
    let dll_hash = hex::encode(Sha256::digest(fs::read(&dll)?));
    let report_path = PathBuf::from(&args[1]);
    ensure!(
        !report_path.exists(),
        "report already exists; preserve measurements"
    );
    let workspace = tempfile::tempdir()?;
    let mut packs = Vec::<ImportedPack>::new();
    for root in &args[2..] {
        let root = Path::new(root);
        for choice in file_icons_core::discover(root)? {
            packs.push(file_icons_core::import(
                root,
                &choice.id,
                &workspace.path().join("packs"),
            )?);
        }
    }
    let active = &packs[0];
    let suffix = format!("wfi-perf-{}", std::process::id());
    let class = file_icons_windows::association(&suffix)?;
    ensure!(
        class == format!(".{suffix}"),
        "benchmark extension is already associated"
    );
    let icon = active
        .theme
        .resolve("package.json")
        .context("theme must define package.json")?
        .to_path_buf();
    let named = workspace.path().join("package.json");
    fs::write(&named, b"{}")?;
    let extension = workspace.path().join("sample.json");
    fs::write(&extension, b"{}")?;
    let fallback = workspace.path().join(format!("unthemed.{suffix}"));
    fs::write(&fallback, b"unchanged")?;
    let linked = workspace.path().join(format!("linked.{suffix}"));
    make_link(&linked, &icon)?;
    let ordinary = active
        .theme
        .resolve("sample.json")
        .context("theme must define JSON")?
        .to_path_buf();
    let system = PathBuf::from(std::env::var_os("SystemRoot").context("SystemRoot missing")?)
        .join("System32/shell32.dll");
    let plan = file_icons_windows::plan(active, &dll)?;
    // 計画を読むだけで実行しない。実際と同量の復元データを専用設定へ含める。
    let mut settings = Settings {
        active: Some(active.id.clone()),
        packs: packs.clone(),
        journal: plan.changes,
        created_keys: plan.created_keys,
        fallbacks: plan.fallbacks,
        original_handlers: plan.original_handlers,
        ..Default::default()
    };
    settings.fallbacks.insert(
        class.clone(),
        IconLocation {
            path: system.to_string_lossy().into(),
            index: 0,
        },
    );
    let basic_settings = settings.clone();
    let mut output = serde_json::Map::new();
    for (name, file, expected, delegated) in [
        ("filename", &named, &icon, false),
        ("extension", &extension, &ordinary, false),
        ("static_fallback", &fallback, &system, false),
        ("native_delegation", &linked, &icon, true),
    ] {
        let mut reports = Vec::new();
        for _ in 0..ROUNDS {
            let profile = tempfile::tempdir_in(workspace.path())?;
            let home = profile.path().join("WindowsFileIconsDev");
            fs::create_dir(&home)?;
            settings = basic_settings.clone();
            if delegated {
                settings.original_handlers.insert(
                    class.clone(),
                    "{00021401-0000-0000-C000-000000000046}".into(),
                );
            }
            file_icons_windows::save_to(&settings, &home.join("settings.json"))?;
            let recovery = json!({"version":1,"active":settings.active,"fallbacks":settings.fallbacks,
                "original_handlers":settings.original_handlers,"journal":settings.journal,"created_keys":settings.created_keys});
            fs::write(
                home.join("recovery.json"),
                serde_json::to_vec_pretty(&recovery)?,
            )?;
            let result = Command::new(std::env::current_exe()?)
                .arg("--child")
                .arg(&dll)
                .arg(file)
                .arg(expected)
                .arg("0")
                .arg(name)
                .arg(if diagnostic { "1" } else { "0" })
                .env("LOCALAPPDATA", profile.path())
                .env("WFI_PERF_CHILD", "1")
                .output()?;
            ensure!(
                result.status.success(),
                "{name} child failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            reports.push(serde_json::from_slice::<Value>(&result.stdout)?);
        }
        let location = reports
            .iter()
            .flat_map(|r| {
                r["location_ns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect::<Vec<_>>();
        let extraction = reports
            .iter()
            .flat_map(|r| {
                r["extraction_ns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect::<Vec<_>>();
        let association = reports
            .iter()
            .flat_map(|r| {
                r["association_ns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect::<Vec<_>>();
        let native = reports
            .iter()
            .flat_map(|r| {
                r["direct_native_ns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect::<Vec<_>>();
        output.insert(name.into(), json!({"association_resolution":summary(&association),
            "direct_native_handler":if native.is_empty() { None } else { Some(summary(&native)) },
            "activation":summary(&samples(&reports,"activation_ns")),
            "first_query":summary(&samples(&reports,"first_query_ns")), "reload_query":summary(&samples(&reports,"reload_query_ns")),
            "warm_location":summary(&location), "icon_extraction_and_release":summary(&extraction), "raw":reports}));
    }
    let value = json!({"format":2,"diagnostics":diagnostic,"scope":"direct DLL COM calls; no Explorer registration; process-cold, OS cache uncontrolled",
        "dll_sha256":dll_hash, "dll":dll,
        "rounds_per_case":ROUNDS,"warm_requests_per_round":WARM,
        "settings_bytes":serde_json::to_vec_pretty(&basic_settings)?.len(),
        "registration_changes":basic_settings.journal.len(),
        "packs":packs.iter().map(|p|json!({"id":p.id,"version":p.version,"filename_rules":p.theme.rules().files.len(),"extension_rules":p.theme.rules().extensions.len()})).collect::<Vec<_>>(),
        "cases":output});
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    // 既存の報告は上書きしない。測定用一時素材は終了時に除去する。
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report_path)?;
    file.write_all(&serde_json::to_vec_pretty(&value)?)?;
    println!(
        "{}",
        json!({"report":report_path,"cases":value["cases"].as_object().unwrap().iter().map(|(k,v)| (k.clone(),json!({"cold":v["first_query"],"warm":v["warm_location"],"extract":v["icon_extraction_and_release"]}))).collect::<serde_json::Map<_,_>>()})
    );
    Ok(())
}

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a == "--child") {
        child(&args)
    } else if args.first().is_some_and(|a| a == "--diagnostics") {
        parent(&args[1..], true)
    } else {
        parent(&args, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantiles_use_nearest_rank_and_keep_the_tail() {
        let values = (1..=100).rev().collect::<Vec<_>>();
        let result = summary(&values);
        assert_eq!(result["p50_ns"], 50);
        assert_eq!(result["p95_ns"], 95);
        assert_eq!(result["max_ns"], 100);
        assert_eq!(summary(&[41])["p95_ns"], 41);
    }
}
