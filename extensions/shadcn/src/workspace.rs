use crate::{Result, error, inventory, read};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct PnpmWorkspace {
    #[serde(default)]
    packages: Vec<String>,
}

pub fn directories() -> Result<Vec<String>> {
    let paths = inventory()?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let root: serde_json::Value = serde_json::from_str(
        &read("package.json")?.ok_or_else(|| error("workspace discovery requires package.json"))?,
    )?;
    let mut patterns: Vec<String> = root
        .get("workspaces")
        .map(|value| value.get("packages").unwrap_or(value))
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    if let Some(text) = read("pnpm-workspace.yaml")? {
        let pnpm: PnpmWorkspace = serde_yaml_ng::from_str(&text)
            .map_err(|failure| error(format!("pnpm-workspace.yaml: {failure}")))?;
        patterns.extend(pnpm.packages);
    }
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let mut include = globset::GlobSetBuilder::new();
    let mut exclude = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let (negative, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, pattern.as_str()), |value| (true, value));
        let pattern = pattern.trim_start_matches("./");
        if pattern.starts_with('/')
            || pattern.split('/').any(|part| {
                part == ".." || part == ".git" || part == "node_modules" || part == ".lenso"
            })
        {
            return Err(error(
                "workspace pattern escapes project or accesses reserved directories",
            ));
        }
        let glob = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|failure| error(format!("workspace pattern: {failure}")))?;
        if negative {
            exclude.add(glob);
        } else {
            include.add(glob);
        }
    }
    let include = include
        .build()
        .map_err(|failure| error(failure.to_string()))?;
    let exclude = exclude
        .build()
        .map_err(|failure| error(failure.to_string()))?;
    let mut result = Vec::new();
    for path in paths {
        let Some(directory) = path.strip_suffix("/package.json") else {
            continue;
        };
        if include.is_match(directory) && !exclude.is_match(directory) {
            result.push(directory.to_owned());
            if result.len() > 128 {
                return Err(error("workspace package limit exceeded"));
            }
        }
    }
    result.sort();
    Ok(result)
}
