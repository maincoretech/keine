//! System document picker; archive format and installation stay in storage.
use std::fs::File;
use std::os::fd::FromRawFd;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use jni::objects::JObject;
use jni::refs::Global;
use jni::{JValue, JavaVM, jni_sig, jni_str};

pub(crate) struct BackupSelection {
    pub(crate) export: bool,
    pub(crate) file: File,
}

#[derive(Default)]
struct PickerState {
    pending: Option<bool>,
    result: Option<Result<Option<BackupSelection>>>,
}

static PICKER: Mutex<PickerState> = Mutex::new(PickerState {
    pending: None,
    result: None,
});

pub(crate) fn request_backup(export: bool) -> Result<()> {
    let app = bevy::android::ANDROID_APP
        .get()
        .context("Android Activity is unavailable")?
        .clone();
    {
        let mut state = PICKER
            .lock()
            .map_err(|_| anyhow::anyhow!("document picker is unavailable"))?;
        if state.pending.is_some() {
            bail!("a backup picker is already open");
        }
        state.pending = Some(export);
    }
    let activity_app = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: AndroidApp owns this VM and the unowned global Activity ref;
        // keep its clone alive throughout the scoped JNI call.
        let vm = unsafe { JavaVM::from_raw(activity_app.vm_as_ptr().cast()) };
        let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let raw = activity_app.activity_as_ptr() as jni::sys::jobject;
            // Borrow the global reference; never delete or retain it ourselves.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let result = env.call_method(
                activity.as_ref(),
                jni_str!("chooseBackup"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(export)],
            );
            if result.is_err() {
                env.exception_clear();
            }
            result.map(|_| ())
        });
        if let Err(error) = result {
            if let Ok(mut state) = PICKER.lock() {
                state.result = Some(Err(anyhow::anyhow!(
                    "failed to open document picker: {error}"
                )));
            }
            super::wake();
        }
    }));
    Ok(())
}

pub(crate) fn take_backup_result() -> Option<Result<Option<BackupSelection>>> {
    let mut state = PICKER.lock().ok()?;
    let result = state.result.take()?;
    state.pending = None;
    Some(result)
}

/// Java transfers exactly one owned descriptor via ParcelFileDescriptor.detachFd.
#[unsafe(no_mangle)]
pub extern "system" fn Java_moe_maincore_keine_EngineActivity_nativeBackupResult(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    descriptor: i32,
    export: u8,
) {
    let result = match descriptor {
        // SAFETY: the private Java callback detached this valid descriptor and
        // relinquished its ownership. File closes it on every success/error path.
        fd if fd >= 0 => Ok(Some(BackupSelection {
            export: export != 0,
            file: unsafe { File::from_raw_fd(fd) },
        })),
        -1 => Ok(None), // User canceled; persistent data stays untouched.
        _ => Err(anyhow::anyhow!(
            "document provider could not open the backup"
        )),
    };
    if let Ok(mut state) = PICKER.lock()
        && state.pending == Some(export != 0)
        && state.result.is_none()
    {
        state.result = Some(result);
    }
    super::wake();
}
