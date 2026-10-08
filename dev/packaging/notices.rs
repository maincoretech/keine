//! Shared, verbatim distribution notices for Publisher and Editor playtest exports.
use std::io::{self, Read};
use std::path::Path;

pub const BASE: &str = concat!(
    "Kēne distribution notices\n",
    "Source document names below identify sections in this file or the upstream source tree.\n",
    "Each component retains its own license; game content is not licensed by the engine.\n",
    "\n===== NOTICE =====\n",
    include_str!("../../NOTICE"),
    "\n===== LICENSE =====\n",
    include_str!("../../LICENSE"),
    "\n===== FONT-LICENSES.txt =====\n",
    include_str!("../../src/assets/fonts/FONT-LICENSES.txt"),
);

pub fn append(document: &mut String, name: &str, text: &str) {
    document.push_str(&format!("\n===== {name} =====\n"));
    document.push_str(text);
}

pub fn game(project: &Path) -> anyhow::Result<Option<String>> {
    let mount = keine_loader::ContentMount::new(
        keine_loader::ContentBackend::FileSystem(project.canonicalize()?),
        "",
    )?;
    match mount.open_file(Path::new("LICENSE")) {
        Ok(file) => Ok(Some(read(file)?)),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub fn read(input: impl Read) -> io::Result<String> {
    // Notices are text documents, not an unbounded resource-loading path.
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    input.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "notice exceeds 4 MiB",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if text.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty notice"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_document_preserves_complete_original_terms() {
        for text in [
            include_str!("../../NOTICE"),
            include_str!("../../LICENSE"),
            include_str!("../../src/assets/fonts/FONT-LICENSES.txt"),
        ] {
            assert!(BASE.contains(text));
        }
        let mut document = BASE.to_owned();
        let license = "Copyright author.\r\nAll rights reserved.\r\n";
        append(&mut document, "GAME-LICENSE", license);
        assert!(document.ends_with(license));
        assert!(document.contains(BASE));
    }
}
