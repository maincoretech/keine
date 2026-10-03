//! Shell recycle/restore uses the returned namespace item, not guessed $Recycle.Bin paths.
use std::{
    cell::RefCell,
    io,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    Win32::{
        Foundation::{E_ABORT, E_FAIL},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOF_NO_CONNECTED_ELEMENTS, FOF_NOERRORUI, FOF_SILENT, FOFX_EARLYFAILURE,
            FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IFileOperationProgressSink,
            IFileOperationProgressSink_Impl, IShellItem, SHCreateItemFromParsingName,
            SIGDN_DESKTOPABSOLUTEPARSING, TSF_DELETE_RECYCLE_IF_POSSIBLE,
        },
    },
    core::{HRESULT, PCWSTR, Ref, implement},
};

struct Apartment;
impl Apartment {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: this operation runs on its own thread with an STA apartment.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful initialization on this same thread.
        unsafe { CoUninitialize() };
    }
}
fn wide(path: &Path) -> windows::core::Result<Vec<u16>> {
    let mut text: Vec<u16> = path.as_os_str().encode_wide().collect();
    if text.contains(&0) {
        return Err(E_FAIL.into());
    }
    // Rust canonicalize uses verbatim drive/UNC paths. Shell parsing uses
    // ordinary DOS/UNC spelling; preserve UTF-16 including unpaired surrogates.
    let unc: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
    let verbatim: Vec<u16> = "\\\\?\\".encode_utf16().collect();
    if text.starts_with(&unc) {
        text.splice(..unc.len(), [u16::from(b'\\'), u16::from(b'\\')]);
    } else if text.starts_with(&verbatim) && text.get(5) == Some(&u16::from(b':')) {
        text.drain(..verbatim.len());
    }
    text.push(0);
    Ok(text)
}
fn item(path: &Path) -> windows::core::Result<IShellItem> {
    let path = wide(path)?;
    // SAFETY: a terminated path lives for the call; the returned COM object is owned.
    unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }
}
fn operation() -> windows::core::Result<IFileOperation> {
    // SAFETY: this thread has COM initialized; the interface owns its reference.
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }?;
    // No automatic Yes-to-All: destructive prompts must never be accepted.
    unsafe {
        operation.SetOperationFlags(
            FOFX_RECYCLEONDELETE
                | FOFX_EARLYFAILURE
                | FOF_NOERRORUI
                | FOF_SILENT
                | FOF_NO_CONNECTED_ELEMENTS,
        )
    }?;
    Ok(operation)
}
fn complete(operation: &IFileOperation) -> windows::core::Result<()> {
    // SAFETY: a live interface; queued items and sinks remain owned throughout.
    unsafe {
        operation.PerformOperations()?;
        if operation.GetAnyOperationsAborted()?.as_bool() {
            return Err(E_ABORT.into());
        }
    }
    Ok(())
}
fn on_sta<T: Send + 'static>(
    run: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
) -> io::Result<T> {
    std::thread::spawn(move || {
        let _apartment = Apartment::new()?;
        run()
    })
    .join()
    .map_err(|_| io::Error::other("Trash worker failed"))?
    .map_err(|error| io::Error::other(error.to_string()))
}

pub(in super::super) fn trash(path: &Path) -> io::Result<PathBuf> {
    let path = path.to_owned();
    on_sta(move || {
        let operation = operation()?;
        let item = item(&path)?;
        let receipt = Rc::new(RefCell::new(None));
        let sink = RecycleSink {
            receipt: receipt.clone(),
            restoring: false,
        };
        let sink: IFileOperationProgressSink = sink.into();
        // SAFETY: both live COM interfaces are retained until completion.
        unsafe { operation.DeleteItem(&item, &sink) }?;
        complete(&operation)?;
        let receipt = receipt
            .borrow()
            .clone()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        Ok(receipt)
    })
}
pub(in super::super) fn restore(receipt: &Path, destination: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ));
    }
    let receipt = receipt.to_owned();
    let destination = destination.to_owned();
    on_sta(move || {
        let operation = operation()?;
        let source = item(&receipt)?;
        let parent = item(
            destination
                .parent()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?,
        )?;
        let name = wide(Path::new(
            destination
                .file_name()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?,
        ))?;
        let sink: IFileOperationProgressSink = RecycleSink {
            receipt: Rc::new(RefCell::new(None)),
            restoring: true,
        }
        .into();
        // SAFETY: the shell resolves a recycle namespace item and updates bin metadata.
        unsafe { operation.MoveItem(&source, &parent, PCWSTR(name.as_ptr()), &sink) }?;
        complete(&operation)
    })
}

#[implement(IFileOperationProgressSink)]
struct RecycleSink {
    receipt: Rc<RefCell<Option<PathBuf>>>,
    restoring: bool,
}
#[allow(non_snake_case, unused_variables)]
impl IFileOperationProgressSink_Impl for RecycleSink_Impl {
    fn PreDeleteItem(&self, flags: u32, item: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            return Err(E_ABORT.into());
        }
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        flags: u32,
        item: Ref<'_, IShellItem>,
        result: HRESULT,
        recycled: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        result.ok()?;
        let recycled = recycled
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        // SAFETY: the shell allocates a terminated string; we copy it and free
        // it using the matching COM allocator even for non-Unicode filenames.
        unsafe {
            let name = recycled.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
            let path = PathBuf::from(std::ffi::OsString::from_wide(name.as_wide()));
            CoTaskMemFree(Some(name.0.cast()));
            *self.receipt.borrow_mut() = Some(path);
        }
        Ok(())
    }
    fn PreMoveItem(
        &self,
        flags: u32,
        item: Ref<'_, IShellItem>,
        destination: Ref<'_, IShellItem>,
        name: &PCWSTR,
    ) -> windows::core::Result<()> {
        if self.restoring {
            // Reject collisions again in the shell's preflight. We also leave
            // confirmation enabled so a racing collision cannot be auto-approved.
            unsafe {
                let folder = destination
                    .as_ref()
                    .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
                let path = folder.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
                let folder = PathBuf::from(std::ffi::OsString::from_wide(path.as_wide()));
                CoTaskMemFree(Some(path.0.cast()));
                let target = folder.join(std::ffi::OsString::from_wide(name.as_wide()));
                if std::fs::symlink_metadata(target).is_ok() {
                    return Err(E_ABORT.into());
                }
            }
        }
        Ok(())
    }
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, hrresult: windows::core::HRESULT) -> windows::core::Result<()> {
        hrresult.ok()
    }
    fn PreRenameItem(
        &self,
        dwflags: u32,
        psiitem: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        dwflags: u32,
        psiitem: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
        hrrename: windows::core::HRESULT,
        psinewlycreated: windows::core::Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        dwflags: u32,
        psiitem: windows::core::Ref<'_, IShellItem>,
        psidestinationfolder: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
        hrmove: windows::core::HRESULT,
        psinewlycreated: windows::core::Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        hrmove.ok()
    }
    fn PreCopyItem(
        &self,
        dwflags: u32,
        psiitem: windows::core::Ref<'_, IShellItem>,
        psidestinationfolder: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        dwflags: u32,
        psiitem: windows::core::Ref<'_, IShellItem>,
        psidestinationfolder: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
        hrcopy: windows::core::HRESULT,
        psinewlycreated: windows::core::Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreNewItem(
        &self,
        dwflags: u32,
        psidestinationfolder: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        dwflags: u32,
        psidestinationfolder: windows::core::Ref<'_, IShellItem>,
        psznewname: &windows::core::PCWSTR,
        psztemplatename: &windows::core::PCWSTR,
        dwfileattributes: u32,
        hrnew: windows::core::HRESULT,
        psinewitem: windows::core::Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, iworktotal: u32, iworksofar: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_paths_normalize_canonical_prefixes_and_reject_nul() {
        assert_eq!(
            wide(Path::new(r"\\?\C:\demo\图.webp")).unwrap(),
            wide(Path::new(r"C:\demo\图.webp")).unwrap()
        );
        assert_eq!(
            wide(Path::new(r"\\?\UNC\server\demo\图.webp")).unwrap(),
            wide(Path::new(r"\\server\demo\图.webp")).unwrap()
        );
        assert!(wide(Path::new("bad\0path")).is_err());
    }
}
