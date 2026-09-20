use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use keine_authoring::{SharedFrameConsumer, remove_stale_mapping};
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
