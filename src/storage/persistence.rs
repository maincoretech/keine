use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use keine_core::config::ProjectMetadata;

#[derive(Clone, Copy)]
enum HostPlatform {
    #[cfg(any(target_os = "macos", test))]
    MacOs,
    #[cfg(any(target_os = "windows", test))]
    Windows,
    #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
    Linux,
}

/// Select the root that owns the `saves/` directory.
///
/// Editable projects intentionally keep their local sidecar. Packaged content
/// is immutable and instead receives a stable per-game user-data namespace.
pub(crate) fn root(
    content_root: &Path,
    project: &ProjectMetadata,
    packaged: bool,
) -> Result<PathBuf> {
    if !packaged {
        return Ok(content_root.to_owned());
    }
    packaged_root(project, current_platform(), &|name| std::env::var_os(name))
}

/// Recover the selected persistence root and copy a legacy packaged sidecar
/// once, without deleting it or replacing an already initialized new root.
pub(crate) fn prepare(content_root: &Path, persistence_root: &Path) -> Result<()> {
    super::backup::recover(persistence_root)?;
    if content_root == persistence_root {
        return Ok(());
    }

    // Old packaged builds wrote beside game.haku. Settle any interrupted old
    // import before reading it, then preserve the old directory as a fallback.
    super::backup::recover(content_root)?;
    let legacy_saves = content_root.join("saves");
    let current_saves = persistence_root.join("saves");
    let migration = persistence_root.join(".legacy-saves-migration.keine-backup");
    if current_saves.is_dir() || !legacy_saves.is_dir() {
        cleanup_migration_archive(&migration);
        return Ok(());
    }

    if migration.exists() {
        fs::remove_file(&migration).with_context(|| {
            format!(
                "failed to remove stale save migration archive {}",
                migration.display()
            )
        })?;
        super::sync_directory(persistence_root)?;
    }
    super::backup::export(content_root, &migration)
        .context("failed to stage legacy save migration")?;
    super::backup::import(persistence_root, &migration)
        .context("failed to install legacy save data")?;
    cleanup_migration_archive(&migration);
    log::info!(
        "migrated legacy save data from {} to {} without removing the old copy",
        legacy_saves.display(),
        current_saves.display()
    );
    Ok(())
}

fn cleanup_migration_archive(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {
            if let Some(parent) = path.parent()
                && let Err(error) = super::sync_directory(parent)
            {
                log::warn!("save migration completed, but cleanup was not synchronized: {error:#}");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "save migration completed, but {} could not be removed: {error}",
            path.display()
        ),
    }
}

fn current_platform() -> HostPlatform {
    #[cfg(target_os = "macos")]
    return HostPlatform::MacOs;
    #[cfg(target_os = "windows")]
    return HostPlatform::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return HostPlatform::Linux;
}

fn packaged_root(
    project: &ProjectMetadata,
    platform: HostPlatform,
    environment: &impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    match platform {
        #[cfg(any(target_os = "macos", test))]
        HostPlatform::MacOs => {
            let application = project.application_identifier().context(
                "project.bundle_identifier must be a valid reverse-DNS identifier, or be omitted to derive one from project.id",
            )?;
            let home = absolute_environment_path(environment, "HOME")
                .context("could not locate the macOS user home directory")?;
            Ok(home.join("Library/Application Support").join(application))
        }
        #[cfg(any(target_os = "windows", test))]
        HostPlatform::Windows => {
            let project_id = required_project_id(project)?;
            let base = absolute_environment_path(environment, "LOCALAPPDATA")
                .or_else(|| absolute_environment_path(environment, "APPDATA"))
                .context("could not locate the Windows per-user application data directory")?;
            Ok(base.join("Kēne").join(project_id))
        }
        #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
        HostPlatform::Linux => {
            let project_id = required_project_id(project)?;
            let base = absolute_environment_path(environment, "XDG_DATA_HOME").or_else(|| {
                absolute_environment_path(environment, "HOME").map(|home| home.join(".local/share"))
            });
            Ok(base
                .context("could not locate the Linux user data directory")?
                .join("keine")
                .join(project_id))
        }
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn required_project_id(project: &ProjectMetadata) -> Result<&str> {
    project.valid_id().context(
        "packaged projects require project.id to be a lowercase ASCII slug (letters, digits and hyphens; maximum 64 bytes)",
    )
}

fn absolute_environment_path(
    environment: &impl Fn(&str) -> Option<OsString>,
    name: &str,
) -> Option<PathBuf> {
    environment(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn metadata() -> ProjectMetadata {
        ProjectMetadata {
            id: "example-game".into(),
            ..ProjectMetadata::default()
        }
    }

    fn environment<'a>(values: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            values
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| OsString::from(value)))
        }
    }

    #[test]
    fn development_stays_project_local_without_an_id() {
        let project = Path::new("/project");
        assert_eq!(
            root(project, &ProjectMetadata::default(), false).unwrap(),
            project
        );
    }

    #[test]
    fn packaged_root_uses_each_platform_user_data_contract() {
        let project = metadata();
        assert_eq!(
            packaged_root(
                &project,
                HostPlatform::MacOs,
                &environment(&[("HOME", "/Users/test")]),
            )
            .unwrap(),
            Path::new("/Users/test/Library/Application Support/moe.maincore.keine.example-game")
        );
        assert_eq!(
            packaged_root(
                &project,
                HostPlatform::Windows,
                &environment(&[("LOCALAPPDATA", "/windows/local")]),
            )
            .unwrap(),
            Path::new("/windows/local/Kēne/example-game")
        );
        assert_eq!(
            packaged_root(
                &project,
                HostPlatform::Linux,
                &environment(&[("XDG_DATA_HOME", "/xdg/data")]),
            )
            .unwrap(),
            Path::new("/xdg/data/keine/example-game")
        );
        assert_eq!(
            packaged_root(
                &project,
                HostPlatform::Linux,
                &environment(&[("XDG_DATA_HOME", "relative"), ("HOME", "/home/test")]),
            )
            .unwrap(),
            Path::new("/home/test/.local/share/keine/example-game")
        );
    }

    #[test]
    fn packaged_root_rejects_missing_or_invalid_identity() {
        assert!(
            packaged_root(
                &ProjectMetadata::default(),
                HostPlatform::Linux,
                &environment(&[("HOME", "/home/test")]),
            )
            .is_err()
        );
        let project = ProjectMetadata {
            id: "example".into(),
            bundle_identifier: "invalid".into(),
            ..ProjectMetadata::default()
        };
        assert!(
            packaged_root(
                &project,
                HostPlatform::MacOs,
                &environment(&[("HOME", "/Users/test")]),
            )
            .is_err()
        );
    }

    #[test]
    fn legacy_sidecar_is_copied_once_without_overwriting_or_deleting_it() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_root = std::env::temp_dir().join(format!("keine-persistence-migration-{nonce}"));
        let content = test_root.join("content");
        let persistence = test_root.join("user-data");
        fs::create_dir_all(content.join("saves")).unwrap();
        fs::write(content.join("saves/slot_1.keine"), b"legacy").unwrap();

        prepare(&content, &persistence).unwrap();

        assert_eq!(
            fs::read(persistence.join("saves/slot_1.keine")).unwrap(),
            b"legacy"
        );
        assert_eq!(
            fs::read(content.join("saves/slot_1.keine")).unwrap(),
            b"legacy"
        );
        fs::write(persistence.join("saves/slot_1.keine"), b"current").unwrap();
        prepare(&content, &persistence).unwrap();
        assert_eq!(
            fs::read(persistence.join("saves/slot_1.keine")).unwrap(),
            b"current"
        );
        assert!(
            !persistence
                .join(".legacy-saves-migration.keine-backup")
                .exists()
        );
        let _ = fs::remove_dir_all(test_root);
    }
}
