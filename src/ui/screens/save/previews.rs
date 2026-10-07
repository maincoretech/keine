//! Save_Load previews; registered through the parent facade.
use super::*;

pub(super) fn request_preview(
    project_root: &PersistenceRoot,
    slot: u32,
    cache: &mut SavePreviewCache,
) -> Option<Handle<Image>> {
    if let Some(cached) = cache.ready.get(&slot)
        && cached.modified.is_none()
    {
        return Some(cached.handle.clone());
    }
    let path = crate::storage::save::preview_path(project_root, slot);
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    if let Some(cached) = cache.ready.get(&slot)
        && cached.modified == Some(modified)
    {
        return Some(cached.handle.clone());
    }
    let project_root = project_root.to_path_buf();
    cache.pending.entry(slot).or_insert_with(|| {
        IoTaskPool::get().spawn(async move {
            let bytes = crate::storage::save::read_preview(&project_root, slot).ok()?;
            let image = crate::scene::images::decode_preview(&bytes).ok()?;
            Some(LoadedPreview { modified, image })
        })
    });
    None
}

pub fn poll_preview_tasks(
    mut cache: ResMut<SavePreviewCache>,
    mut images: ResMut<Assets<Image>>,
    mut previews: Query<(&SaveLoadPreviewImage, &mut ImageNode)>,
) {
    let completed = cache
        .pending
        .iter_mut()
        .filter_map(|(slot, task)| block_on(poll_once(task)).map(|result| (*slot, result)))
        .collect::<Vec<_>>();
    for (slot, result) in completed {
        cache.pending.remove(&slot);
        let Some(loaded) = result else { continue };
        let handle = images.add(loaded.image);
        cache.ready.insert(
            slot,
            CachedPreview {
                modified: Some(loaded.modified),
                handle: handle.clone(),
            },
        );
        for (preview, mut image) in &mut previews {
            if preview.0 == slot {
                image.image = handle.clone();
            }
        }
    }
}
