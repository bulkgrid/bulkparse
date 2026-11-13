//! Diagnostic only: allocator instrumentation is excluded from timed benchmarks.
use serde_json::json;
use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{BufRead, Read, Seek};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
fn add(n: usize) {
    let live = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(live, Relaxed);
    TOTAL.fetch_add(n, Relaxed);
    CALLS.fetch_add(1, Relaxed);
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            add(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(layout);
        if !p.is_null() {
            add(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        System.dealloc(p, layout);
        LIVE.fetch_sub(layout.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = System.realloc(p, layout, size);
        if !new.is_null() {
            LIVE.fetch_sub(layout.size(), Relaxed);
            add(size);
        }
        new
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

#[cfg(target_os = "macos")]
#[derive(Default)]
#[repr(C)]
struct MallocStats {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}
#[cfg(target_os = "macos")]
extern "C" {
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStats);
}
fn snapshot() -> serde_json::Value {
    let live = LIVE.load(Relaxed);
    let peak = PEAK.load(Relaxed);
    let total = TOTAL.load(Relaxed);
    let calls = CALLS.load(Relaxed);
    #[cfg(target_os = "macos")]
    let system = {
        let mut s = MallocStats::default();
        // macOS SDK malloc.h: a null zone sums statistics across all zones.
        unsafe {
            malloc_zone_statistics(std::ptr::null_mut(), &mut s);
        }
        json!({"in_use":s.size_in_use,"reserved":s.size_allocated,"blocks":s.blocks_in_use})
    };
    #[cfg(not(target_os = "macos"))]
    let system = serde_json::Value::Null;
    json!({"live":live,"peak":peak,"allocated":total,"calls":calls,"system":system})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = bulkparse::Parser::new(bulkparse::Options {
        partial_on_limit: std::env::args().any(|a| a == "--partial-on-limit"),
        ..Default::default()
    });
    for path in std::io::stdin().lock().lines() {
        let path = path?;
        PEAK.store(LIVE.load(Relaxed), Relaxed);
        TOTAL.store(0, Relaxed);
        CALLS.store(0, Relaxed);
        let before = snapshot();
        if std::env::args().any(|a| a == "--structured") {
            let bytes = std::fs::read(&path)?;
            let result = parser.parse_bytes(&bytes, Some(&path));
            drop(bytes);
            let parsed = snapshot();
            let markdown = result.as_ref().ok().map(|r| r.to_markdown());
            let rendered = snapshot();
            let response =
                json!({"markdown":markdown.as_ref().map(|m| &m.text),"ok":result.is_ok()});
            serde_json::to_writer(std::io::sink(), &response)?;
            let transport = snapshot();
            drop(response);
            drop(markdown);
            drop(result);
            let released = snapshot();
            println!(
                "{}",
                json!({"path":path,"before":before,"parsed":parsed,"rendered":rendered,"transport":transport,"released":released})
            );
            continue;
        }
        let mut file = tempfile::tempfile()?;
        let bytes = std::fs::read(&path)?;
        let result = parser.parse_to_markdown(&bytes, Some(&path), &mut file);
        let parsed = snapshot();
        drop(bytes);
        file.rewind()?;
        let mut markdown = String::new();
        file.read_to_string(&mut markdown)?;
        let readback = snapshot();
        let response = json!({"markdown":markdown,"ok":result.is_ok()});
        serde_json::to_writer(std::io::sink(), &response)?;
        let transport = snapshot();
        drop(response);
        drop(markdown);
        drop(result);
        drop(file);
        let released = snapshot();
        println!(
            "{}",
            json!({"path":path,"before":before,"parsed":parsed,"readback":readback,"transport":transport,"released":released})
        );
    }
    Ok(())
}
