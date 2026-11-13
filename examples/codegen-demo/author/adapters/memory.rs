use crate::business::Store;
use std::collections::BTreeMap;
use std::io;
use std::sync::Mutex;

#[derive(Default)]
pub struct MemoryStore(Mutex<BTreeMap<String, String>>);
impl Store for MemoryStore {
    fn set(&self, key: &str, value: &str) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?
            .insert(key.into(), value.into());
        Ok(())
    }
    fn get(&self, key: &str) -> io::Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| io::Error::other("store lock poisoned"))?
            .get(key)
            .cloned())
    }
}
