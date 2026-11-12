use std::fs::File;
use std::path::{Path, PathBuf};

use lenso_cli::Error;
use sha2::{Digest, Sha256};

use crate::project::{atomic_write, read_bounded};
use crate::project_path;

/// Per-user content-addressed download cache, shared by projects without sharing policy.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// Select LENSO_CACHE_DIR or the platform's per-user cache directory.
    ///
    /// # Errors
    /// Returns an error when no user directory is available or the override is relative.
    pub fn from_env() -> Result<Self, Error> {
        if let Some(path) = std::env::var_os("LENSO_CACHE_DIR") {
            return Self::at(PathBuf::from(path));
        }
        #[cfg(target_os = "macos")]
        let path = home()?.join("Library/Caches/lenso");
        #[cfg(target_os = "windows")]
        let path = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| {
                Error::Invalid("LOCALAPPDATA is unavailable; set LENSO_CACHE_DIR".into())
            })?
            .join("Lenso/Cache");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let path = match std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
            Some(path) if path.is_absolute() => path.join("lenso"),
            _ => home()?.join(".cache/lenso"),
        };
        Self::at(path)
    }

    /// Select an explicit cache root without creating it.
    ///
    /// # Errors
    /// Rejects relative paths.
    pub fn at(root: PathBuf) -> Result<Self, Error> {
        if !root.is_absolute() {
            return Err(Error::Invalid("cache directory must be absolute".into()));
        }
        Ok(Self { root })
    }

    /// User cache root; all versioned entries live below this directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn path(&self, relative: &Path) -> Result<PathBuf, Error> {
        project_path(&self.root, relative)
    }

    pub(crate) fn lock(&self, key: &str) -> Result<File, Error> {
        let relative = Path::new("v1/locks").join(format!("{}.lock", digest(key.as_bytes())));
        let path = self.path(&relative)?;
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| Error::Invalid("cache lock has no parent".into()))?,
        )?;
        let path = self.path(&relative)?;
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.lock()?;
        Ok(file)
    }

    pub(crate) fn blob_path(&self, sha256: &str) -> Result<PathBuf, Error> {
        check_digest(sha256)?;
        self.path(&Path::new("v1/blobs/sha256").join(sha256))
    }

    pub(crate) fn read(&self, sha256: &str, max: usize) -> Result<Option<Vec<u8>>, Error> {
        let path = self.blob_path(sha256)?;
        match read_bounded(&path, max) {
            Ok(bytes) if digest(&bytes) == sha256 => Ok(Some(bytes)),
            Ok(_) | Err(Error::Invalid(_)) => Ok(None),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn ensure(
        &self,
        sha256: &str,
        max: usize,
        load: impl FnOnce() -> Result<Vec<u8>, Error>,
    ) -> Result<PathBuf, Error> {
        if self.read(sha256, max)?.is_some() {
            return self.blob_path(sha256);
        }
        let _guard = self.lock(sha256)?;
        if self.read(sha256, max)?.is_none() {
            let bytes = load()?;
            if bytes.len() > max || digest(&bytes) != sha256 {
                return Err(Error::Permission(
                    "dependency digest mismatch; cached bytes were not replaced".into(),
                ));
            }
            let path = self.blob_path(sha256)?;
            std::fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| Error::Invalid("cache blob has no parent".into()))?,
            )?;
            atomic_write(&self.blob_path(sha256)?, &bytes)?;
        }
        self.blob_path(sha256)
    }

    pub(crate) fn metadata(&self, source: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.path(&Path::new("v1/metadata").join(digest(source.as_bytes())))?;
        let pin = match read_bounded(&path, 64) {
            Ok(bytes) => String::from_utf8(bytes).ok(),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(Error::Invalid(_)) => None,
            Err(error) => return Err(error),
        };
        match pin.filter(|pin| check_digest(pin).is_ok()) {
            Some(pin) => self.read(&pin, 1024 * 1024),
            None => Ok(None),
        }
    }

    pub(crate) fn save_metadata(&self, source: &str, bytes: &[u8]) -> Result<(), Error> {
        let pin = digest(bytes);
        self.ensure(&pin, 1024 * 1024, || Ok(bytes.to_vec()))?;
        let relative = Path::new("v1/metadata").join(digest(source.as_bytes()));
        let path = self.path(&relative)?;
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| Error::Invalid("metadata has no parent".into()))?,
        )?;
        atomic_write(&self.path(&relative)?, pin.as_bytes())
    }
}

#[cfg(not(target_os = "windows"))]
fn home() -> Result<PathBuf, Error> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| Error::Invalid("HOME is unavailable; set LENSO_CACHE_DIR".into()))
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn check_digest(value: &str) -> Result<(), Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid(
            "dependency SHA-256 must contain 64 lowercase hexadecimal digits".into(),
        ));
    }
    Ok(())
}
