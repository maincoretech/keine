//! Exercise the generated code with the optimizer, and the build-input boundary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "../build/runtime_keys.rs"]
mod runtime_keys;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "keine-key-build-{}-{nonce}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compile(source: &Path, executable: &Path) {
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition=2024", "-O", "-C", "lto", "-C", "strip=symbols"])
        .arg(source)
        .arg("-o")
        .arg(executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn optimized_reconstruction_preserves_keys_across_fresh_splits() {
    let scratch = Scratch::new();
    let root: [u8; 32] = std::array::from_fn(|index| (index * 17) as u8);
    let public = [0xa5; 32];
    let mut source = String::new();
    let mut checks = String::new();
    let mut previous = String::new();
    for variant in 0..8 {
        let a = std::array::from_fn(|index| (index * 29 + index / 4 + variant * 31) as u8);
        let b = std::array::from_fn(|index| a[index] ^ root[index]);
        let generated = runtime_keys::generate(&a, &b, &public);
        assert_ne!(generated, previous);
        source.push_str(&format!("mod case_{variant} {{\n{generated}}}\n"));
        checks.push_str(&format!(
            "assert!(case_{variant}::load() == ({root:?}, {public:?}));\n"
        ));
        previous = generated;
    }
    source.push_str(&format!("fn main() {{\n{checks}}}\n"));
    let path = scratch.0.join("probe.rs");
    let executable = scratch
        .0
        .join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    fs::write(&path, source).unwrap();
    compile(&path, &executable);
    assert!(Command::new(executable).status().unwrap().success());
}

#[test]
fn build_requires_complete_valid_material_and_tracks_input_files() {
    const VARIABLES: [&str; 3] = [
        "KEINE_HAKUTAKU_KEY_SHARE_A",
        "KEINE_HAKUTAKU_KEY_SHARE_B",
        "KEINE_HAKUTAKU_PUBLIC_KEY",
    ];
    let scratch = Scratch::new();
    let executable = scratch
        .0
        .join(format!("build-script{}", std::env::consts::EXE_SUFFIX));
    compile(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs"),
        &executable,
    );
    let paths = VARIABLES.map(|variable| scratch.0.join(variable));
    for path in &paths {
        fs::write(path, [0x5a; 32]).unwrap();
    }
    let run = |count: usize| -> Output {
        let mut command = Command::new(&executable);
        command.env("OUT_DIR", &scratch.0);
        for variable in VARIABLES {
            command.env_remove(variable);
        }
        for index in 0..count {
            command.env(VARIABLES[index], &paths[index]);
        }
        command.output().unwrap()
    };
    let generated = scratch.0.join("hakutaku-runtime-keys.rs");
    assert!(run(0).status.success());
    assert!(fs::read(&generated).unwrap().is_empty());
    for count in [1, 2] {
        let output = run(count);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("must be supplied together"));
    }
    fs::write(&paths[1], [0x5a; 31]).unwrap();
    let invalid = run(3);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("raw 32-byte key"));
    fs::write(&paths[1], [0x5a; 32]).unwrap();
    let valid = run(3);
    assert!(valid.status.success());
    let stdout = String::from_utf8(valid.stdout).unwrap();
    assert!(stdout.contains("cargo:rustc-cfg=keine_packaged_keys\n"));
    for path in paths {
        assert!(stdout.contains(&format!("cargo:rerun-if-changed={}\n", path.display())));
    }
    assert!(!fs::read(&generated).unwrap().is_empty());
    for filename in ["hakutaku-key-share-a.bin", "hakutaku-key-share-b.bin"] {
        assert!(!scratch.0.join(filename).exists());
    }
}
