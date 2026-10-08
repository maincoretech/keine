//! ARM64 APK assembly using the existing encrypted publisher payload.
use super::*;

pub(super) fn validate_features(features: &str) -> Result<()> {
    if features
        .split(',')
        .any(|feature| feature.starts_with("video-"))
    {
        bail!("Android does not support video; remove video content before packaging");
    }
    Ok(())
}

pub(super) fn application_id(project: &keine_core::config::ProjectMetadata) -> Result<String> {
    let id = if project.bundle_identifier.is_empty() {
        format!("moe.maincore.keine.game_{}", project.id.replace('-', "_"))
    } else {
        project.bundle_identifier.clone()
    };
    if !id.contains('.')
        || !id.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
    {
        bail!("project.bundle_identifier is not a valid Android application ID");
    }
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn assemble(
    output: &Path,
    project: &Path,
    config: &keine_core::config::GameConfig,
    key_share_a: &Path,
    key_share_b: &Path,
    public_key: &Path,
    icons: &Path,
    benchmark: bool,
) -> Result<()> {
    let id = application_id(&config.project)?;
    if benchmark && id != "moe.maincore.keine.benchmark" {
        bail!(
            "Android benchmark requires the derived native-benchmark fixture; run prepare-android-benchmark.py first"
        );
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build.current_dir(repo).args([
        "ndk",
        "-t",
        "arm64-v8a",
        "-P",
        "26",
        "-o",
        "target/android/jniLibs",
        "rustc",
        "--locked",
        "-p",
        "keine",
        "--lib",
        "--crate-type",
        "cdylib",
        "--target-dir",
        "target/android",
        "--profile",
        engine_profile(benchmark),
        "--no-default-features",
        "--features",
        if benchmark {
            "ui-sounds,startup-metrics"
        } else {
            "hardened,ui-sounds"
        },
        "--",
        "-C",
        "link-arg=-Wl,-z,max-page-size=16384",
        "-C",
        "link-arg=-Wl,-z,common-page-size=16384",
    ]);
    configure_engine_environment(&mut build, key_share_a, key_share_b, public_key);
    build.env("KEINE_APP_ICON_DIR", icons);
    if !build
        .status()
        .context("failed to build Android Engine")?
        .success()
    {
        bail!("Android Engine build failed");
    }
    // This command has no publisher or derived key material in its environment.
    let mut metadata = Command::new("cargo")
        .current_dir(repo)
        .env_remove("KEINE_HAKUTAKU_IDENTITY")
        .env_remove("HAKUTAKU_IDENTITY_BASE64")
        .env_remove("KEINE_HAKUTAKU_KEY_SHARE_A")
        .env_remove("KEINE_HAKUTAKU_KEY_SHARE_B")
        .env_remove("KEINE_HAKUTAKU_PUBLIC_KEY")
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            "aarch64-linux-android",
            "--no-default-features",
            "--features",
            "ui-sounds",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let mut notices = Command::new("python3");
    notices
        .current_dir(repo)
        .env_remove("KEINE_HAKUTAKU_IDENTITY")
        .env_remove("HAKUTAKU_IDENTITY_BASE64")
        .env_remove("KEINE_HAKUTAKU_KEY_SHARE_A")
        .env_remove("KEINE_HAKUTAKU_KEY_SHARE_B")
        .env_remove("KEINE_HAKUTAKU_PUBLIC_KEY")
        .arg("dev/scripts/android-notices.py")
        .stdin(metadata.stdout.take().context("metadata pipe missing")?);
    let notice_snapshot = tempdir()?;
    if let Some(license) = super::notices::game(project)? {
        let path = notice_snapshot.path().join("GAME-LICENSE");
        fs::write(&path, license)?;
        notices.arg("--game-license").arg(path);
    }
    let status = notices.status()?;
    let metadata_status = metadata.wait()?;
    if !status.success() || !metadata_status.success() {
        bail!("Android native license staging failed");
    }
    let assets = tempdir()?;
    let game = assets.path().join("keine-game");
    fs::create_dir_all(game.join("data"))?;
    if benchmark {
        let plan = serde_json::to_vec_pretty(&crate::runtime::bootstrap::android_benchmark_plan())?;
        fs::write(assets.path().join("keine-benchmark.json"), &plan)?;
        fs::write(output.join("android-benchmark.json"), &plan)?;
        for (name, bytes) in [
            (
                "benchmark-android.py",
                include_bytes!("../../dev/scripts/benchmark-android.py").as_slice(),
            ),
            (
                "profile-runtime.py",
                include_bytes!("../../dev/scripts/profile-runtime.py").as_slice(),
            ),
        ] {
            fs::write(output.join(name), bytes)?;
        }
        fs::write(
            output.join("BENCHMARK.txt"),
            "Kēne Android full benchmark (ARM64, Android 8+)\nInstall Python 3.9+ and adb, enable USB debugging and authorize this computer.\nRun: python3 benchmark-android.py --apk game.apk --output android-report\nUse --serial DEVICE if multiple phones are connected. --backend auto|gl|vulkan selects a separate full run.\nKeep the device unlocked and foreground; do not change resolution or refresh rate during a run.\nReports, raw frames and device/thermal context are automatically copied to the computer.\nUnknown refresh budget stays unknown; optional --hz is an explicit reference, not a measured display rate.\nPass timing differences remain available when VSync caps frame rate.\nMatching unstripped lib/libkeine.so is included for optional Simpleperf attribution.\nThis debuggable profiling APK is a performance tool, not a hardened shipping game.\nVideo and desktop window-size cases are explicit unsupported skips; Android stress excludes video.\n",
        )?;
    }
    link_or_copy(&output.join("game.haku"), &game.join("game.haku"))?;
    for entry in fs::read_dir(output.join("data"))? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext == SEGMENT_FILE_EXTENSION)
        {
            link_or_copy(&entry.path(), &game.join("data").join(entry.file_name()))?;
        }
    }
    let mut gradle = Command::new("gradle");
    gradle
        .current_dir(repo)
        .env_remove("KEINE_HAKUTAKU_IDENTITY")
        .env_remove("HAKUTAKU_IDENTITY_BASE64")
        .env_remove("KEINE_HAKUTAKU_KEY_SHARE_A")
        .env_remove("KEINE_HAKUTAKU_KEY_SHARE_B")
        .env_remove("KEINE_HAKUTAKU_PUBLIC_KEY")
        .args(["--no-daemon", "-p", "dev/android"])
        .arg(format!("-PengineVersion={}", env!("CARGO_PKG_VERSION")))
        .arg(format!("-PengineBenchmark={benchmark}"))
        .arg(format!("-PengineApplicationId={id}"))
        .arg(format!("-PengineTitle={}", config.title))
        .arg(format!("-PengineGameDir={}", assets.path().display()))
        .arg(format!(
            "-PengineIconDir={}",
            icons.join("android").display()
        ))
        .args(["clean", "assembleRelease"]);
    if !gradle
        .status()
        .context("failed to package Android game")?
        .success()
    {
        bail!("Android APK assembly failed");
    }
    fs::copy(
        repo.join("dev/android/app/build/outputs/apk/release/app-release.apk"),
        output.join("game.apk"),
    )?;
    if benchmark {
        let symbols = output.join("lib");
        fs::create_dir_all(&symbols)?;
        fs::copy(
            repo.join("target/android/jniLibs/arm64-v8a/libkeine.so"),
            symbols.join("libkeine.so"),
        )?;
    }
    fs::write(
        output.join("ANDROID.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "application_id": id, "title": config.title, "project_id": config.project.id,
            "engine_version": env!("CARGO_PKG_VERSION"), "benchmark": benchmark,
        }))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_rejects_video_and_derives_a_stable_separate_application_id() {
        validate_features("ui-sounds").unwrap();
        assert!(validate_features("ui-sounds,video-native").is_err());
        assert!(validate_features("ui-sounds,video-ffmpeg").is_err());
        let mut project = keine_core::config::ProjectMetadata {
            id: "game-a".into(),
            ..Default::default()
        };
        assert_eq!(
            application_id(&project).unwrap(),
            "moe.maincore.keine.game_game_a"
        );
        project.bundle_identifier = "org.example.game".into();
        assert_eq!(application_id(&project).unwrap(), "org.example.game");
        project.bundle_identifier = "org.example.game-a".into();
        assert!(application_id(&project).is_err());
    }
}
