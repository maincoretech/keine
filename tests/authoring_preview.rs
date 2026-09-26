use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use keine_authoring::{PreviewInput, SharedFrameConsumer, remove_stale_mapping};
use keine_editor::engine::EngineProcess;

#[test]
#[ignore = "requires a local graphics adapter; run explicitly for Phase 4 acceptance"]
fn authoring_child_publishes_a_real_composited_frame_and_stops_cleanly() {
    let engine = PathBuf::from(env!("CARGO_BIN_EXE_keine"));
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("projects/test-project");
    let generation = 41;
    let mut child = EngineProcess::launch(&engine, &project, generation)
        .expect("launch the prebuilt Engine authoring child");
    for cycle in 0..3 {
        let mapping = std::env::temp_dir().join(format!(
            "keine-preview-acceptance-{}-{generation}-{cycle}.frames",
            std::process::id()
        ));
        remove_stale_mapping(&mapping).unwrap();
        {
            let mut frames =
                SharedFrameConsumer::create(mapping.clone(), 0x4b454e45, generation, 1920, 1080)
                    .expect("create shared preview frame mapping");
            child
                .start_preview(frames.descriptor().clone(), 0)
                .expect("start the offscreen runtime");

            let started = Instant::now();
            let deadline = Instant::now() + Duration::from_secs(15);
            let frame = loop {
                if let Some(frame) = frames.read_latest(0).expect("read latest preview frame") {
                    let first = &frame.bytes[..4];
                    if frame.bytes.chunks_exact(4).any(|pixel| pixel != first) {
                        break frame;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "Engine did not publish a visible Preview frame within 15 seconds"
                );
                thread::sleep(Duration::from_millis(16));
            };
            assert_eq!((frame.metadata.width, frame.metadata.height), (1920, 1080));
            assert_eq!(frame.metadata.stride, 1920 * 4);
            assert_eq!(frame.bytes.len(), 1920 * 1080 * 4);
            let stats = frames.stats();
            assert!(stats.published >= 1);
            assert!(
                child
                    .set_execution_cursor(0, Path::new("scripts/absent.shou"), 1, 1)
                    .expect("a non-action cursor is not a Preview failure")
                    .is_none()
            );
            assert!(
                child
                    .set_execution_cursor(0, Path::new("scripts/main.shou"), 5, 1)
                    .expect("a valid source cursor still works after a miss")
                    .is_some()
            );
            eprintln!(
                "preview cycle {}: visible_frame_ms={} published={} overwritten={}",
                cycle + 1,
                started.elapsed().as_millis(),
                stats.published,
                stats.overwritten
            );

            child.pause().expect("pause Preview");
            let paused_at = frames.stats().published;
            thread::sleep(Duration::from_millis(500));
            let paused_after = frames.stats().published;
            assert!(
                paused_after.saturating_sub(paused_at) <= 3,
                "paused Preview must not keep presenting continuously"
            );
            child.resume().expect("resume Preview");
            child.stop().expect("stop Preview");
        }
        assert!(
            !mapping.exists(),
            "frame mapping must be removed after each Stop cycle"
        );
    }
    child.shutdown().expect("shut down authoring child");
}

#[test]
#[ignore = "requires a local graphics adapter; run explicitly for Preview acceptance"]
fn live_source_patch_publishes_a_new_revision() {
    let engine = PathBuf::from(env!("CARGO_BIN_EXE_keine"));
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("projects/test-project");
    let generation = 42;
    let mut child = EngineProcess::launch(&engine, &project, generation).unwrap();
    let mapping = std::env::temp_dir().join(format!(
        "keine-preview-patch-{}-{generation}.frames",
        std::process::id()
    ));
    remove_stale_mapping(&mapping).unwrap();
    let mut frames =
        SharedFrameConsumer::create(mapping.clone(), 0x4b454e45, generation, 1920, 1080).unwrap();
    let source_path = Path::new("scripts/main.shou");
    let source = fs::read(project.join(source_path)).unwrap();
    child.apply_snapshot(source_path, 1, &source).unwrap();
    child.start_preview(frames.descriptor().clone(), 1).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let before = loop {
        if let Some(frame) = frames.read_latest(1).unwrap() {
            break frame;
        }
        assert!(
            Instant::now() < deadline,
            "initial Preview frame was not published"
        );
        thread::sleep(Duration::from_millis(8));
    };

    let start = source
        .windows(7)
        .position(|bytes| bytes == b"Welcome")
        .unwrap();
    let changed_at = Instant::now();
    child
        .apply_patch(source_path, 1, 2, start..start + 7, b"Hello")
        .unwrap();
    let acknowledged_ms = changed_at.elapsed().as_millis();
    let deadline = Instant::now() + Duration::from_secs(15);
    let after = loop {
        if let Some(frame) = frames.read_latest(2).unwrap() {
            break frame;
        }
        assert!(
            Instant::now() < deadline,
            "edited Preview frame was not published"
        );
        thread::sleep(Duration::from_millis(8));
    };
    assert_ne!(
        before.bytes, after.bytes,
        "source patch must change the rendered frame"
    );
    eprintln!(
        "preview source patch: acknowledge_ms={acknowledged_ms} visible_frame_ms={} published={} overwritten={}",
        changed_at.elapsed().as_millis(),
        frames.stats().published,
        frames.stats().overwritten,
    );
    assert!(
        child
            .set_execution_cursor(2, source_path, 5, 1)
            .unwrap()
            .is_some(),
        "selecting another Block must resolve a source position"
    );
    let selected = child.execution_location(2).unwrap().unwrap();
    assert_eq!(selected.0, source_path);
    assert_eq!(selected.1, 5);
    let mut advanced = false;
    for _ in 0..4 {
        child.input(2, PreviewInput::Advance).unwrap();
        thread::sleep(Duration::from_millis(120));
        let location = child.execution_location(2).unwrap();
        if location.is_some_and(|(_, line, _)| line > selected.1) {
            advanced = true;
            break;
        }
    }
    assert!(
        advanced,
        "Preview input must move the reported Block position"
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(frame) = frames.read_latest(2).unwrap()
            && frame.bytes != after.bytes
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "selecting another Block did not change Preview pixels"
        );
        thread::sleep(Duration::from_millis(8));
    }
    // Incomplete authoring edits retain the last good Program while the
    // protocol revision keeps advancing, so the next correction can recover.
    let entry = source
        .windows(7)
        .position(|bytes| bytes == b"opening")
        .unwrap();
    child
        .apply_patch(source_path, 2, 3, entry..entry + 7, b"unknown")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while frames.read_latest(3).unwrap().is_none() {
        assert!(Instant::now() < deadline, "invalid edit stopped Preview");
        thread::sleep(Duration::from_millis(8));
    }
    child
        .apply_patch(source_path, 3, 4, entry..entry + 7, b"opening")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while frames.read_latest(4).unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "corrected edit did not recover Preview"
        );
        thread::sleep(Duration::from_millis(8));
    }
    child.stop().unwrap();
    drop(frames);
    assert!(!mapping.exists());
    child.shutdown().unwrap();
}
