//! Host-only icon tool for CI; no Engine, Editor or publisher identity required.
fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let source = args
        .next()
        .expect("icons <source.png|source.webp> <fresh-output>");
    let output = args
        .next()
        .expect("icons <source.png|source.webp> <fresh-output>");
    assert!(args.next().is_none(), "unexpected argument");
    let output = std::path::PathBuf::from(output);
    if output.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "choose a fresh icon output directory",
        ));
    }
    keine_media::icons::IconSet::read(std::fs::File::open(source)?)?.write(&output)
}
