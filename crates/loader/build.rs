//! Obfuscates the Hexz resource key at build time so the plaintext never
//! lands in the compiled binary. The ciphertext and its mask are written to
//! OUT_DIR and reassembled at runtime by `hexz_password()`.
//!
//! This is deliberately weak distribution protection, not DRM: a determined
//! attacker can still recover the key from a running process. It only raises
//! the cost above a trivial `strings`/binwalk extraction.

use std::env;
use std::fs;
use std::path::PathBuf;

/// Fallback used by development builds, which never open encrypted archives.
const DEFAULT_HEXZ_PASSWORD: &str = "keine-hexz-resource-v1";

/// Fixed rotation mask; enough to keep the key out of plaintext string tables.
const MASK: [u8; 16] = *b"keine-hexz-masks";

fn main() {
    println!("cargo:rerun-if-env-changed=HEXZ_PASSWORD");
    println!("cargo:rerun-if-env-changed=KEINE_HEXZ_PUBLIC_KEY");
    let password = env::var("HEXZ_PASSWORD").unwrap_or_else(|_| DEFAULT_HEXZ_PASSWORD.to_owned());
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out.join("hexz-password.bin"), xor(password.as_bytes()))
        .expect("failed to write obfuscated key");
    fs::write(out.join("hexz-password-mask.bin"), MASK).expect("failed to write key mask");

    let public_key = env::var_os("KEINE_HEXZ_PUBLIC_KEY").map_or_else(
        || vec![0; 32],
        |path| fs::read(path).expect("failed to read KEINE_HEXZ_PUBLIC_KEY"),
    );
    assert_eq!(
        public_key.len(),
        32,
        "KEINE_HEXZ_PUBLIC_KEY must contain one raw 32-byte Ed25519 public key"
    );
    fs::write(out.join("hexz-public-key.bin"), public_key)
        .expect("failed to write embedded Hexz public key");
}

fn xor(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .zip(MASK.iter().cycle())
        .map(|(byte, mask)| byte ^ mask)
        .collect()
}
