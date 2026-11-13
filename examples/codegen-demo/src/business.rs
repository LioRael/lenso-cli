use std::io;
use std::sync::Arc;

pub trait Store: Send + Sync {
    fn set(&self, key: &str, value: &str) -> io::Result<()>;
    fn get(&self, key: &str) -> io::Result<Option<String>>;
}

pub struct Greeter {
    prefix: String,
    store: Arc<dyn Store>,
}
impl Greeter {
    pub fn new(prefix: String, store: Arc<dyn Store>) -> Self {
        Self { prefix, store }
    }
    pub fn greet(&self, name: &str) -> io::Result<String> {
        self.store.set("name", name)?;
        let name = self
            .store
            .get("name")?
            .ok_or_else(|| io::Error::other("missing greeting name"))?;
        Ok(format!("{} {}", self.prefix, name))
    }
}
