package moe.maincore.keine;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.ParcelFileDescriptor;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

/** System Back and document selection; screen and backup ownership remain in Rust. */
public final class EngineActivity extends NativeActivity {
    private static native void nativeBack();
    private static native void nativeBackupResult(int descriptor, boolean export);
    private static final int EXPORT_BACKUP = 101;
    private static final int IMPORT_BACKUP = 102;
    private OnBackInvokedCallback backCallback;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        // Associate NativeActivity's library with the VM for JNI resolution.
        System.loadLibrary("keine");
        hideSystemBars();
        // Keep only the dedicated, foreground benchmark awake. Never change a
        // user's global display timeout or the shipping game's window flags.
        if ("moe.maincore.keine.benchmark".equals(getPackageName())) {
            getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        }
        if (Build.VERSION.SDK_INT >= 33) {
            backCallback = EngineActivity::nativeBack;
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, backCallback);
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        // Run after the window attaches, including returns from the document picker.
        getWindow().getDecorView().post(this::hideSystemBars);
    }

    @Override
    public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) hideSystemBars();
    }

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        if (Build.VERSION.SDK_INT >= 30) {
            getWindow().setDecorFitsSystemWindows(false);
            WindowInsetsController controller = getWindow().getInsetsController();
            if (controller != null) {
                controller.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                controller.hide(WindowInsets.Type.systemBars());
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    | View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        // API 26–32 uses the same action as API 33+.
        nativeBack();
    }

    /** Called on the Java main thread; only grants access to the chosen document. */
    public void chooseBackup(boolean export) {
        Intent intent = new Intent(export ? Intent.ACTION_CREATE_DOCUMENT : Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(export ? "application/octet-stream" : "*/*");
        if (export) intent.putExtra(Intent.EXTRA_TITLE, "keine.keine-backup");
        try {
            startActivityForResult(intent, export ? EXPORT_BACKUP : IMPORT_BACKUP);
        } catch (RuntimeException error) {
            nativeBackupResult(-2, export);
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != EXPORT_BACKUP && requestCode != IMPORT_BACKUP) return;
        boolean export = requestCode == EXPORT_BACKUP;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            nativeBackupResult(-1, export);
            return;
        }
        try (ParcelFileDescriptor descriptor = getContentResolver().openFileDescriptor(
                data.getData(), export ? "wt" : "r")) {
            if (descriptor == null) {
                nativeBackupResult(-2, export);
            } else {
                nativeBackupResult(descriptor.detachFd(), export);
            }
        } catch (java.io.IOException | RuntimeException error) {
            nativeBackupResult(-2, export);
        }
    }

    @Override
    protected void onDestroy() {
        if (Build.VERSION.SDK_INT >= 33 && backCallback != null) {
            getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(backCallback);
        }
        super.onDestroy();
        // NativeActivity waits for the Rust thread here. After its saves and
        // teardown complete, end this standalone Engine process: winit and
        // Bevy's AndroidApp are process singletons and cannot be recreated.
        // Pause/resume does not destroy the Activity or take this path.
        System.exit(0);
    }
}
