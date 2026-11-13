use crate::business::Store;
use std::{fs, io, path::PathBuf};

pub struct DiskStore(PathBuf);
impl DiskStore {
    pub fn new(path: &str) -> io::Result<Self> {
        fs::create_dir_all(path)?;
        Ok(Self(path.into()))
    }
    fn key(&self, key: &str) -> PathBuf {
        let key: String = key.bytes().map(|byte| format!("{byte:02x}")).collect();
        self.0.join(key)
    }
}
impl Store for DiskStore {
    fn set(&self, key: &str, value: &str) -> io::Result<()> {
        fs::write(self.key(key), value)
    }
    fn get(&self, key: &str) -> io::Result<Option<String>> {
        match fs::read_to_string(self.key(key)) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}
