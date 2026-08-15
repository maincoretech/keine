//! Embeds the two Hakutaku runtime key shares and publisher public key.
//!
//! Development builds use all-zero placeholders and cannot open release
//! packages. `keine bundle` supplies all three files to the hardened engine
//! build; neither file contains the complete AES root key by itself.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MATERIAL: [(&str, &str); 3] = [
    ("KEINE_HAKUTAKU_KEY_SHARE_A", "hakutaku-key-share-a.bin"),
    ("KEINE_HAKUTAKU_KEY_SHARE_B", "hakutaku-key-share-b.bin"),
    ("KEINE_HAKUTAKU_PUBLIC_KEY", "hakutaku-public-key.bin"),
];

fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let configured = MATERIAL
        .iter()
        .filter(|(variable, _)| env::var_os(variable).is_some())
        .count();
    assert!(
        configured == 0 || configured == MATERIAL.len(),
        "Hakutaku runtime key paths must be supplied together"
    );

    for (variable, filename) in MATERIAL {
        println!("cargo:rerun-if-env-changed={variable}");
        let bytes = env::var_os(variable)
            .map_or_else(|| vec![0; 32], |path| read_key(variable, Path::new(&path)));
        fs::write(output.join(filename), bytes)
            .unwrap_or_else(|error| panic!("failed to write {filename}: {error}"));
    }
}

fn read_key(variable: &str, path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        panic!("failed to read {variable} from {}: {error}", path.display())
    });
    assert_eq!(
        bytes.len(),
        32,
        "{variable} must point to a raw 32-byte key"
    );
    bytes
}
