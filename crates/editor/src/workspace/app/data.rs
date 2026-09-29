use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const APP_ID: &str = "moe.maincore.keine-editor";

/// Selects the editor-owned app-data root. Project content is never a fallback.
pub fn root() -> io::Result<PathBuf> {
    root_for_current_platform(&|name| std::env::var_os(name))
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

fn missing(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("could not locate an absolute {name} for Kēne Editor app data"),
    )
}

fn root_for_current_platform(
    environment: &impl Fn(&str) -> Option<OsString>,
) -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        absolute_environment_path(environment, "HOME")
            .map(|home| home.join("Library/Application Support").join(APP_ID))
            .ok_or_else(|| missing("HOME"))
    }

    #[cfg(target_os = "windows")]
    {
        absolute_environment_path(environment, "LOCALAPPDATA")
            .or_else(|| absolute_environment_path(environment, "APPDATA"))
            .map(|base| base.join("Kēne").join("Editor"))
            .ok_or_else(|| missing("LOCALAPPDATA or APPDATA"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let base = absolute_environment_path(environment, "XDG_DATA_HOME").or_else(|| {
            absolute_environment_path(environment, "HOME").map(|home| home.join(".local/share"))
        });
        base.map(|path| path.join("keine").join("editor"))
            .ok_or_else(|| missing("XDG_DATA_HOME or HOME"))
    }
}

/// Returns whether a selected app-data root is separate from a project root.
pub fn is_outside_project(app_data: &Path, project: &Path) -> bool {
    !app_data.starts_with(project)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn environment<'a>(values: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            values
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| OsStr::new(value).to_os_string()))
        }
    }

    #[test]
    fn app_data_is_absolute_and_never_uses_the_project_as_fallback() {
        let project = Path::new("/work/game");
        let root = root_for_current_platform(&environment(&[
            ("HOME", "/Users/test"),
            ("LOCALAPPDATA", "/Users/test/AppData/Local"),
            ("XDG_DATA_HOME", "/Users/test/.local/share"),
        ]))
        .unwrap();

        assert!(root.is_absolute());
        assert!(is_outside_project(&root, project));
        assert_ne!(root, project);
    }

    #[test]
    fn relative_environment_paths_are_rejected() {
        let error = root_for_current_platform(&environment(&[
            ("HOME", "relative-home"),
            ("LOCALAPPDATA", "relative-local"),
            ("APPDATA", "relative-roaming"),
            ("XDG_DATA_HOME", "relative-data"),
        ]))
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
