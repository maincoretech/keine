//! A bounded, cursor-independent window into an uncompressed APK entry.
use keine_loader::{HakutakuError, PositionedFile};
use std::fs::File;
use std::os::unix::fs::FileExt;

pub(super) struct ApkFile {
    file: File,
    offset: u64,
    size: u64,
}

impl ApkFile {
    pub(super) fn new(file: File, offset: u64, size: u64) -> Result<Self, HakutakuError> {
        let file_size = file.metadata()?.len();
        if offset.checked_add(size).is_none_or(|end| end > file_size) {
            return Err(HakutakuError::InvalidRange);
        }
        Ok(Self { file, offset, size })
    }
}

impl PositionedFile for ApkFile {
    fn len(&self) -> Result<u64, HakutakuError> {
        Ok(self.size)
    }
    fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> Result<(), HakutakuError> {
        if offset
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.size)
        {
            return Err(HakutakuError::InvalidRange);
        }
        let offset = self
            .offset
            .checked_add(offset)
            .ok_or(HakutakuError::InvalidRange)?;
        self.file.read_exact_at(bytes, offset).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn apk_reads_cannot_escape_into_adjacent_zip_entries() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("keine-apk-window-{}-{nonce}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        // Unix keeps this descriptor alive after unlinking the test file.
        std::fs::remove_file(path).unwrap();
        file.write_all(b"prefixPAYLOADsuffix").unwrap();
        assert!(ApkFile::new(file.try_clone().unwrap(), u64::MAX, 7).is_err());
        assert!(ApkFile::new(file.try_clone().unwrap(), 6, 100).is_err());
        let asset = ApkFile::new(file, 6, 7).unwrap();
        let mut bytes = [0; 4];
        asset.read_exact_at(2, &mut bytes).unwrap();
        assert_eq!(&bytes, b"YLOA");
        assert!(asset.read_exact_at(4, &mut bytes).is_err());
        assert!(asset.read_exact_at(u64::MAX, &mut bytes).is_err());
        assert!(asset.read_exact_at(8, &mut []).is_err());
        asset.read_exact_at(7, &mut []).unwrap();
        asset.read_exact_at(0, &mut bytes).unwrap();
        assert_eq!(&bytes, b"PAYL");
    }
}
