use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use lenso_runtime::{Evidence, ProcessSpec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A complete UTF-8 file replacement requested by an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileChange {
    /// Project-relative destination, without symlinks or traversal.
    pub path: PathBuf,
    /// Complete new file content.
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    path: PathBuf,
    content: Option<String>,
}

/// Frozen changes and exact preimages used to detect stale approvals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema_version: u32,
    project: PathBuf,
    files: Vec<FileChange>,
    snapshots: Vec<Snapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    finalizations: Vec<FileChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deletions: Vec<PathBuf>,
    #[serde(default)]
    evidence: Evidence,
    #[serde(default)]
    commands: Vec<ProcessSpec>,
}

/// Planning or application failure, including partial-application evidence.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    /// A malformed or unsafe file operation.
    #[error("invalid plan: {0}")]
    Invalid(String),
    /// A read or destination changed after planning.
    #[error("project changed after review: {0}")]
    Stale(PathBuf),
    /// Repository provenance changed or became unavailable after planning.
    #[error("repository evidence changed after review")]
    EvidenceChanged,
    /// A file operation failed before application.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Serialization of the approval digest failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Some replacements or deletions completed; no global rollback is claimed.
    #[error(
        "apply failed after changing {completed:?}: {cause}; inspect these files before retrying"
    )]
    Partial {
        /// Files already replaced successfully.
        completed: Vec<PathBuf>,
        /// Failure that interrupted application.
        cause: String,
    },
}

/// Maximum aggregate new content and aggregate preimages in a plan.
const MAX_PLAN_BYTES: usize = 1024 * 1024;

impl Plan {
    /// Snapshot destinations and freeze a bounded set of complete file replacements.
    ///
    /// # Errors
    /// Rejects traversal, symlinks, internal metadata, duplicate paths, and oversized files.
    pub fn build(project: &Path, files: Vec<FileChange>) -> Result<Self, PlanError> {
        Self::build_operations(project, files, Vec::new())
    }

    /// Snapshot replacements and explicit file deletions in one reviewed plan.
    /// # Errors
    /// Rejects unsafe paths, duplicate operations and oversized plans.
    pub fn build_operations(
        project: &Path,
        files: Vec<FileChange>,
        deletions: Vec<PathBuf>,
    ) -> Result<Self, PlanError> {
        let project = project.canonicalize()?;
        let mut plan = Self {
            schema_version: 1,
            project,
            files,
            deletions,
            snapshots: Vec::new(),
            finalizations: Vec::new(),
            evidence: Evidence::default(),
            commands: Vec::new(),
        };
        plan.validate_files()?;
        let mut size = 0;
        for path in plan.paths().map(Path::to_path_buf).collect::<Vec<_>>() {
            let content = read_file(&guarded_path(&plan.project, &path)?)?;
            size += content.as_ref().map_or(0, String::len);
            if size > MAX_PLAN_BYTES {
                return Err(PlanError::Invalid("preimages exceed 1 MiB".into()));
            }
            plan.snapshots.push(Snapshot { path, content });
        }
        Ok(plan)
    }

    /// Add exact second-phase replacements committed only after successful commands.
    /// Each path must already have a prepared replacement in this plan.
    /// # Errors
    /// Rejects undeclared paths, duplicate finalizations and excessive bytes.
    pub fn with_finalizations(mut self, files: Vec<FileChange>) -> Result<Self, PlanError> {
        self.finalizations = files;
        self.validate_files()?;
        Ok(self)
    }

    /// Commit reviewed completion records after commands succeed.
    /// # Errors
    /// Rejects altered prepared outputs, stale completion paths and invalid approval.
    pub fn finalize(&self, approved_id: &str) -> Result<Vec<PathBuf>, PlanError> {
        if self.id()? != approved_id {
            return Err(PlanError::Invalid(
                "approval digest does not match this plan".into(),
            ));
        }
        self.validate_files()?;
        if self.finalizations.is_empty() {
            return Ok(Vec::new());
        }
        for file in &self.files {
            if read_file(&guarded_path(&self.project, &file.path)?)?.as_deref()
                != Some(&file.content)
            {
                return Err(PlanError::Stale(file.path.clone()));
            }
        }
        for path in &self.deletions {
            if read_file(&guarded_path(&self.project, path)?)?.is_some() {
                return Err(PlanError::Stale(path.clone()));
            }
        }
        let prepared: Vec<_> = self
            .finalizations
            .iter()
            .map(|file| {
                let parent = guarded_path(&self.project, &file.path)?
                    .parent()
                    .ok_or_else(|| PlanError::Invalid("completion path has no parent".into()))?
                    .to_owned();
                let mut temp = tempfile::NamedTempFile::new_in(parent)?;
                let target = guarded_path(&self.project, &file.path)?;
                temp.write_all(file.content.as_bytes())?;
                temp.as_file()
                    .set_permissions(fs::metadata(target)?.permissions())?;
                temp.as_file().sync_all()?;
                Ok::<_, PlanError>(temp)
            })
            .collect::<Result<_, _>>()?;
        let mut completed = Vec::new();
        for (file, temp) in self.finalizations.iter().zip(prepared) {
            let result = (|| {
                let target = guarded_path(&self.project, &file.path)?;
                let expected = self
                    .files
                    .iter()
                    .find(|prepared| prepared.path == file.path)
                    .ok_or_else(|| PlanError::Invalid("missing prepared replacement".into()))?;
                if read_file(&target)?.as_deref() != Some(&expected.content) {
                    return Err(PlanError::Stale(file.path.clone()));
                }
                temp.persist(target).map_err(|error| error.error)?;
                Ok::<_, PlanError>(())
            })();
            if let Err(error) = result {
                return Err(PlanError::Partial {
                    completed,
                    cause: error.to_string(),
                });
            }
            completed.push(file.path.clone());
        }
        Ok(completed)
    }

    /// Bind host-recorded source digests and project reads into this approval.
    ///
    /// # Errors
    /// Rejects project inputs that already changed since the extension read them.
    pub fn with_evidence(mut self, evidence: Evidence) -> Result<Self, PlanError> {
        self.evidence = evidence;
        self.check_reads()?;
        Ok(self)
    }

    /// Include exact external commands in the reviewed and hashed plan.
    #[must_use]
    pub fn with_commands(mut self, commands: Vec<ProcessSpec>) -> Self {
        self.commands = commands;
        self
    }

    /// Commands requested after successful file application.
    #[must_use]
    pub fn commands(&self) -> &[ProcessSpec] {
        &self.commands
    }

    /// File reads whose digests this plan checks before application.
    pub fn reads(&self) -> impl Iterator<Item = &Path> {
        self.evidence.reads.keys().map(PathBuf::as_path)
    }

    /// Canonical project root this plan can modify.
    #[must_use]
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// Iterate the exact project-relative destinations included in this plan.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files
            .iter()
            .map(|file| file.path.as_path())
            .chain(self.deletions.iter().map(PathBuf::as_path))
    }

    /// Compute the consent identifier over paths, preimages, content, and project root.
    ///
    /// # Errors
    /// Returns a serialization error if the plan cannot be encoded.
    pub fn id(&self) -> Result<String, PlanError> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    /// Render every proposed change as a unified diff without modifying the project.
    #[must_use]
    pub fn preview(&self) -> String {
        let mut preview = String::new();
        for file in &self.files {
            let old = self
                .snapshots
                .iter()
                .find(|s| s.path == file.path)
                .and_then(|s| s.content.as_deref())
                .unwrap_or("");
            preview.push_str(
                &similar::TextDiff::from_lines(old, &file.content)
                    .unified_diff()
                    .header(
                        &format!("a/{}", file.path.display()),
                        &format!("b/{}", file.path.display()),
                    )
                    .to_string(),
            );
        }
        for path in &self.deletions {
            let old = self
                .snapshots
                .iter()
                .find(|s| &s.path == path)
                .and_then(|s| s.content.as_deref())
                .unwrap_or("");
            preview.push_str(&format!("\nDelete file: {}\n", path.display()));
            preview.push_str(
                &similar::TextDiff::from_lines(old, "")
                    .unified_diff()
                    .header(&format!("a/{}", path.display()), "/dev/null")
                    .to_string(),
            );
        }
        for file in &self.finalizations {
            let prepared = self
                .files
                .iter()
                .find(|prepared| prepared.path == file.path)
                .map_or("", |prepared| prepared.content.as_str());
            preview.push_str(&format!(
                "\nAfter successful commands: {}\n",
                file.path.display()
            ));
            preview.push_str(
                &similar::TextDiff::from_lines(prepared, &file.content)
                    .unified_diff()
                    .header("prepared", "completed")
                    .to_string(),
            );
        }
        if !self.commands.is_empty() {
            preview.push_str("\nExternal commands (run after file changes):\n");
            for command in &self.commands {
                preview.push_str(&format!("  {} {:?}\n", command.program, command.args));
            }
            preview.push_str("Commands can access the network and change lockfiles, installed packages, and caches. They are not a sandboxed or globally reversible transaction.\n");
        }
        preview
    }

    /// Recheck all preimages, stage replacements, and apply an explicitly approved digest.
    ///
    /// File replacements are individually atomic. A failure can leave newly created
    /// directories or previously completed files; the error records completed paths.
    /// This is optimistic concurrency protection, not a hostile-filesystem sandbox.
    ///
    /// # Errors
    /// Rejects changed plans, stale files, unsafe paths, and reports partial I/O failures.
    pub fn apply(&self, approved_id: &str) -> Result<Vec<PathBuf>, PlanError> {
        if self.id()? != approved_id {
            return Err(PlanError::Invalid(
                "approval digest does not match this plan".into(),
            ));
        }
        self.validate_files()?;
        self.check_reads()?;
        if self.snapshots.len() != self.files.len() + self.deletions.len()
            || self
                .snapshots
                .iter()
                .zip(self.paths())
                .any(|(s, path)| s.path != path)
        {
            return Err(PlanError::Invalid(
                "snapshot paths do not match changes".into(),
            ));
        }
        self.check_snapshots()?;
        let mut staged = Vec::new();
        for file in &self.files {
            let target = guarded_path(&self.project, &file.path)?;
            let parent = target
                .parent()
                .ok_or_else(|| PlanError::Invalid("destination has no parent".into()))?;
            fs::create_dir_all(parent)?;
            guarded_path(&self.project, &file.path)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(file.content.as_bytes())?;
            if let Ok(metadata) = fs::metadata(&target) {
                temp.as_file().set_permissions(metadata.permissions())?;
            }
            temp.as_file().sync_all()?;
            staged.push((target, temp));
        }
        self.check_snapshots()?;
        let mut completed = Vec::new();
        for path in &self.deletions {
            let result = (|| {
                let target = guarded_path(&self.project, path)?;
                let snapshot = self
                    .snapshots
                    .iter()
                    .find(|s| &s.path == path)
                    .ok_or_else(|| PlanError::Invalid("missing deletion preimage".into()))?;
                if read_file(&target)? != snapshot.content {
                    return Err(PlanError::Stale(path.clone()));
                }
                if snapshot.content.is_some() {
                    fs::remove_file(target)?;
                }
                Ok::<_, PlanError>(())
            })();
            if let Err(error) = result {
                return Err(PlanError::Partial {
                    completed,
                    cause: error.to_string(),
                });
            }
            completed.push(path.clone());
        }
        for ((target, temp), snapshot) in staged.into_iter().zip(&self.snapshots) {
            let result = (|| {
                guarded_path(&self.project, &snapshot.path)?;
                if read_file(&target)? != snapshot.content {
                    return Err(PlanError::Stale(snapshot.path.clone()));
                }
                if snapshot.content.is_none() {
                    temp.persist_noclobber(target).map_err(|e| e.error)?;
                } else {
                    temp.persist(target).map_err(|e| e.error)?;
                }
                Ok::<_, PlanError>(())
            })();
            if let Err(error) = result {
                return Err(PlanError::Partial {
                    completed,
                    cause: error.to_string(),
                });
            }
            completed.push(snapshot.path.clone());
        }
        Ok(completed)
    }

    fn validate_files(&self) -> Result<(), PlanError> {
        if self.schema_version != 1
            || !self.project.is_absolute()
            || self.project.canonicalize()? != self.project
            || !self.project.is_dir()
        {
            return Err(PlanError::Invalid(
                "unsupported schema or noncanonical project root".into(),
            ));
        }
        if self.files.len() + self.deletions.len() > 128
            || self.files.iter().map(|f| f.content.len()).sum::<usize>() > MAX_PLAN_BYTES
        {
            return Err(PlanError::Invalid(
                "plans allow at most 128 files and 1 MiB of content".into(),
            ));
        }
        let mut finalized = BTreeSet::new();
        if self.finalizations.len() > 128
            || self
                .files
                .iter()
                .chain(&self.finalizations)
                .map(|file| file.content.len())
                .sum::<usize>()
                > MAX_PLAN_BYTES
        {
            return Err(PlanError::Invalid(
                "prepared and completion content exceed plan limits".into(),
            ));
        }
        for file in &self.finalizations {
            if !self.files.iter().any(|prepared| prepared.path == file.path)
                || !finalized.insert(&file.path)
            {
                return Err(PlanError::Invalid(
                    "completion paths must be unique prepared replacements".into(),
                ));
            }
        }
        let mut paths = BTreeSet::new();
        for path in self.paths() {
            guarded_path(&self.project, path)?;
            if self
                .paths()
                .any(|other| path != other && path.starts_with(other))
            {
                return Err(PlanError::Invalid(
                    "file destinations cannot contain one another".into(),
                ));
            }
            if !paths.insert(path) {
                return Err(PlanError::Invalid(format!(
                    "duplicate path: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    fn check_snapshots(&self) -> Result<(), PlanError> {
        for snapshot in &self.snapshots {
            if read_file(&guarded_path(&self.project, &snapshot.path)?)? != snapshot.content {
                return Err(PlanError::Stale(snapshot.path.clone()));
            }
        }
        Ok(())
    }

    fn check_reads(&self) -> Result<(), PlanError> {
        lenso_runtime::verify_resource_pins(&self.project, &self.evidence.resources)
            .map_err(|_| PlanError::EvidenceChanged)?;
        for (path, expected) in &self.evidence.reads {
            let absolute = lenso_runtime::project_path(&self.project, path)
                .map_err(|e| PlanError::Invalid(e.to_string()))?;
            let actual = read_file(&absolute)?
                .map(|content| format!("{:x}", Sha256::digest(content.as_bytes())));
            if &actual != expected {
                return Err(PlanError::Stale(path.clone()));
            }
        }
        Ok(())
    }
}

fn guarded_path(project: &Path, relative: &Path) -> Result<PathBuf, PlanError> {
    if relative.as_os_str().is_empty() {
        return Err(PlanError::Invalid("empty path".into()));
    }
    let mut path = project.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(PlanError::Invalid(format!(
                "path must be relative without traversal: {}",
                relative.display()
            )));
        };
        if name == ".git" || name == ".lenso" {
            return Err(PlanError::Invalid(
                "internal metadata cannot be a plan destination".into(),
            ));
        }
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(PlanError::Invalid(format!(
                    "symlink in destination: {}",
                    relative.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

fn read_file(path: &Path) -> Result<Option<String>, PlanError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_PLAN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PLAN_BYTES {
        return Err(PlanError::Invalid("existing file exceeds 1 MiB".into()));
    }
    Ok(Some(String::from_utf8(bytes).map_err(|_| {
        PlanError::Invalid(format!("non-UTF-8 file: {}", path.display()))
    })?))
}
