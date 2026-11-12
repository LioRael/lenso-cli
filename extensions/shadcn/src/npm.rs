use crate::{Result, error, request_http};
use node_semver::{Range, Version};
use serde_json::json;

struct Request {
    name: String,
    requirements: Vec<String>,
}

pub fn resolve(name: &str, requirements: &[String]) -> Result<String> {
    let request = Request {
        name: name.to_owned(),
        requirements: requirements.to_vec(),
    };
    if request.name.is_empty()
        || request.name.starts_with('-')
        || request.requirements.is_empty()
        || request.requirements.len() > 32
        || !request
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@/-_.".contains(&b))
        || request.name.contains("..")
    {
        return Err(error("invalid npm dependency request"));
    }
    let base = format!(
        "https://registry.npmjs.org/{}",
        request.name.replace('/', "%2f")
    );
    let mut ranges = Vec::new();
    let mut tagged = Vec::new();
    for value in &request.requirements {
        if let Ok(range) = value.parse::<Range>() {
            ranges.push(range);
        } else if !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            let metadata = metadata(&format!("{base}/{value}"))?;
            if metadata["name"] != request.name {
                return Err(error("npm package identity mismatch"));
            }
            let version: Version = metadata["version"]
                .as_str()
                .ok_or_else(|| error("npm version is missing"))?
                .parse()
                .map_err(|_| error("invalid npm version"))?;
            tagged.push(version);
        } else {
            return Err(error("unsupported npm dependency range or protocol"));
        }
    }
    let satisfies = |version: &Version| {
        ranges.iter().all(|range| version.satisfies(range))
            && tagged.iter().all(|tag| tag == version)
    };
    let latest = if let Some(version) = tagged.first() {
        version.clone()
    } else {
        let metadata = metadata(&format!("{base}/latest"))?;
        if metadata["name"] != request.name {
            return Err(error("npm package identity mismatch"));
        }
        metadata["version"]
            .as_str()
            .ok_or_else(|| error("npm version is missing"))?
            .parse::<Version>()
            .map_err(|_| error("invalid npm version"))?
    };
    let version = if satisfies(&latest) {
        latest
    } else if !tagged.is_empty() {
        return Err(error(format!(
            "conflicting npm requirements for {}",
            request.name
        )));
    } else {
        let metadata = request_http(
            json!({"url":base,"headers":{"Accept":"application/vnd.npm.install-v1+json"},"max_bytes":16*1024*1024,"select":{"name":{"op":"value","pointer":"/name"},"versions":{"op":"keys","pointer":"/versions"}}}),
        )?;
        if metadata["name"] != request.name {
            return Err(error("npm package identity mismatch"));
        }
        metadata["versions"]
            .as_array()
            .ok_or_else(|| error("npm versions are missing"))?
            .iter()
            .filter_map(|version| version.as_str()?.parse::<Version>().ok())
            .filter(satisfies)
            .max()
            .ok_or_else(|| {
                error(format!(
                    "no npm version satisfies the requirements for {}",
                    request.name
                ))
            })?
    };
    Ok(version.to_string())
}

fn metadata(url: &str) -> Result<serde_json::Value> {
    request_http(
        json!({"url":url,"select":{"name":{"op":"value","pointer":"/name"},"version":{"op":"value","pointer":"/version"}}}),
    )
}
