//! Bounded background encoding and generation-checked writes for slot thumbnails.
use bevy::prelude::*;
use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread;

struct SavePreviewJob {
    image: Image,
    path: PathBuf,
    slot: u32,
    generation: crate::storage::save::SavePreviewGeneration,
    coordinator: crate::storage::save::SavePreviewCoordinator,
}

#[derive(Resource)]
pub(crate) struct SavePreviewWriter {
    sender: Option<SyncSender<SavePreviewJob>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Default for SavePreviewWriter {
    fn default() -> Self {
        // Keep at most two final-size render targets waiting behind the image
        // currently being encoded. Rapid repeated saves never grow memory
        // without bound or stall the render thread on image compression.
        let (sender, receiver) = sync_channel::<SavePreviewJob>(2);
        let worker = thread::Builder::new()
            .name("keine-save-preview".into())
            .spawn(move || {
                for job in receiver {
                    write_save_preview(job);
                }
            })
            .map_err(|error| log::error!("failed to start save preview writer: {error}"))
            .ok();
        Self {
            sender: Some(sender),
            worker,
        }
    }
}

impl SavePreviewWriter {
    pub(crate) fn enqueue(
        &self,
        image: Image,
        path: PathBuf,
        slot: u32,
        generation: crate::storage::save::SavePreviewGeneration,
        coordinator: &crate::storage::save::SavePreviewCoordinator,
    ) {
        let Some(sender) = &self.sender else {
            return;
        };
        match sender.try_send(SavePreviewJob {
            image,
            path,
            slot,
            generation,
            coordinator: coordinator.clone(),
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => {
                log::warn!("save preview queue is full; skipped {}", job.path.display())
            }
            Err(TrySendError::Disconnected(job)) => log::error!(
                "save preview writer stopped before writing {}",
                job.path.display()
            ),
        }
    }
}

impl Drop for SavePreviewWriter {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn write_save_preview(job: SavePreviewJob) {
    let result = job
        .image
        .data
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("captured preview has no CPU pixel data"))
        .and_then(|rgba| {
            crate::scene::images::encode_preview(rgba, job.image.width(), job.image.height())
                .map_err(anyhow::Error::from)
        })
        .and_then(|bytes| {
            job.coordinator
                .commit_if_current(job.slot, job.generation, || {
                    crate::storage::write_atomically(&job.path, &bytes)
                })
                .map(|_| ())
        });
    if let Err(error) = result {
        log::error!("failed to save slot preview: {error:#}");
    }
}
