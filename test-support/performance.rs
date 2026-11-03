//! 専用の計測プロセスで使う。計数の費用を含むため、同じ構成の変更前後だけを比較する。
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};

type SectionTotals = BTreeMap<&'static str, (u64, u64, u64)>;
thread_local! {
    static SECTIONS: RefCell<Option<SectionTotals>> = const { RefCell::new(None) };
}

/// 区間は包含時間。親区間と子区間を加算して全体時間にしない。
#[allow(dead_code, reason = "共有計測器のうちGUIだけが内訳区間を使う")]
pub struct Section {
    name: &'static str,
    before: Option<(Instant, u64)>,
}

#[allow(dead_code, reason = "共有計測器のうちGUIだけが内訳区間を使う")]
pub fn section(name: &'static str) -> Section {
    Section {
        name,
        before: SECTIONS.with(|s| {
            s.borrow()
                .as_ref()
                .map(|_| (Instant::now(), BYTES.load(Relaxed)))
        }),
    }
}

impl Drop for Section {
    fn drop(&mut self) {
        if let Some((began, bytes)) = self.before {
            let elapsed = began.elapsed().as_nanos() as u64;
            let allocated = BYTES.load(Relaxed) - bytes;
            SECTIONS.with(|s| {
                if let Some(sections) = s.borrow_mut().as_mut() {
                    let entry = sections.entry(self.name).or_default();
                    entry.0 += elapsed;
                    entry.1 += allocated;
                    entry.2 += 1;
                }
            });
        }
    }
}

struct Allocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

fn allocated(size: usize) {
    ALLOCATIONS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size as u64, Relaxed);
    let live = LIVE.fetch_add(size as u64, Relaxed) + size as u64;
    PEAK.fetch_max(live, Relaxed);
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as u64, Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Relaxed);
            allocated(size);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

pub fn live_bytes() -> u64 {
    LIVE.load(Relaxed)
}

/// 計時外の準備は呼び出し側で行う。戻り値で出力の同一性も確認する。
pub fn measure(name: &str, mut run: impl FnMut() -> u64) -> serde_json::Value {
    let expected = run();
    for _ in 0..2 {
        assert_eq!(run(), expected);
    }
    let mut samples = Vec::with_capacity(17);
    for _ in 0..17 {
        SECTIONS.with(|s| *s.borrow_mut() = Some(BTreeMap::new()));
        let before = (ALLOCATIONS.load(Relaxed), BYTES.load(Relaxed), live_bytes());
        PEAK.store(before.2, Relaxed);
        let began = Instant::now();
        let actual = std::hint::black_box(run());
        let elapsed = began.elapsed().as_nanos() as u64;
        let after = (ALLOCATIONS.load(Relaxed), BYTES.load(Relaxed), live_bytes());
        let peak = PEAK.load(Relaxed).saturating_sub(before.2);
        let sections = SECTIONS.with(|s| s.borrow_mut().take().unwrap());
        assert_eq!(actual, expected, "{name}: output changed between samples");
        samples.push(
            serde_json::json!({"ns":elapsed, "allocations":after.0-before.0,
            "allocated_bytes":after.1-before.1, "peak_live_increase":peak,
            "retained_delta":after.2 as i64-before.2 as i64, "sections":sections}),
        );
    }
    let mut times: Vec<_> = samples.iter().map(|v| v["ns"].as_u64().unwrap()).collect();
    times.sort_unstable();
    serde_json::json!({"name":name,"output":expected,"p50_ns":times[8],"p95_ns":times[16],"samples":samples})
}

pub fn write(cases: Vec<serde_json::Value>, conditions: serde_json::Value) {
    use std::io::Write;
    let path =
        std::path::PathBuf::from(std::env::var_os("WFI_PERF_OUTPUT").expect("WFI_PERF_OUTPUT"));
    assert!(path.is_absolute());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let executable = std::env::current_exe().unwrap();
    let report = serde_json::json!({"format":"filehue-performance-v1",
        "revision":std::env::var("WFI_PERF_REVISION").expect("WFI_PERF_REVISION"),
        "optimized":!cfg!(debug_assertions),"conditions":conditions,"cases":cases,
        "executable":executable,"os":std::env::consts::OS,"arch":std::env::consts::ARCH});
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
}
