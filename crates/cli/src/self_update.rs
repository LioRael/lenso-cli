use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lenso_cli::{Argument, ArgumentKind, Cli, CommandSpec, Context, Error, Outcome};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

const REPO: &str = "LioRael/lenso-cli";
const RECEIPT: &str = "lenso-install.json";
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_METADATA: u64 = 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    method: String,
    version: String,
    target: String,
    binary: String,
    binary_sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct Package {
    version: String,
    target: String,
    binary: String,
    binary_sha256: String,
}

pub fn register(cli: &mut Cli, offline: bool) -> Result<(), Error> {
    cli.command(
        CommandSpec {
            path: vec!["self".into(), "update".into()],
            workflow: "lenso/self-update".into(),
            about: "Update the native CLI executable".into(),
            provider: "lenso".into(),
            arguments: [
                (
                    "check",
                    "Check for an update without changing files",
                    ArgumentKind::Flag,
                ),
                (
                    "version",
                    "Install an exact release version (permits downgrade)",
                    ArgumentKind::Text,
                ),
                (
                    "adopt",
                    "Take ownership of an unmanaged executable",
                    ArgumentKind::Flag,
                ),
            ]
            .into_iter()
            .map(|(name, help, kind)| Argument {
                name: name.into(),
                help: help.into(),
                positional: false,
                kind,
            })
            .collect(),
        },
        move |ctx| async move {
            tokio::task::spawn_blocking(move || run(ctx, offline))
                .await
                .map_err(|e| Error::Operation(e.to_string()))?
        },
    )
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn file_digest(path: &Path) -> Result<String, Error> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn target() -> Result<&'static str, Error> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") if cfg!(target_env = "gnu") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") if cfg!(target_env = "gnu") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "x86_64") if cfg!(target_env = "msvc") => Ok("x86_64-pc-windows-msvc"),
        _ => Err(invalid("no native release supports this build target")),
    }
}

pub(super) fn managed_by(executable: &Path) -> Option<&'static str> {
    if executable
        .components()
        .any(|c| c.as_os_str() == "Cellar" || c.as_os_str() == "Homebrew")
    {
        return Some("Homebrew owns this executable; use brew upgrade with its formula name");
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".cargo"))
        });
    let cargo_receipt = executable
        .parent()
        .and_then(Path::parent)
        .is_some_and(|root| {
            root.join(".crates2.json").is_file() || root.join(".crates.toml").is_file()
        });
    if cargo_receipt
        || cargo_home.is_some_and(|home| {
            let bin = home.join("bin");
            executable.parent() == Some(fs::canonicalize(&bin).unwrap_or(bin).as_path())
        })
    {
        return Some("Cargo owns this executable; use cargo install lenso --locked --force");
    }
    None
}

fn receipt(executable: &Path, platform: &str) -> Result<Option<Receipt>, Error> {
    let path = executable.with_file_name(RECEIPT);
    let bytes = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => read_file(&path, MAX_METADATA)?,
        Ok(_) => return Err(invalid("installation receipt must be a regular file")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let receipt: Receipt = serde_json::from_slice(&bytes)?;
    if receipt.schema_version != 1
        || receipt.method != "native"
        || receipt.version != env!("CARGO_PKG_VERSION")
        || receipt.target != platform
        || Some(std::ffi::OsStr::new(&receipt.binary)) != executable.file_name()
        || receipt.binary_sha256 != file_digest(executable)?
    {
        return Err(invalid(
            "installation receipt does not match this executable; reinstall with the official installer",
        ));
    }
    Ok(Some(receipt))
}

fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
    read_limited(fs::File::open(path)?, limit)
}
fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("download or file exceeds size limit"));
    }
    Ok(bytes)
}

struct Github {
    client: reqwest::blocking::Client,
    token: Option<String>,
}

pub(super) fn latest_public_version(timeout: Duration) -> Result<Version, Error> {
    Github::anonymous(timeout)?.public_version(None)
}

impl Github {
    fn new() -> Result<Self, Error> {
        let mut github = Self::anonymous(Duration::from_secs(300))?;
        github.token = ["GH_TOKEN", "GITHUB_TOKEN"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|value| !value.is_empty());
        Ok(github)
    }
    fn anonymous(timeout: Duration) -> Result<Self, Error> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("lenso-self-update")
            .connect_timeout(timeout.min(Duration::from_secs(20)))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let url = attempt.url();
                let allowed = url.scheme() == "https"
                    && url.host_str().is_some_and(|host| {
                        host == "github.com"
                            || host == "api.github.com"
                            || host.ends_with(".githubusercontent.com")
                    });
                if attempt.previous().len() < 5 && allowed {
                    attempt.follow()
                } else {
                    attempt.error("invalid release download redirect")
                }
            }))
            .build()
            .map_err(|_| invalid("could not initialize update HTTP client"))?;
        Ok(Self {
            client,
            token: None,
        })
    }
    fn get(&self, endpoint: &str, accept: &str, limit: u64) -> Result<Vec<u8>, Error> {
        let mut request = self
            .client
            .get(format!("https://api.github.com/repos/{REPO}/{endpoint}"))
            .header(reqwest::header::ACCEPT, accept);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        response_bytes(send(request)?, limit)
    }
    fn public_version(&self, requested: Option<&Version>) -> Result<Version, Error> {
        if let Some(version) = requested {
            let url = format!("https://github.com/{REPO}/releases/download/v{version}/SHA256SUMS");
            send(self.client.head(url))?;
            return Ok(version.clone());
        }
        let response = send(
            self.client
                .head(format!("https://github.com/{REPO}/releases/latest")),
        )?;
        public_version(response.url())
    }
    fn public_asset(&self, version: &Version, name: &str, limit: u64) -> Result<Vec<u8>, Error> {
        let url = format!("https://github.com/{REPO}/releases/download/v{version}/{name}");
        response_bytes(send(self.client.get(url))?, limit)
    }
    fn asset(&self, asset: &Asset, limit: u64) -> Result<Vec<u8>, Error> {
        if asset.size > limit {
            return Err(invalid("release asset exceeds size limit"));
        }
        let bytes = self.get(
            &format!("releases/assets/{}", asset.id),
            "application/octet-stream",
            limit,
        )?;
        if bytes.len() as u64 != asset.size {
            return Err(invalid("release asset size mismatch"));
        }
        if let Some(expected) = &asset.digest
            && expected != &format!("sha256:{}", digest(&bytes))
        {
            return Err(invalid("release asset SHA-256 mismatch"));
        }
        Ok(bytes)
    }
}

fn send(request: reqwest::blocking::RequestBuilder) -> Result<reqwest::blocking::Response, Error> {
    let response = request
        .send()
        .map_err(|_| invalid("release request failed (check network access and proxy settings)"))?;
    if !response.status().is_success() {
        return Err(http_error(response.status(), response.headers()));
    }
    Ok(response)
}
fn response_bytes(response: reqwest::blocking::Response, limit: u64) -> Result<Vec<u8>, Error> {
    if response.content_length().is_some_and(|size| size > limit) {
        return Err(invalid("release asset exceeds size limit"));
    }
    read_limited(response, limit)
        .map_err(|_| invalid("release response is unreadable or exceeds the size limit"))
}
fn public_version(url: &reqwest::Url) -> Result<Version, Error> {
    let prefix = format!("/{REPO}/releases/tag/v");
    let version = url
        .path()
        .strip_prefix(&prefix)
        .and_then(|v| Version::parse(v).ok());
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("unexpected public release URL"));
    }
    version
        .filter(|v| v.pre.is_empty())
        .ok_or_else(|| invalid("latest public release is not a stable CLI version"))
}

fn http_error(status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) -> Error {
    let number = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
    };
    let advice = if status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || (status == reqwest::StatusCode::FORBIDDEN && number("x-ratelimit-remaining") == Some(0))
    {
        let mut message = "GitHub API rate limit reached; wait or use GH_TOKEN/GITHUB_TOKEN with repository read access".to_owned();
        if let Some(reset) = number("x-ratelimit-reset") {
            message.push_str(&format!("; reset at Unix timestamp {reset}"));
        }
        if let Some(delay) = number("retry-after") {
            message.push_str(&format!("; retry after {delay} seconds"));
        }
        message
    } else if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        "check GH_TOKEN/GITHUB_TOKEN, repository read access and GitHub API rate limits".into()
    } else if status == reqwest::StatusCode::NOT_FOUND {
        "release or asset not found; check the requested version and private repository access"
            .into()
    } else {
        "GitHub could not complete the release request; retry later".into()
    };
    Error::Operation(format!(
        "release request returned HTTP {}: {advice}",
        status.as_u16()
    ))
}

fn select_asset<'a>(release: &'a Release, name: &str) -> Result<&'a Asset, Error> {
    let matches: Vec<_> = release.assets.iter().filter(|a| a.name == name).collect();
    match matches.as_slice() {
        [asset] => Ok(asset),
        _ => Err(invalid("release asset is missing or ambiguous")),
    }
}

fn archive_hash(sums: &[u8], name: &str) -> Result<String, Error> {
    let text = std::str::from_utf8(sums).map_err(|_| invalid("invalid SHA256SUMS encoding"))?;
    let matches: Vec<_> = text
        .lines()
        .filter_map(|line| line.split_once("  "))
        .filter(|(_, file)| *file == name)
        .collect();
    match matches.as_slice() {
        [(hash, _)] if hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()) => {
            Ok(hash.to_ascii_lowercase())
        }
        _ => Err(invalid("archive checksum is missing or ambiguous")),
    }
}

fn unpack(
    bytes: &[u8],
    windows: bool,
    version: &str,
    platform: &str,
    directory: &Path,
) -> Result<PathBuf, Error> {
    let binary_name = if windows { "lenso.exe" } else { "lenso" };
    let mut files = std::collections::BTreeMap::new();
    let mut add = |name: &str, body: Vec<u8>| -> Result<(), Error> {
        if ![binary_name, "release.json", "README.txt"].contains(&name)
            || files.insert(name.to_owned(), body).is_some()
        {
            return Err(invalid("unexpected or duplicate archive entry"));
        }
        Ok(())
    };
    if windows {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|_| invalid("invalid release ZIP"))?;
        if archive.len() != 3 {
            return Err(invalid("unexpected archive entry count"));
        }
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|_| invalid("invalid ZIP entry"))?;
            if !entry.is_file() || entry.is_symlink() {
                return Err(invalid("archive contains a non-regular file"));
            }
            let name = entry.name().to_owned();
            let limit = if name == binary_name {
                MAX_ARCHIVE
            } else {
                MAX_METADATA
            };
            add(&name, read_limited(entry, limit)?)?;
        }
    } else {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder.take(MAX_ARCHIVE + 4 * MAX_METADATA));
        for entry in archive.entries()? {
            let entry = entry?;
            if !entry.header().entry_type().is_file() {
                return Err(invalid("archive contains a non-regular file"));
            }
            let name = entry
                .path()?
                .to_str()
                .ok_or_else(|| invalid("invalid archive path"))?
                .to_owned();
            let limit = if name == binary_name {
                MAX_ARCHIVE
            } else {
                MAX_METADATA
            };
            add(&name, read_limited(entry, limit)?)?;
        }
    }
    let metadata: Package = serde_json::from_slice(
        files
            .get("release.json")
            .ok_or_else(|| invalid("missing release metadata"))?,
    )?;
    let binary = files
        .get(binary_name)
        .ok_or_else(|| invalid("missing release binary"))?;
    if metadata.version != version
        || metadata.target != platform
        || metadata.binary != binary_name
        || metadata.binary_sha256 != digest(binary)
    {
        return Err(invalid("release binary identity or SHA-256 mismatch"));
    }
    let path = directory.join(binary_name);
    fs::write(&path, binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

fn run(ctx: Context, offline: bool) -> Result<Outcome, Error> {
    let flag = |name: &str| -> Result<bool, Error> {
        ctx.input
            .get(name)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| invalid("update flags must be booleans"))
            })
            .transpose()
            .map(|v| v.unwrap_or(false))
    };
    let check = flag("check")?;
    let adopt = flag("adopt")?;
    let requested = ctx
        .input
        .get("version")
        .map(|v| {
            v.as_str()
                .ok_or_else(|| invalid("--version must be a semantic version"))
                .and_then(|s| {
                    Version::parse(s).map_err(|_| invalid("--version must be a semantic version"))
                })
        })
        .transpose()?;
    let executable = fs::canonicalize(std::env::current_exe()?)?;
    if let Some(message) = managed_by(&executable) {
        return Ok(Outcome::complete(
            json!({"message":message,"state":"managed","path":executable}),
        ));
    }
    let platform = target()?;
    let installed = receipt(&executable, platform)?;
    if installed.is_none() && !adopt && !check {
        return Err(Error::Permission(format!(
            "unmanaged executable: {}; use --adopt to allow native self-updates, or upgrade through its package manager",
            executable.display()
        )));
    }
    if offline {
        return Err(invalid(
            "self update requires network access; --offline prevents release checks",
        ));
    }
    let github = Github::new()?;
    let (version, release) = if github.token.is_none() {
        (github.public_version(requested.as_ref())?, None)
    } else {
        let endpoint = requested.as_ref().map_or_else(
            || "releases/latest".to_owned(),
            |v| format!("releases/tags/v{v}"),
        );
        let release: Release = serde_json::from_slice(&github.get(
            &endpoint,
            "application/vnd.github+json",
            MAX_METADATA,
        )?)?;
        let version = release
            .tag_name
            .strip_prefix('v')
            .and_then(|s| Version::parse(s).ok())
            .ok_or_else(|| invalid("release does not have a CLI version tag"))?;
        if release.draft
            || requested.as_ref().is_some_and(|v| *v != version)
            || (requested.is_none() && (release.prerelease || !version.pre.is_empty()))
        {
            return Err(invalid(
                "release does not match the requested update channel/version",
            ));
        }
        (version, Some(release))
    };
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| invalid("invalid current CLI version"))?;
    let change = if requested.is_some() {
        version != current
    } else {
        version > current
    };
    if !check && adopt && installed.is_none() && requested.is_none() && version < current {
        return Err(invalid(
            "installed CLI is newer than the stable release; use --version to explicitly select a version for adoption",
        ));
    }
    let message = if change {
        format!("Lenso {current} → {version}")
    } else {
        format!("Lenso {current} is already up to date")
    };
    if check || (!change && installed.is_some()) {
        return Ok(Outcome::complete(
            json!({"message":message,"state":if change {"available"} else {"current"},"current_version":current.to_string(),"version":version.to_string(),"path":executable}),
        ));
    }
    let directory = executable
        .parent()
        .ok_or_else(|| invalid("executable has no parent directory"))?;
    let lock_path = directory.join(".lenso-update.lock");
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err(invalid("update lock must be a regular file"));
    }
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock()
        .map_err(|_| invalid("another CLI update is running"))?;
    let before = file_digest(&executable)?;
    // Recheck after taking the lock: another updater may have replaced our executable.
    receipt(&executable, platform)?;
    let windows = platform.contains("windows");
    let archive_name = format!(
        "lenso-v{version}-{platform}.{}",
        if windows { "zip" } else { "tar.gz" }
    );
    let sums = match &release {
        Some(release) => github.asset(select_asset(release, "SHA256SUMS")?, MAX_METADATA)?,
        None => github.public_asset(&version, "SHA256SUMS", MAX_METADATA)?,
    };
    let expected = archive_hash(&sums, &archive_name)?;
    ctx.events
        .preparing(format!("Downloading Lenso {version} ({platform})"));
    let archive = match &release {
        Some(release) => github.asset(select_asset(release, &archive_name)?, MAX_ARCHIVE)?,
        None => github.public_asset(&version, &archive_name, MAX_ARCHIVE)?,
    };
    if digest(&archive) != expected {
        return Err(invalid(
            "archive SHA-256 mismatch; existing installation was not changed",
        ));
    }
    let staging = tempfile::tempdir_in(directory)?;
    let binary = unpack(
        &archive,
        windows,
        &version.to_string(),
        platform,
        staging.path(),
    )?;
    let reported = std::process::Command::new(&binary)
        .arg("--version")
        .env("PATH", "")
        .output()?;
    if !reported.status.success()
        || String::from_utf8_lossy(&reported.stdout).trim() != format!("lenso {version}")
    {
        return Err(invalid(
            "downloaded executable version mismatch; existing installation was not changed",
        ));
    }
    let next = Receipt {
        schema_version: 1,
        method: "native".into(),
        version: version.to_string(),
        target: platform.into(),
        binary: executable
            .file_name()
            .ok_or_else(|| invalid("missing executable name"))?
            .to_string_lossy()
            .into_owned(),
        binary_sha256: file_digest(&binary)?,
    };
    replace(&executable, &binary, &next, &before)?;
    Ok(Outcome::complete(
        json!({"message":format!("Installed Lenso {version} at {}", executable.display()),"state":"updated","previous_version":current.to_string(),"version":version.to_string(),"path":executable}),
    ))
}

fn replace(executable: &Path, binary: &Path, next: &Receipt, before: &str) -> Result<(), Error> {
    let directory = executable
        .parent()
        .ok_or_else(|| invalid("executable has no parent directory"))?;
    let mut staged_receipt = tempfile::NamedTempFile::new_in(directory)?;
    staged_receipt.write_all(&serde_json::to_vec_pretty(&next)?)?;
    staged_receipt.as_file().sync_all()?;
    if file_digest(executable)? != before {
        return Err(invalid(
            "executable changed during update; retry using the installed CLI",
        ));
    }
    replace_binary(executable, binary)?;
    staged_receipt
        .persist(directory.join(RECEIPT))
        .map_err(|e| {
            Error::Operation(format!(
                "CLI replaced, but installation receipt could not be saved: {}",
                e.error
            ))
        })?;
    Ok(())
}

fn replace_binary(executable: &Path, binary: &Path) -> Result<(), Error> {
    #[cfg(windows)]
    {
        // Windows replacement moves the running image before copying the new file.
        // Keep an independent copy so a failed copy/rename can restore the old path.
        let backup = tempfile::NamedTempFile::new_in(
            executable
                .parent()
                .ok_or_else(|| invalid("executable has no parent directory"))?,
        )?;
        fs::copy(executable, backup.path())?;
        backup.as_file().sync_all()?;
        if let Err(error) = self_replace::self_replace(binary) {
            if !executable.exists() {
                backup.persist(executable).map_err(|recovery| {
                    let cause = recovery.error.to_string();
                    let saved = recovery.file.into_temp_path().keep();
                    Error::Operation(format!("replacement failed: {error}; restore failed: {cause}; recovery copy: {saved:?}"))
                })?;
            }
            return Err(error.into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = executable;
        self_replace::self_replace(binary)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_child() {
        if std::env::var_os("LENSO_TEST_REPLACEMENT_CHILD").is_none() {
            return;
        }
        let executable = std::env::current_exe().unwrap();
        let directory = executable.parent().unwrap();
        let binary = directory.join("replacement");
        fs::copy(&executable, &binary).unwrap();
        let before = file_digest(&executable).unwrap();
        let next = Receipt {
            schema_version: 1,
            method: "native".into(),
            version: "2.0.0".into(),
            target: target().unwrap().into(),
            binary: executable.file_name().unwrap().to_str().unwrap().into(),
            binary_sha256: before.clone(),
        };
        #[cfg(windows)]
        if std::env::var("LENSO_TEST_REPLACEMENT_CHILD").as_deref() == Ok("failure") {
            assert!(
                replace(
                    &executable,
                    &directory.join("missing-binary"),
                    &next,
                    &before
                )
                .is_err()
            );
            assert_eq!(file_digest(&executable).unwrap(), before);
            assert!(!directory.join(RECEIPT).exists());
            return;
        }
        assert!(replace(&executable, &binary, &next, "concurrent change").is_err());
        assert!(!directory.join(RECEIPT).exists());
        replace(&executable, &binary, &next, &before).unwrap();
    }

    #[test]
    fn running_executable_is_replaced_in_an_isolated_child() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory
            .path()
            .join(if cfg!(windows) { "lenso.exe" } else { "lenso" });
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new(&executable)
                .args(["--exact", "self_update::tests::replacement_child"])
                .env("LENSO_TEST_REPLACEMENT_CHILD", "failure")
                .status()
                .unwrap();
            assert!(status.success());
        }
        let output = std::process::Command::new(&executable)
            .args([
                "--exact",
                "self_update::tests::replacement_child",
                "--nocapture",
            ])
            .env("LENSO_TEST_REPLACEMENT_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let record: Receipt =
            serde_json::from_slice(&fs::read(directory.path().join(RECEIPT)).unwrap()).unwrap();
        assert_eq!(record.version, "2.0.0");
        assert_eq!(record.binary_sha256, file_digest(&executable).unwrap());
        let output = std::process::Command::new(executable)
            .arg("--list")
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    #[test]
    fn public_latest_requires_the_official_stable_release_url() {
        let valid = reqwest::Url::parse("https://github.com/LioRael/lenso-cli/releases/tag/v1.1.1")
            .unwrap();
        assert_eq!(public_version(&valid).unwrap(), Version::new(1, 1, 1));
        for url in [
            "http://github.com/LioRael/lenso-cli/releases/tag/v1.1.1",
            "https://example.com/LioRael/lenso-cli/releases/tag/v1.1.1",
            "https://github.com/other/repo/releases/tag/v1.1.1",
            "https://github.com/LioRael/lenso-cli/releases/tag/code-v1.1.1",
            "https://github.com/LioRael/lenso-cli/releases/tag/v1.1.1-beta.1",
            "https://github.com/LioRael/lenso-cli/releases/tag/v1.1.1?redirect=other",
        ] {
            assert!(
                public_version(&reqwest::Url::parse(url).unwrap()).is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn github_errors_distinguish_rate_limits_and_access_failures() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1234567890".parse().unwrap());
        let error = http_error(reqwest::StatusCode::FORBIDDEN, &headers).to_string();
        assert!(error.contains("rate limit reached") && error.contains("1234567890"));
        headers.clear();
        assert!(
            http_error(reqwest::StatusCode::FORBIDDEN, &headers)
                .to_string()
                .contains("repository read access")
        );
        assert!(
            http_error(reqwest::StatusCode::NOT_FOUND, &headers)
                .to_string()
                .contains("requested version")
        );
        headers.insert("retry-after", "60".parse().unwrap());
        assert!(
            http_error(reqwest::StatusCode::TOO_MANY_REQUESTS, &headers)
                .to_string()
                .contains("60 seconds")
        );
    }

    fn package(binary: &[u8], metadata: serde_json::Value) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, body) in [
            ("lenso", binary.to_vec()),
            ("release.json", serde_json::to_vec(&metadata).unwrap()),
            ("README.txt", vec![]),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive
                .append_data(&mut header, name, body.as_slice())
                .unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn package_identity_is_verified_before_staging_executable() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = json!({"version":"2.0.0","target":"aarch64-apple-darwin","binary":"lenso","binary_sha256":digest(b"binary")});
        let bytes = package(b"binary", metadata.clone());
        assert!(
            unpack(
                &bytes,
                false,
                "2.0.0",
                "x86_64-apple-darwin",
                directory.path()
            )
            .is_err()
        );
        assert!(!directory.path().join("lenso").exists());
        assert!(
            unpack(
                &bytes,
                false,
                "1.0.0",
                "aarch64-apple-darwin",
                directory.path()
            )
            .is_err()
        );
        let corrupt = package(b"tampered", metadata);
        assert!(
            unpack(
                &corrupt,
                false,
                "2.0.0",
                "aarch64-apple-darwin",
                directory.path()
            )
            .is_err()
        );
        assert!(!directory.path().join("lenso").exists());
        let binary = unpack(
            &bytes,
            false,
            "2.0.0",
            "aarch64-apple-darwin",
            directory.path(),
        )
        .unwrap();
        assert_eq!(fs::read(binary).unwrap(), b"binary");
    }

    #[test]
    fn zip_identity_and_symlinks_are_checked() {
        fn zip(symlink: bool) -> Vec<u8> {
            let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            if symlink {
                archive
                    .add_symlink("lenso.exe", "elsewhere", options)
                    .unwrap();
            } else {
                archive.start_file("lenso.exe", options).unwrap();
                archive.write_all(b"binary").unwrap();
            }
            archive.start_file("release.json", options).unwrap();
            archive.write_all(&serde_json::to_vec(&json!({"version":"2.0.0","target":"x86_64-pc-windows-msvc","binary":"lenso.exe","binary_sha256":digest(b"binary")})).unwrap()).unwrap();
            archive.start_file("README.txt", options).unwrap();
            archive.finish().unwrap().into_inner()
        }
        let directory = tempfile::tempdir().unwrap();
        assert!(
            unpack(
                &zip(true),
                true,
                "2.0.0",
                "x86_64-pc-windows-msvc",
                directory.path()
            )
            .is_err()
        );
        assert!(!directory.path().join("lenso.exe").exists());
        assert!(
            unpack(
                &zip(false),
                true,
                "2.0.0",
                "x86_64-pc-windows-msvc",
                directory.path()
            )
            .is_ok()
        );
    }

    #[test]
    fn checksum_and_asset_ambiguity_are_rejected() {
        let hash = digest(b"archive");
        let line = format!("{hash}  archive.tar.gz\n");
        assert_eq!(
            archive_hash(line.as_bytes(), "archive.tar.gz").unwrap(),
            hash
        );
        assert!(archive_hash(line.repeat(2).as_bytes(), "archive.tar.gz").is_err());
        assert!(archive_hash(b"invalid  archive.tar.gz", "archive.tar.gz").is_err());
        assert!(archive_hash(line.as_bytes(), "other.tar.gz").is_err());
        let release: Release = serde_json::from_value(json!({"tag_name":"v2.0.0","draft":false,"prerelease":false,"assets":[{"id":1,"name":"archive","size":1},{"id":2,"name":"archive","size":1}]})).unwrap();
        assert!(select_asset(&release, "archive").is_err());
    }

    #[test]
    fn receipt_binds_binary_version_and_platform() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("lenso");
        fs::write(&executable, b"original").unwrap();
        assert!(receipt(&executable, "test-target").unwrap().is_none());
        let record = Receipt {
            schema_version: 1,
            method: "native".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            target: "test-target".into(),
            binary: "lenso".into(),
            binary_sha256: digest(b"original"),
        };
        fs::write(
            directory.path().join(RECEIPT),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(receipt(&executable, "test-target").unwrap().is_some());
        assert!(receipt(&executable, "wrong-target").is_err());
        fs::write(&executable, b"replaced-by-another-installer").unwrap();
        assert!(receipt(&executable, "test-target").is_err());
    }

    #[test]
    fn bounded_reads_reject_overflow() {
        assert_eq!(read_limited(b"123".as_slice(), 3).unwrap(), b"123");
        assert!(read_limited(b"1234".as_slice(), 3).is_err());
    }
}
