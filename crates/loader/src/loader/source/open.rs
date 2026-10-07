//! Open an already confined canonical path without trusting a second pathname
//! lookup to preserve the result of the confinement check.

use std::fs::File;
use std::io;
use std::path::Path;

pub(super) fn resolved_file(root: &Path, resolved: &Path) -> io::Result<File> {
    if !resolved.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "content file escaped its mount",
        ));
    }
    let file = open(root, resolved)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "content entry is not a regular file",
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn open(_root: &Path, resolved: &Path) -> io::Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;

    // Each openat sees exactly one component. No ancestor or leaf can turn
    // into a followed symlink after canonicalization. In-root symlinks have
    // already been resolved and continue to work through their canonical path.
    #[cfg(target_os = "android")]
    let mut directory = {
        use std::os::unix::fs::OpenOptionsExt;
        File::options()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open("/")?
    };
    #[cfg(not(target_os = "android"))]
    let mut directory = File::open("/")?;
    let mut components = resolved.components().peekable();
    if components.next() != Some(Component::RootDir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "content path is not canonical",
        ));
    }
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "content path is not canonical",
            ));
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in content path"))?;
        let flags = libc::O_CLOEXEC | libc::O_NOFOLLOW;
        let flags = if components.peek().is_some() {
            // Traversal needs search permission, not permission to list the
            // directory. Preserve ordinary file-open permission semantics.
            #[cfg(any(target_os = "linux", target_os = "android"))]
            let access = libc::O_PATH;
            #[cfg(target_os = "macos")]
            let access = libc::O_SEARCH;
            #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
            let access = libc::O_RDONLY;
            flags | access | libc::O_DIRECTORY
        } else {
            // A concurrent regular-file -> FIFO replacement must not block
            // before the handle's type can be checked. No effect on regular IO.
            flags | libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOCTTY
        };
        // SAFETY: directory owns a live fd; name is a NUL-terminated single
        // component. No O_CREAT flag is used, so openat needs no mode argument.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor, transferred exactly
        // once into File. Replacing directory closes the previous descriptor.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(windows)]
fn open(root: &Path, resolved: &Path) -> io::Result<File> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let file = File::open(resolved)?;
    let mut buffer = vec![0_u16; 512];
    let query = |buffer: &mut [u16]| {
        // SAFETY: File owns the handle and the writable buffer has exactly the
        // reported capacity. The call neither transfers nor closes the handle.
        unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        }
    };
    let mut length = query(&mut buffer);
    if length as usize >= buffer.len() && length <= 32_768 {
        buffer.resize(length as usize, 0);
        length = query(&mut buffer);
    }
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "content handle path is too long or changed during validation",
        ));
    }
    let actual = std::path::PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    // canonicalize and GetFinalPathNameByHandleW both use normalized DOS paths
    // with the extended-length prefix. Check the opened handle before any read.
    if !actual.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "opened content file escaped its mount",
        ));
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn open(_root: &Path, _resolved: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "confined content files require Unix or Windows",
    ))
}
