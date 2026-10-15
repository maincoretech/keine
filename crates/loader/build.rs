//! Generates a small, per-bundle runtime key reconstruction function.
//! This changes static extraction patterns, not the offline security boundary.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "build/runtime_keys.rs"]
mod runtime_keys;

const MATERIAL: [&str; 3] = [
    "KEINE_HAKUTAKU_KEY_SHARE_A",
    "KEINE_HAKUTAKU_KEY_SHARE_B",
    "KEINE_HAKUTAKU_PUBLIC_KEY",
];

fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/runtime_keys.rs");
    println!("cargo:rustc-check-cfg=cfg(keine_packaged_keys)");
    for variable in MATERIAL {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    let configured = MATERIAL
        .iter()
        .filter(|variable| env::var_os(variable).is_some())
        .count();
    assert!(
        configured == 0 || configured == MATERIAL.len(),
        "Hakutaku runtime key paths must be supplied together"
    );

    let source = if configured == MATERIAL.len() {
        let [share_a, share_b, public_key] = MATERIAL.map(|variable| {
            let path = env::var_os(variable).expect("all key paths are configured");
            let path = Path::new(&path);
            println!("cargo:rerun-if-changed={}", path.display());
            read_key(variable, path)
        });
        println!("cargo:rustc-cfg=keine_packaged_keys");
        runtime_keys::generate(&share_a, &share_b, &public_key)
    } else {
        String::new()
    };
    fs::write(output.join("hakutaku-runtime-keys.rs"), source)
        .expect("failed to write Hakutaku runtime key source");
}

fn read_key(variable: &str, path: &Path) -> [u8; 32] {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        panic!("failed to read {variable} from {}: {error}", path.display())
    });
    bytes
        .try_into()
        .unwrap_or_else(|_| panic!("{variable} must point to a raw 32-byte key"))
}
