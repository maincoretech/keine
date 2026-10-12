//! Short system UI feedback; no script effects or vibration timers.
use jni::objects::JObject;
use jni::refs::Global;
use jni::{JValue, JavaVM, jni_sig, jni_str};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn ui_feedback(switch: bool) {
    let Some(app) = bevy::android::ANDROID_APP.get().cloned() else {
        return;
    };
    let activity_app = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: the cloned AndroidApp keeps its VM and Activity global ref
        // alive throughout this scoped call, matching the backup bridge.
        let vm = unsafe { JavaVM::from_raw(activity_app.vm_as_ptr().cast()) };
        let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let raw = activity_app.activity_as_ptr() as jni::sys::jobject;
            // Borrow AndroidApp's reference; do not retain or delete it.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let result = env.call_method(
                activity.as_ref(),
                jni_str!("uiHapticFeedback"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(switch)],
            );
            if result.is_err() {
                env.exception_clear();
            }
            result.map(|_| ())
        });
        static REPORTED: AtomicBool = AtomicBool::new(false);
        if let Err(error) = result
            && !REPORTED.swap(true, Ordering::Relaxed)
        {
            bevy::log::warn!("Android UI haptics unavailable: {error}");
        }
    }));
}
