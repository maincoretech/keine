fn main() {
    println!("cargo:rerun-if-env-changed=KEINE_APP_ICON_DIR");
    let icon = std::env::var_os("KEINE_APP_ICON_DIR")
        .map(std::path::PathBuf::from)
        .map(|directory| directory.join("keine.ico"))
        .unwrap_or_else(|| "../../src/assets/icons/keine.ico".into());
    println!("cargo:rerun-if-changed={}", icon.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    winresource::WindowsResource::new()
        .set_icon(icon.to_str().unwrap())
        .compile()
        .expect("failed to embed the Kēne Editor Windows icon");
}
