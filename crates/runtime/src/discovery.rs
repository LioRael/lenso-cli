use crate::host::{Host, MAX_RESOURCE_BYTES};
use lenso_cli::Error;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    exclude: Vec<String>,
}

impl Host {
    pub(crate) fn list_files(&self, raw: &str) -> Result<Vec<String>, Error> {
        if !self.discover {
            return Ok(Vec::new());
        }
        let request: Request = serde_json::from_str(raw)?;
        if request.exclude.len() > 32
            || request
                .exclude
                .iter()
                .any(|name| name.is_empty() || name.contains(['/', '\\']))
        {
            return Err(Error::Invalid(
                "directory exclusions must be at most 32 plain names".into(),
            ));
        }
        let mut result = Vec::new();
        let mut directories = 0;
        let mut files = 0;
        for entry in walkdir::WalkDir::new(&self.project)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .filter_entry(|entry| {
                !entry.file_type().is_symlink()
                    && !matches!(entry.file_name().to_str(), Some(".git" | ".lenso"))
                    && !request
                        .exclude
                        .iter()
                        .any(|name| entry.file_name() == name.as_str())
            })
        {
            let entry = entry.map_err(|error| Error::Operation(error.to_string()))?;
            if entry.file_type().is_dir() {
                directories += 1;
            }
            if entry.file_type().is_file() {
                files += 1;
            }
            if directories > 4096 || files > 16384 {
                return Err(Error::Invalid("directory discovery limit exceeded".into()));
            }
            if entry.file_type().is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(&self.project)
                    .map_err(|_| Error::Invalid("directory escaped project".into()))?;
                if crate::path_allowed(relative, &self.reads, &self.read_roots) {
                    result.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        result.sort();
        if serde_json::to_vec(&result)?.len() > MAX_RESOURCE_BYTES {
            return Err(Error::Invalid("directory listing exceeds 1 MiB".into()));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lenso_cli::{Context, serde_json::json};
    #[test]
    fn inventory_is_opt_in_bounded_and_does_not_read_file_contents() {
        let root = tempfile::tempdir().unwrap();
        for directory in ["apps/web", "packages/ignored", "vendor", ".git"] {
            std::fs::create_dir_all(root.path().join(directory)).unwrap();
            std::fs::write(
                root.path().join(directory).join("metadata.txt"),
                "private contents",
            )
            .unwrap();
        }
        let mut ctx = Context {
            events: Default::default(),
            abi_version: 1,
            project: root.path().into(),
            input: json!({}),
            answers: Default::default(),
            steps: Default::default(),
            hooks: Default::default(),
            capabilities: Default::default(),
        };
        assert!(
            Host::new(&ctx)
                .unwrap()
                .list_files("{}")
                .unwrap()
                .is_empty()
        );
        ctx.capabilities.insert("host.discover".into(), json!(true));
        ctx.capabilities.insert(
            "host.read_roots".into(),
            json!(["apps", "packages", "vendor"]),
        );
        let host = Host::new(&ctx).unwrap();
        assert_eq!(
            host.list_files(r#"{"exclude":["ignored","vendor"]}"#)
                .unwrap(),
            ["apps/web/metadata.txt"]
        );
        assert!(host.evidence.reads.is_empty());
        assert!(host.list_files(r#"{"exclude":["../outside"]}"#).is_err());
    }
}
