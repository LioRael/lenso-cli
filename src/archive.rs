use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, bail};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_ARCHIVE_FILES: usize = 4_096;
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) fn archive_bundle(source: &Path, output: &Path) -> anyhow::Result<()> {
    if output.exists() {
        bail!("Plugin Bundle output already exists: {}", output.display());
    }
    let parent = output
        .parent()
        .context("Plugin Bundle output has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = ZipWriter::new(temporary.reopen()?);
    let mut files = Vec::new();
    collect_files(source, source, &mut files)?;
    files.sort();
    for relative in files {
        let source_file = source.join(&relative);
        let metadata = fs::symlink_metadata(&source_file)?;
        if !metadata.file_type().is_file() {
            bail!(
                "Plugin Bundle archives accept regular files only: {}",
                source_file.display()
            );
        }
        let name = portable_path(&relative)?;
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        };
        #[cfg(not(unix))]
        let permissions = 0o644;
        writer.start_file(
            name,
            SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(permissions),
        )?;
        io::copy(&mut File::open(&source_file)?, &mut writer)?;
    }
    writer.finish()?.sync_all()?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("publish Plugin Bundle archive {}", output.display()))?;
    Ok(())
}

pub(crate) fn extract_bundle(archive: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination)?;
    let mut zip = ZipArchive::new(File::open(archive)?)
        .with_context(|| format!("open Plugin Bundle archive {}", archive.display()))?;
    if zip.len() > MAX_ARCHIVE_FILES {
        bail!("Plugin Bundle archive exceeds {MAX_ARCHIVE_FILES} files");
    }
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("Plugin Bundle archive contains an unsafe path")?;
        validate_relative_path(&relative)?;
        if entry.is_dir() {
            fs::create_dir_all(destination.join(relative))?;
            continue;
        }
        if !entry.is_file()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            bail!(
                "Plugin Bundle archive contains a non-file entry: {}",
                entry.name()
            );
        }
        let size = entry.size();
        let mode = entry.unix_mode();
        total = total
            .checked_add(size)
            .context("Plugin Bundle archive size overflow")?;
        if total > MAX_ARCHIVE_BYTES {
            bail!("Plugin Bundle archive exceeds 256 MiB");
        }
        let output = destination.join(relative);
        let parent = output.parent().context("archive entry has no parent")?;
        fs::create_dir_all(parent)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)?;
        io::copy(&mut entry.take(size), &mut file)?;
        file.flush()?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

pub(crate) fn with_bundle_directory<T>(
    bundle: &Path,
    use_directory: impl FnOnce(&Path) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    if bundle.is_dir() {
        return use_directory(bundle);
    }
    let temporary = tempfile::tempdir().context("extract Plugin Bundle archive")?;
    extract_bundle(bundle, temporary.path())?;
    use_directory(temporary.path())
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            bail!(
                "Plugin Bundle cannot archive symlinks: {}",
                entry.path().display()
            );
        }
        if metadata.file_type().is_dir() {
            collect_files(root, &entry.path(), output)?;
        } else if metadata.file_type().is_file() {
            output.push(entry.path().strip_prefix(root)?.to_path_buf());
        } else {
            bail!(
                "Plugin Bundle cannot archive special files: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn portable_path(path: &Path) -> anyhow::Result<String> {
    validate_relative_path(path)?;
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .context("Plugin Bundle path is not UTF-8"),
            _ => unreachable!("validated relative path contains only normal components"),
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn validate_relative_path(path: &Path) -> anyhow::Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "Plugin Bundle archive path is not a safe relative path: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_and_extracts_regular_bundle_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("source")).unwrap();
        fs::write(root.path().join("source/lenso-plugin.json"), "{}").unwrap();
        let archive = root.path().join("fixture.lenso-plugin");
        archive_bundle(&root.path().join("source"), &archive).unwrap();
        let output = root.path().join("output");
        extract_bundle(&archive, &output).unwrap();
        assert_eq!(fs::read(output.join("lenso-plugin.json")).unwrap(), b"{}");
    }
}
