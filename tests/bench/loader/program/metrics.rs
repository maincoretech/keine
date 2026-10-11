//! Opt-in allocation census, kept separate from Criterion's timed samples.
//! System allocation sizes exclude allocator metadata and resident-page slack.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

use keine_loader::{DiagnosticLevel, parse_native_scenes};

#[derive(Clone, Copy, Default)]
struct Counts {
    allocations: usize,
    reallocations: usize,
    allocated_bytes: usize,
    live_bytes: usize,
    peak_bytes: usize,
}

thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn allocated(bytes: usize, old: Option<usize>) {
    let _ = COUNTS.try_with(|slot| {
        if let Some(mut counts) = slot.get() {
            if let Some(old) = old {
                counts.reallocations += 1;
                counts.live_bytes -= old;
            } else {
                counts.allocations += 1;
            }
            counts.allocated_bytes += bytes;
            counts.live_bytes += bytes;
            counts.peak_bytes = counts.peak_bytes.max(counts.live_bytes);
            slot.set(Some(counts));
        }
    });
}

// SAFETY: Every operation forwards the original pointer, layout and size to
// System. Accounting uses allocation-free TLS; failed allocations are ignored.
// Only this thread's complete parse/output-drop lifetime is enabled below.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies GlobalAlloc's valid layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size(), None);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies GlobalAlloc's valid layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size(), None);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = COUNTS.try_with(|slot| {
            if let Some(mut counts) = slot.get() {
                counts.live_bytes -= layout.size();
                slot.set(Some(counts));
            }
        });
        // SAFETY: The caller supplies System's live pointer and original layout.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The caller supplies System's live pointer, layout and new size.
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            allocated(size, Some(layout.size()));
        }
        replacement
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

pub(super) fn run() -> bool {
    let Ok(case) = std::env::var("KEINE_PARSE_METRICS") else {
        return false;
    };
    let source = if case == "corpus" {
        super::corpus().expect("KEINE_NATIVE_SOURCE_DIR for corpus")
    } else {
        let (kind, size) = case.split_once(':').expect("text:BYTES or mixed:BYTES");
        workload(kind, size.parse().expect("input byte count"))
    };
    // Warm the implementation once. The source and warm-up are not counted.
    drop(parse_native_scenes(&source));
    let started = Instant::now();
    let scenes = parse_native_scenes(&source);
    let parse_ns = started.elapsed().as_nanos();
    let actions: usize = scenes.iter().map(|scene| scene.report.actions.len()).sum();
    assert!(
        scenes
            .iter()
            .flat_map(|scene| &scene.report.diagnostics)
            .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error)
    );
    // This output identity is computed outside the timed/allocation interval.
    let fingerprint = keine_core::Program::from_scenes(
        scenes
            .into_iter()
            .map(|scene| (scene.name.expect("scene name"), scene.report.actions)),
    )
    .fingerprint();

    COUNTS.with(|slot| slot.set(Some(Counts::default())));
    let scenes = parse_native_scenes(&source);
    let retained = COUNTS.with(|slot| slot.get().unwrap().live_bytes);
    drop(scenes);
    let counts = COUNTS.with(|slot| slot.replace(None).unwrap());
    assert_eq!(
        counts.live_bytes, 0,
        "parse must release all counted allocations"
    );
    println!(
        "{}",
        serde_json::json!({
            "case": case, "input_bytes": source.len(), "actions": actions,
            "fingerprint": format!("{fingerprint:016x}"), "parse_ns": parse_ns,
            "allocations": counts.allocations, "reallocations": counts.reallocations,
            "allocated_bytes": counts.allocated_bytes, "peak_heap_bytes": counts.peak_bytes,
            "retained_heap_bytes": retained, "after_drop_heap_bytes": counts.live_bytes,
        })
    );
    true
}

fn workload(kind: &str, minimum_bytes: usize) -> String {
    let mut source = String::with_capacity(minimum_bytes + 4096);
    if kind == "mixed" {
        source.push_str("scene init { let counter = 0 }\n");
    }
    let mut scene = 0;
    while source.len() < minimum_bytes {
        source.push_str(&format!("scene bench_{scene} {{\n"));
        match kind {
            "text" => {
                for line in 0..1000 {
                    if line != 0 {
                        source.push_str(",\n");
                    }
                    source.push_str(&format!("\"Line {line}: 你好，世界。\""));
                }
            }
            "mixed" => {
                for line in 0..100 {
                    if line != 0 {
                        source.push_str(",\n");
                    }
                    // `if` lowers to conditional jumps and labels. `call` and
                    // `goto` retain real scene references in the same workload.
                    source.push_str("counter += 1,\nif (counter > 0) { Narrator: \"值 ${counter}\" } else { \"零\" }");
                }
                source.push_str(",\ncall(init),\ngoto(init)");
            }
            _ => panic!("unknown workload {kind}"),
        }
        source.push_str("\n}\n");
        scene += 1;
    }
    source
}
