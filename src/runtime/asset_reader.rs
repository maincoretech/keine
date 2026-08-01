use std::collections::HashSet;
use std::io::{Error, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use bevy::asset::io::file::FileAssetReader;
use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetReaderFuture, AssetSourceBuilder, AssetSourceEvent,
    AssetWatcher, PathStream, Reader, ReaderNotSeekableError, STACK_FUTURE_SIZE, SeekableReader,
    StackFuture,
};
use futures_lite::io::{AsyncRead, AsyncSeek};
use keine_loader::{ContentFile, ContentMount};
use notify::{EventKind, RecursiveMode, Watcher};

const ASSET_WATCH_QUIET_PERIOD: Duration = Duration::from_millis(50);

#[derive(Default)]
struct PendingAssetChanges {
    paths: HashSet<PathBuf>,
    stopped: bool,
}

#[derive(Default)]
struct AssetChangeSet {
    pending: Mutex<PendingAssetChanges>,
    wake: Condvar,
}

impl AssetChangeSet {
    fn extend(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if pending.stopped {
            return;
        }
        pending.paths.extend(paths);
        drop(pending);
        self.wake.notify_one();
    }

    fn next_batch(&self) -> Option<HashSet<PathBuf>> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while pending.paths.is_empty() && !pending.stopped {
            pending = self
                .wake
                .wait(pending)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if pending.stopped && pending.paths.is_empty() {
            return None;
        }

        loop {
            let (next, timeout) = self
                .wake
                .wait_timeout(pending, ASSET_WATCH_QUIET_PERIOD)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            pending = next;
            if timeout.timed_out() || pending.stopped {
                return Some(std::mem::take(&mut pending.paths));
            }
        }
    }

    fn stop(&self) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.stopped = true;
        drop(pending);
        self.wake.notify_one();
    }
}

/// Creates Bevy's default source as a deterministic read-only overlay. Sources
/// are configured from low to high priority; the final source wins. Filesystem
/// mounts stay zero-copy and packaged mounts remain unopened until read.
pub(crate) fn overlay_source(mounts: Vec<ContentMount>) -> AssetSourceBuilder {
    let watched_mounts = mounts.clone();
    AssetSourceBuilder::new(move || Box::new(OverlayAssetReader::new(mounts.clone()))).with_watcher(
        move |sender| {
            let roots = watched_mounts
                .iter()
                .filter_map(ContentMount::filesystem_root)
                .collect::<Vec<_>>();
            if roots.is_empty() {
                return None;
            }
            let changes = Arc::new(AssetChangeSet::default());
            let callback_changes = Arc::clone(&changes);
            let watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    let event = match result {
                        Ok(event) => event,
                        Err(error) => {
                            log::warn!("asset watcher error: {error}");
                            return;
                        }
                    };
                    if !matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) {
                        return;
                    }
                    callback_changes.extend(event.paths);
                });
            let mut watcher = match watcher {
                Ok(watcher) => watcher,
                Err(error) => {
                    log::warn!("asset hot reload disabled: {error}");
                    return None;
                }
            };
            for root in roots {
                if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
                    log::warn!("failed to watch asset root {}: {error}", root.display());
                    return None;
                }
            }
            let worker_roots = watched_mounts
                .iter()
                .filter_map(ContentMount::filesystem_root)
                .collect::<Vec<_>>();
            let worker_changes = Arc::clone(&changes);
            let worker = thread::Builder::new()
                .name("keine-asset-watch".into())
                .spawn(move || {
                    while let Some(paths) = worker_changes.next_batch() {
                        for path in paths {
                            let Some((logical, is_meta)) = logical_asset_path(&path, &worker_roots)
                            else {
                                continue;
                            };
                            let exists = worker_roots
                                .iter()
                                .any(|root| watched_path(root, &logical, is_meta).is_file());
                            let event = match (is_meta, exists) {
                                (true, true) => AssetSourceEvent::ModifiedMeta(logical),
                                (true, false) => AssetSourceEvent::RemovedMeta(logical),
                                // Modified also retries a previously failed handle and
                                // reveals a lower-priority fallback after removal.
                                (false, true) => AssetSourceEvent::ModifiedAsset(logical),
                                (false, false) => AssetSourceEvent::RemovedAsset(logical),
                            };
                            let _ = sender.try_send(event);
                        }
                    }
                });
            let worker = match worker {
                Ok(worker) => worker,
                Err(error) => {
                    log::warn!("asset hot reload worker failed: {error}");
                    return None;
                }
            };
            Some(Box::new(OverlayAssetWatcher {
                watcher: Some(watcher),
                worker: Some(worker),
                changes,
            }))
        },
    )
}

struct OverlayAssetWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    worker: Option<thread::JoinHandle<()>>,
    changes: Arc<AssetChangeSet>,
}

impl AssetWatcher for OverlayAssetWatcher {}

impl Drop for OverlayAssetWatcher {
    fn drop(&mut self) {
        drop(self.watcher.take());
        self.changes.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn logical_asset_path(path: &Path, roots: &[PathBuf]) -> Option<(PathBuf, bool)> {
    let root = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())?;
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    let is_meta = relative
        .extension()
        .is_some_and(|extension| extension == "meta");
    Some((
        if is_meta {
            relative.with_extension("")
        } else {
            relative.to_owned()
        },
        is_meta,
    ))
}

fn watched_path(root: &Path, logical: &Path, is_meta: bool) -> PathBuf {
    let mut path = root.join(logical);
    if is_meta {
        let extension = path.extension().map_or_else(
            || "meta".into(),
            |value| format!("{}.meta", value.to_string_lossy()),
        );
        path.set_extension(extension);
    }
    path
}

enum MountedReader {
    FileSystem(FileAssetReader),
    Content(ContentAssetReader),
}

impl MountedReader {
    fn new(mount: ContentMount) -> Self {
        if let Some(root) = mount.filesystem_root() {
            Self::FileSystem(FileAssetReader::new(root))
        } else {
            Self::Content(ContentAssetReader::new(mount))
        }
    }

    async fn read<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        match self {
            Self::FileSystem(reader) => reader
                .read(path)
                .await
                .map(|reader| Box::new(reader) as Box<dyn Reader>),
            Self::Content(reader) => reader
                .read(path)
                .await
                .map(|reader| Box::new(reader) as Box<dyn Reader>),
        }
    }

    async fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        match self {
            Self::FileSystem(reader) => reader
                .read_meta(path)
                .await
                .map(|reader| Box::new(reader) as Box<dyn Reader>),
            Self::Content(reader) => reader
                .read_meta(path)
                .await
                .map(|reader| Box::new(reader) as Box<dyn Reader>),
        }
    }

    async fn read_directory(&self, path: &Path) -> Result<Box<PathStream>, AssetReaderError> {
        match self {
            Self::FileSystem(reader) => reader.read_directory(path).await,
            Self::Content(reader) => reader.read_directory(path).await,
        }
    }

    async fn is_directory(&self, path: &Path) -> Result<bool, AssetReaderError> {
        match self {
            Self::FileSystem(reader) => reader.is_directory(path).await,
            Self::Content(reader) => reader.is_directory(path).await,
        }
    }
}

struct OverlayAssetReader {
    readers: Vec<MountedReader>,
}

impl OverlayAssetReader {
    fn new(mounts: Vec<ContentMount>) -> Self {
        Self {
            readers: mounts.into_iter().rev().map(MountedReader::new).collect(),
        }
    }
}

impl AssetReader for OverlayAssetReader {
    fn read<'a>(&'a self, path: &'a Path) -> impl AssetReaderFuture<Value: Reader + 'a> {
        async move {
            for reader in &self.readers {
                match reader.read(path).await {
                    Ok(value) => return Ok(value),
                    Err(AssetReaderError::NotFound(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(AssetReaderError::NotFound(path.to_owned()))
        }
    }

    fn read_meta<'a>(&'a self, path: &'a Path) -> impl AssetReaderFuture<Value: Reader + 'a> {
        async move {
            for reader in &self.readers {
                match reader.read(path).await {
                    Ok(_) => return reader.read_meta(path).await,
                    Err(AssetReaderError::NotFound(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(AssetReaderError::NotFound(path.to_owned()))
        }
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        for reader in &self.readers {
            match reader.read_directory(path).await {
                Ok(value) => return Ok(value),
                Err(AssetReaderError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Err(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        for reader in &self.readers {
            match reader.is_directory(path).await {
                Ok(true) => return Ok(true),
                Ok(false) | Err(AssetReaderError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }
}

struct ContentAssetReader {
    mount: ContentMount,
}

impl ContentAssetReader {
    fn new(mount: ContentMount) -> Self {
        Self { mount }
    }
}

impl AssetReader for ContentAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        if !self.mount.contains_file(path) {
            return Err(AssetReaderError::NotFound(path.to_owned()));
        }
        self.mount
            .open_file(path)
            .map(ContentStreamReader::new)
            .map_err(asset_io_error)
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<ContentStreamReader, _>(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        if !self.mount.is_directory(path) {
            return Err(AssetReaderError::NotFound(path.to_owned()));
        }
        let entries = self.mount.read_directory(path).map_err(asset_io_error)?;
        Ok(Box::new(futures_lite::stream::iter(entries)))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(self.mount.is_directory(path))
    }
}

struct ContentStreamReader {
    cursor: ContentFile,
}

impl ContentStreamReader {
    fn new(cursor: ContentFile) -> Self {
        Self { cursor }
    }
}

impl AsyncRead for ContentStreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Read::read(&mut self.cursor, buffer))
    }
}

impl AsyncSeek for ContentStreamReader {
    fn poll_seek(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        position: SeekFrom,
    ) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Seek::seek(&mut self.cursor, position))
    }
}

impl Reader for ContentStreamReader {
    fn read_to_end<'a>(
        &'a mut self,
        buffer: &'a mut Vec<u8>,
    ) -> StackFuture<'a, std::io::Result<usize>, STACK_FUTURE_SIZE> {
        StackFuture::from(async move { self.cursor.read_remaining_into(buffer) })
    }

    fn seekable(&mut self) -> Result<&mut dyn SeekableReader, ReaderNotSeekableError> {
        Ok(self)
    }
}

fn asset_io_error(error: anyhow::Error) -> AssetReaderError {
    AssetReaderError::from(Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use bevy::tasks::block_on;

    use super::*;

    #[test]
    fn later_asset_root_overrides_earlier_root() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-overlay-{nonce}"));
        let base = root.join("base");
        let patch = root.join("patch");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&patch).unwrap();
        fs::write(base.join("shared.txt"), "base").unwrap();
        fs::write(patch.join("shared.txt"), "patch").unwrap();
        let overlay = OverlayAssetReader::new(vec![
            keine_loader::SourceMount::assets("test", "base", base)
                .asset
                .unwrap(),
            keine_loader::SourceMount::assets("test", "patch", patch)
                .asset
                .unwrap(),
        ]);

        let bytes = block_on(async {
            let mut reader = overlay.read(Path::new("shared.txt")).await.unwrap();
            let mut bytes = Vec::new();
            Reader::read_to_end(&mut reader, &mut bytes).await.unwrap();
            bytes
        });
        assert_eq!(bytes, b"patch");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn watcher_maps_nested_assets_and_metadata_to_logical_paths() {
        let root = PathBuf::from("project").join("assets");
        assert_eq!(
            logical_asset_path(
                &root.join("background/sea.png"),
                std::slice::from_ref(&root)
            ),
            Some((PathBuf::from("background/sea.png"), false))
        );
        assert_eq!(
            logical_asset_path(
                &root.join("background/sea.png.meta"),
                std::slice::from_ref(&root),
            ),
            Some((PathBuf::from("background/sea.png"), true))
        );
        assert_eq!(
            watched_path(&root, Path::new("background/sea.png"), true),
            root.join("background/sea.png.meta")
        );
    }
}
