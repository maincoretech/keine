#[cfg(target_os = "macos")]
use super::{invalid, move_without_overwrite};
#[cfg(target_os = "macos")]
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
pub(super) fn trash(path: &Path) -> io::Result<PathBuf> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let path = path
        .to_str()
        .ok_or_else(|| invalid("File path is not UTF-8"))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let mut destination = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut destination))
        .map_err(|error| io::Error::other(error.to_string()))?;
    destination
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .ok_or_else(|| io::Error::other("System Trash did not return a recoverable location"))
}

#[cfg(target_os = "macos")]
pub(super) fn restore(receipt: &Path, destination: &Path) -> io::Result<()> {
    move_without_overwrite(receipt, destination)
}

#[cfg(any(target_os = "linux", all(test, unix)))]
#[path = "trash/linux.rs"]
mod linux;
#[cfg(windows)]
#[path = "trash/windows.rs"]
mod windows;
#[cfg(target_os = "linux")]
pub(super) use linux::{restore, trash};
#[cfg(windows)]
pub(super) use windows::{restore, trash};
