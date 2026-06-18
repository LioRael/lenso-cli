use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use include_dir::{Dir, DirEntry, include_dir};

/// Embedded starter-host template. This is the single source of truth for the
/// project that `lenso host init` writes out.
const TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates/starter-host");

/// Optional prebuilt Runtime Console payload. Release builds populate this
/// directory before packaging `lenso-cli`; development builds may only contain
/// the marker file.
const CONSOLE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/console");

/// Template-wide rewrite values applied when scaffolding a named project.
#[derive(Debug, Clone)]
struct Rewrites {
    package_name: String,
    lib_name: String,
}

/// Scaffold a new Lenso host application into `dir`.
pub fn init(dir: &str, name: Option<&str>, force: bool) -> Result<()> {
    let target = PathBuf::from(dir);
    let default_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("lenso-app");
    let package_name = name.unwrap_or(default_name).to_owned();
    validate_package_name(&package_name)?;

    let lib_name = lib_name_from(&package_name);
    let rewrites = Rewrites {
        package_name: package_name.clone(),
        lib_name,
    };

    prepare_target(&target, force)?;
    extract(&TEMPLATE_DIR, &target, PathBuf::new(), &rewrites)?;
    let console_status = install_embedded_console(&target)?;

    print_next_steps(&target, &package_name, console_status);
    Ok(())
}

/// Refresh hosted Runtime Console assets in an existing Lenso host project.
pub fn update_console(repo_root: Option<&Path>) -> Result<()> {
    let target = repo_root.unwrap_or_else(|| Path::new("."));
    match install_embedded_console(target)? {
        ConsoleInstallStatus::Installed => {
            eprintln!(
                "Updated bundled Runtime Console in {}",
                target.join(".lenso").join("console").display()
            );
            Ok(())
        }
        ConsoleInstallStatus::NotPackaged => bail!(
            "Runtime Console assets were not embedded in this lenso-cli build; install a release build that includes the console"
        ),
    }
}

/// Reject names that cannot be a Cargo package name.
fn validate_package_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => bail!("package name must start with an ASCII letter: {name}"),
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("package name may only contain ASCII letters, digits, '_' and '-': {name}");
    }
    Ok(())
}

/// Convert a package name to its Cargo library crate name (`-` becomes `_`).
fn lib_name_from(package_name: &str) -> String {
    package_name.replace('-', "_")
}

/// Ensure the target directory is empty (or missing) unless `force` is set.
fn prepare_target(target: &Path, force: bool) -> Result<()> {
    if target.exists() {
        let is_empty = target
            .read_dir()
            .with_context(|| format!("read target directory {}", target.display()))?
            .next()
            .is_none();
        if !is_empty && !force {
            bail!(
                "target directory is not empty: {} (pass --force to overwrite)",
                target.display()
            );
        }
    } else {
        fs::create_dir_all(target)
            .with_context(|| format!("create target directory {}", target.display()))?;
    }
    Ok(())
}

/// Recursively copy the embedded template into `target`, applying rewrites.
fn extract(dir: &Dir, target: &Path, rel: PathBuf, rewrites: &Rewrites) -> Result<()> {
    for entry in dir.entries() {
        let name = entry_name(entry)?;
        let entry_rel = rel.join(name);
        let out_path = target.join(&entry_rel);
        match entry {
            DirEntry::Dir(child) => {
                fs::create_dir_all(&out_path)
                    .with_context(|| format!("create directory {}", out_path.display()))?;
                extract(child, target, entry_rel, rewrites)?;
            }
            DirEntry::File(file) => {
                let out_path = output_path(target, &entry_rel);
                write_file(
                    file.contents(),
                    rewrite_for(&entry_rel),
                    &out_path,
                    rewrites,
                )?;
            }
        }
    }
    Ok(())
}

/// Map a template-relative path to its rewrite kind.
///
/// The template manifest is stored as `Cargo.toml.tmpl` so the package does not
/// look like a nested Cargo project; it is written out as `Cargo.toml`.
fn rewrite_for(rel: &Path) -> RewriteKind {
    match rel.to_str() {
        Some("Cargo.toml.tmpl") => RewriteKind::Manifest,
        Some(p) if p.starts_with("src/bin/") && p.ends_with(".rs") => RewriteKind::BinSource,
        _ => RewriteKind::None,
    }
}

#[derive(Debug, Clone, Copy)]
enum RewriteKind {
    None,
    Manifest,
    BinSource,
}

/// Output path for a template file, renaming `Cargo.toml.tmpl` to `Cargo.toml`.
fn output_path(target: &Path, rel: &Path) -> PathBuf {
    let mut out = target.join(rel);
    if out.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml.tmpl") {
        out.set_file_name("Cargo.toml");
    }
    out
}

/// File name for a template entry, regardless of nesting depth.
fn entry_name<'a>(entry: &DirEntry<'a>) -> Result<&'a str> {
    entry
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            anyhow!(
                "template entry without a valid file name: {}",
                entry.path().display()
            )
        })
}

/// Write one template file, rewriting the manifest and bin entrypoints.
fn write_file(contents: &[u8], kind: RewriteKind, out: &Path, rewrites: &Rewrites) -> Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }

    let bytes: Vec<u8> = match kind {
        RewriteKind::Manifest => rewrite_cargo_toml(contents, rewrites)?.into_bytes(),
        RewriteKind::BinSource => rewrite_bin_source(contents, rewrites).into_bytes(),
        RewriteKind::None => contents.to_vec(),
    };

    fs::write(out, bytes).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

fn install_embedded_console(target: &Path) -> Result<ConsoleInstallStatus> {
    let Some(dist_dir) = CONSOLE_DIR.get_dir("dist") else {
        return Ok(ConsoleInstallStatus::NotPackaged);
    };
    if CONSOLE_DIR.get_file("dist/index.html").is_none() {
        return Ok(ConsoleInstallStatus::NotPackaged);
    }

    let console_root = target.join(".lenso").join("console");
    copy_embedded_dir(dist_dir, &console_root.join("dist"))?;

    if let Some(extensions_dir) = CONSOLE_DIR.get_dir("extensions") {
        copy_embedded_dir(extensions_dir, &console_root.join("extensions"))?;
    } else {
        fs::create_dir_all(console_root.join("extensions"))
            .with_context(|| format!("create {}", console_root.join("extensions").display()))?;
    }

    let registry = console_root.join("extensions").join("registry.json");
    if !registry.exists() {
        fs::write(&registry, b"{\"version\":1,\"bundles\":[]}\n")
            .with_context(|| format!("write {}", registry.display()))?;
    }

    Ok(ConsoleInstallStatus::Installed)
}

fn copy_embedded_dir(dir: &Dir, target: &Path) -> Result<()> {
    if target.exists() {
        fs::remove_dir_all(target).with_context(|| format!("remove {}", target.display()))?;
    }
    fs::create_dir_all(target).with_context(|| format!("create {}", target.display()))?;

    for entry in dir.entries() {
        let name = entry_name(entry)?;
        let out_path = target.join(name);
        match entry {
            DirEntry::Dir(child) => copy_embedded_dir(child, &out_path)?,
            DirEntry::File(file) => {
                fs::write(&out_path, file.contents())
                    .with_context(|| format!("write {}", out_path.display()))?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ConsoleInstallStatus {
    Installed,
    NotPackaged,
}

/// Replace the template package name with the requested project name.
fn rewrite_cargo_toml(contents: &[u8], rewrites: &Rewrites) -> Result<String> {
    let text = std::str::from_utf8(contents).context("template Cargo.toml is not UTF-8")?;
    let original = "name = \"lenso-starter-host\"";
    let replacement = format!("name = \"{}\"", rewrites.package_name);
    if !text.contains(original) {
        bail!("template Cargo.toml no longer declares the starter package name");
    }
    Ok(text.replacen(original, &replacement, 1))
}

/// Repoint bin entrypoints from the starter lib crate to the project lib crate.
fn rewrite_bin_source(contents: &[u8], rewrites: &Rewrites) -> String {
    let text = std::str::from_utf8(contents).unwrap_or_default();
    text.replace("lenso_starter_host", &rewrites.lib_name)
}

fn print_next_steps(target: &Path, package_name: &str, console_status: ConsoleInstallStatus) {
    eprintln!(
        "Created Lenso host project `{package_name}` in {}",
        target.display()
    );
    match console_status {
        ConsoleInstallStatus::Installed => {
            eprintln!("Installed the bundled Runtime Console into .lenso/console.");
        }
        ConsoleInstallStatus::NotPackaged => {
            eprintln!("Runtime Console assets were not embedded in this lenso-cli build.");
        }
    }
    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  cd {}", target.display());
    eprintln!("  cp .env.example .env");
    eprintln!("  docker compose up -d postgres");
    eprintln!("  cargo run --bin migrate");
    eprintln!("  cargo run --bin api       # API server");
    eprintln!("  cargo run --bin worker    # in another shell");
    if console_status == ConsoleInstallStatus::Installed {
        eprintln!("  open http://127.0.0.1:3000/console");
    }
    eprintln!();
    eprintln!("Install a remote module with `lenso module install <manifest-url>`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lib_name_replaces_dashes() {
        assert_eq!(lib_name_from("lenso-starter-host"), "lenso_starter_host");
        assert_eq!(lib_name_from("my-app"), "my_app");
        assert_eq!(lib_name_from("app"), "app");
    }

    #[test]
    fn validates_package_names() {
        assert!(validate_package_name("my-app").is_ok());
        assert!(validate_package_name("App2").is_ok());
        assert!(validate_package_name("2app").is_err());
        assert!(validate_package_name("my app").is_err());
        assert!(validate_package_name("-app").is_err());
    }

    #[test]
    fn rewrites_cargo_toml_package_name() {
        let rewrites = Rewrites {
            package_name: "billing-svc".to_owned(),
            lib_name: "billing_svc".to_owned(),
        };
        let input = b"[package]\nname = \"lenso-starter-host\"\nversion = \"0.1.0\"\n";
        let out = rewrite_cargo_toml(input, &rewrites).unwrap();
        assert!(out.contains("name = \"billing-svc\""));
        assert!(!out.contains("lenso-starter-host"));
    }

    #[test]
    fn rewrites_bin_source_lib_reference() {
        let rewrites = Rewrites {
            package_name: "billing-svc".to_owned(),
            lib_name: "billing_svc".to_owned(),
        };
        let input = b"lenso_starter_host::host_composition()";
        let out = rewrite_bin_source(input, &rewrites);
        assert_eq!(out, "billing_svc::host_composition()");
    }

    #[test]
    fn copies_embedded_console_tree() {
        let target = std::env::temp_dir().join(format!(
            "lenso-cli-console-copy-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&target);

        copy_embedded_dir(&CONSOLE_DIR, &target).unwrap();
        assert!(target.join(".keep").exists());

        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn update_console_matches_packaged_asset_state() {
        let target = std::env::temp_dir().join(format!(
            "lenso-cli-console-update-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&target).unwrap();

        let result = update_console(Some(&target));

        if CONSOLE_DIR.get_file("dist/index.html").is_some() {
            result.unwrap();
            assert!(target.join(".lenso/console/dist/index.html").exists());
        } else {
            assert!(result.is_err());
        }
        fs::remove_dir_all(target).unwrap();
    }
}
