use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PhysicalIdentity {
    #[cfg(unix)]
    Unix {
        device: u64,
        inode: u64,
    },
    Path(PathBuf),
}

/// Stable identity used to ensure one writable window owns one physical project.
#[derive(Clone, Debug)]
pub struct ProjectKey {
    display_path: PathBuf,
    identity: PhysicalIdentity,
}

impl ProjectKey {
    pub fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let absolute = absolute_lexical(path.as_ref())?;
        let display_path = absolute.canonicalize().unwrap_or(absolute);
        let identity = physical_identity(&display_path).unwrap_or_else(|| {
            PhysicalIdentity::Path(normalize_platform_path(display_path.clone()))
        });
        Ok(Self {
            display_path,
            identity,
        })
    }

    pub fn path(&self) -> &Path {
        &self.display_path
    }

    /// Deterministic, non-secret app-data key for this physical project.
    pub fn workspace_id(&self) -> String {
        let mut hash = FNV_OFFSET;
        match &self.identity {
            #[cfg(unix)]
            PhysicalIdentity::Unix { device, inode } => {
                hash_bytes(&mut hash, b"unix\0");
                hash_bytes(&mut hash, &device.to_le_bytes());
                hash_bytes(&mut hash, &inode.to_le_bytes());
            }
            PhysicalIdentity::Path(path) => {
                hash_bytes(&mut hash, b"path\0");
                hash_bytes(&mut hash, path.to_string_lossy().as_bytes());
            }
        }
        format!("{hash:016x}")
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001b3;

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

impl PartialEq for ProjectKey {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}

impl Eq for ProjectKey {}

impl Hash for ProjectKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
    }
}

fn absolute_lexical(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn normalize_platform_path(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return PathBuf::from(path.to_string_lossy().to_lowercase());
    }
    #[cfg(not(target_os = "windows"))]
    path
}

#[cfg(unix)]
fn physical_identity(path: &Path) -> Option<PhysicalIdentity> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = path.metadata().ok()?;
    Some(PhysicalIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn physical_identity(_: &Path) -> Option<PhysicalIdentity> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("keine-editor-project-key-{nonce}"))
    }

    #[test]
    fn relative_and_absolute_paths_share_a_key() {
        let root = std::env::current_dir().unwrap();
        assert_eq!(
            ProjectKey::from_path(".").unwrap(),
            ProjectKey::from_path(root).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_project_paths_share_a_physical_key() {
        use std::os::unix::fs::symlink;

        let root = test_root();
        let project = root.join("project");
        let link = root.join("alias");
        fs::create_dir_all(&project).unwrap();
        symlink(&project, &link).unwrap();

        assert_eq!(
            ProjectKey::from_path(&project).unwrap(),
            ProjectKey::from_path(&link).unwrap()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_paths_fall_back_to_normalized_absolute_paths() {
        let root = test_root();
        let direct = root.join("project");
        let dotted = root.join("child/../project");
        assert_eq!(
            ProjectKey::from_path(direct).unwrap(),
            ProjectKey::from_path(dotted).unwrap()
        );
    }

    #[test]
    fn workspace_id_is_stable_for_equivalent_paths() {
        let direct = ProjectKey::from_path(".").unwrap();
        let absolute = ProjectKey::from_path(std::env::current_dir().unwrap()).unwrap();
        assert_eq!(direct.workspace_id(), absolute.workspace_id());
        assert_eq!(direct.workspace_id().len(), 16);
    }
}
