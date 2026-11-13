use std::ffi::OsString;
use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lenso_cli::{Cli, Error};
use semver::Version;
use serde::{Deserialize, Serialize};

const WORKER: &str = "__lenso_update_check";
const CACHE_FILE: &str = "v1/update-notice/result.json";
const LOCK_FILE: &str = "v1/update-notice/check.lock";
const SUCCESS_TTL: u64 = 24 * 60 * 60;
const FAILURE_TTL: u64 = 60 * 60;
const CHECK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cached {
    schema_version: u32,
    checked_at: u64,
    latest: Option<Version>,
}

impl Cached {
    fn fresh(&self, now: u64) -> bool {
        self.schema_version == 1
            && self.latest.as_ref().is_none_or(|v| v.pre.is_empty())
            && now.checked_sub(self.checked_at).is_some_and(|age| {
                age < if self.latest.is_some() {
                    SUCCESS_TTL
                } else {
                    FAILURE_TTL
                }
            })
    }

    fn newer(&self, now: u64, current: &Version) -> Option<&Version> {
        self.latest
            .as_ref()
            .filter(|v| self.fresh(now) && *v > current)
    }
}

pub fn is_worker(args: &[OsString]) -> bool {
    args.len() == 2 && args[1] == WORKER
}

fn disabled(env: impl Fn(&str) -> Option<OsString>) -> bool {
    [
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "BUILDKITE",
        "CIRCLECI",
        "TF_BUILD",
        "LENSO_NO_UPDATE_CHECK",
    ]
    .into_iter()
    .any(|name| {
        env(name).is_some_and(|value| {
            let value = value.to_string_lossy();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        })
    })
}

fn eligible(args: &[OsString], env: impl Fn(&str) -> Option<OsString>) -> bool {
    if disabled(&env)
        || env("TERM").as_deref() == Some(std::ffi::OsStr::new("dumb"))
        || Cli::command_from_args(args) == Some("self")
    {
        return false;
    }
    let mut arguments = args.iter().skip(1);
    while let Some(arg) = arguments.next() {
        if arg == "--" {
            break;
        }
        if ["--project", "--input", "--answers", "--color"]
            .iter()
            .any(|name| arg == *name)
        {
            arguments.next();
        } else if [
            "--json",
            "--offline",
            "--no-interactive",
            "--help",
            "-h",
            "--version",
            "-V",
        ]
        .iter()
        .any(|name| arg == *name)
        {
            return false;
        }
    }
    true
}

pub fn enabled(args: &[OsString]) -> bool {
    std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
        && eligible(args, |name| std::env::var_os(name))
}

fn now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|v| v.as_secs())
}

fn read(path: &Path) -> Option<Cached> {
    if !fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(4097)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn path(root: &Path, name: &str) -> Result<PathBuf, Error> {
    lenso_runtime::project_path(root, Path::new(name))
}

fn save(path: &Path, cached: &Cached) -> Result<(), Error> {
    let mut staged = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| Error::Invalid("cache has no parent".into()))?,
    )?;
    staged.write_all(&serde_json::to_vec(cached)?)?;
    staged.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn after_command() {
    let Some(now) = now() else {
        return;
    };
    let Ok(cache) = lenso_runtime::Cache::from_env() else {
        return;
    };
    let Ok(path) = path(cache.root(), CACHE_FILE) else {
        return;
    };
    if let Some(cached) = read(&path).filter(|v| v.fresh(now)) {
        if let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION"))
            && let Some(latest) = cached.newer(now, &current)
        {
            let advice = std::env::current_exe()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| crate::self_update::managed_by(&p))
                .unwrap_or("Run lenso self update to upgrade.");
            eprintln!("New Lenso version: {current} → {latest}. {advice}");
        }
        return;
    }
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    // The worker outlives short commands; no request or worker join runs in this process.
    let _ = worker_command(&executable).spawn();
}

fn worker_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg(WORKER)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

fn refresh(
    root: &Path,
    now: u64,
    check: impl FnOnce() -> Result<Version, Error>,
) -> Result<(), Error> {
    let result_path = path(root, CACHE_FILE)?;
    fs::create_dir_all(
        result_path
            .parent()
            .ok_or_else(|| Error::Invalid("cache has no parent".into()))?,
    )?;
    let lock_path = path(root, LOCK_FILE)?;
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.file_type().is_file()) {
        return Ok(());
    }
    let lock = fs::File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let result_path = path(root, CACHE_FILE)?;
    if read(&result_path).is_some_and(|v| v.fresh(now)) {
        return Ok(());
    }
    let mut cached = Cached {
        schema_version: 1,
        checked_at: now,
        latest: None,
    };
    // Reserve the retry interval before networking, including if the worker is interrupted.
    save(&result_path, &cached)?;
    if let Ok(version) = check() {
        cached.latest = Some(version);
        save(&result_path, &cached)?;
    }
    Ok(())
}

pub fn worker() {
    if disabled(|name| std::env::var_os(name)) {
        return;
    }
    if let (Ok(cache), Some(now)) = (lenso_runtime::Cache::from_env(), now()) {
        let _ = refresh(cache.root(), now, || {
            crate::self_update::latest_public_version(CHECK_TIMEOUT)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn __lenso_update_check() {
        let Some(root) = std::env::var_os("LENSO_TEST_NOTICE_WORKER") else {
            return;
        };
        refresh(Path::new(&root), 100, || Ok(Version::new(2, 0, 0))).unwrap();
    }

    #[test]
    fn background_worker_survives_a_dropped_child_handle() {
        let directory = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let child = worker_command(&executable)
            .env("LENSO_TEST_NOTICE_WORKER", directory.path())
            .spawn()
            .unwrap();
        drop(child);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let result = directory.path().join(CACHE_FILE);
        loop {
            if read(&result).is_some_and(|cached| cached.latest == Some(Version::new(2, 0, 0))) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background worker did not save its result"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn suppression_respects_flags_environment_and_argument_values() {
        for flag in [
            "--json",
            "--offline",
            "--no-interactive",
            "--help",
            "--version",
        ] {
            assert!(
                !eligible(&args(&["lenso", "cache", "path", flag]), |_| None),
                "{flag}"
            );
        }
        for name in [
            "CI",
            "GITHUB_ACTIONS",
            "GITLAB_CI",
            "TF_BUILD",
            "LENSO_NO_UPDATE_CHECK",
        ] {
            assert!(
                !eligible(&args(&["lenso", "cache", "path"]), |key| (key == name)
                    .then(|| "true".into())),
                "{name}"
            );
        }
        assert!(!eligible(&args(&["lenso", "self", "update"]), |_| None));
        assert!(eligible(
            &args(&["lenso", "--project", "--json", "cache", "path"]),
            |_| None
        ));
        assert!(eligible(
            &args(&["lenso", "run", "example", "--input", "--offline"]),
            |_| None
        ));
        assert!(eligible(&args(&["lenso", "cache", "path"]), |key| (key
            == "CI")
            .then(|| "false".into())));
    }

    #[test]
    fn cache_ttl_version_order_and_clock_skew() {
        let current = Version::new(1, 1, 1);
        let mut cached = Cached {
            schema_version: 1,
            checked_at: 100,
            latest: Some(Version::new(2, 0, 0)),
        };
        assert!(cached.newer(100, &current).is_some());
        assert!(cached.newer(100, &Version::new(2, 0, 0)).is_none());
        assert!(!cached.fresh(99));
        assert!(!cached.fresh(100 + SUCCESS_TTL));
        cached.latest = None;
        assert!(cached.fresh(100 + FAILURE_TTL - 1));
        assert!(!cached.fresh(100 + FAILURE_TTL));
    }

    #[test]
    fn cached_success_and_failure_throttle_requests() {
        let directory = tempfile::tempdir().unwrap();
        refresh(directory.path(), 100, || Ok(Version::new(2, 0, 0))).unwrap();
        refresh(directory.path(), 101, || {
            panic!("fresh cache must not make a request")
        })
        .unwrap();
        let file = directory.path().join(CACHE_FILE);
        assert_eq!(read(&file).unwrap().latest, Some(Version::new(2, 0, 0)));
        refresh(directory.path(), 100 + SUCCESS_TTL, || {
            Err(Error::Operation("offline".into()))
        })
        .unwrap();
        assert!(read(&file).unwrap().latest.is_none());
        refresh(directory.path(), 101 + SUCCESS_TTL, || {
            panic!("failed checks must back off")
        })
        .unwrap();
        fs::write(&file, b"broken cache").unwrap();
        refresh(directory.path(), 102 + SUCCESS_TTL, || {
            Ok(Version::new(3, 0, 0))
        })
        .unwrap();
        assert_eq!(read(&file).unwrap().latest, Some(Version::new(3, 0, 0)));
    }

    #[test]
    fn concurrent_refresh_skips_a_held_lock() {
        let directory = tempfile::tempdir().unwrap();
        let lock_path = directory.path().join(LOCK_FILE);
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let lock = fs::File::create(&lock_path).unwrap();
        lock.lock().unwrap();
        refresh(directory.path(), 100, || {
            panic!("another worker holds the lock")
        })
        .unwrap();
        assert!(!directory.path().join(CACHE_FILE).exists());
    }
}
