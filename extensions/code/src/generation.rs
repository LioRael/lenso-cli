//! Ownership-aware source reconciliation; all mutations are returned for Project review.
use crate::{error, local, question, relative};
use extism_pdk::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedFile {
    #[serde(default)]
    pub scope: lenso_codegen::Scope,
    pub module: String,
    pub owners: BTreeSet<String>,
    pub base: String,
    pub sha256: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub generator: String,
    pub generator_version: String,
    pub files: BTreeMap<String, OwnedFile>,
}
pub struct Changes {
    pub receipt: Receipt,
    pub files: Vec<Value>,
    pub deletions: Vec<String>,
    pub expected: BTreeMap<String, Option<String>>,
    pub summary: Value,
}
pub fn digest(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

struct Edit {
    start: usize,
    end: usize,
    lines: Vec<String>,
}
fn edits(base: &str, other: &str) -> Vec<Edit> {
    let lines: Vec<_> = other.split_inclusive('\n').collect();
    similar::TextDiff::from_lines(base, other)
        .ops()
        .iter()
        .filter(|op| op.tag() != similar::DiffTag::Equal)
        .map(|op| Edit {
            start: op.old_range().start,
            end: op.old_range().end,
            lines: lines[op.new_range()]
                .iter()
                .map(|line| (*line).into())
                .collect(),
        })
        .collect()
}
/// Conservatively merge disjoint line edits. Overlapping edits require a user decision.
fn merge(base: &str, current: &str, generated: &str) -> Option<String> {
    if current == base {
        return Some(generated.into());
    }
    if generated == base || current == generated {
        return Some(current.into());
    }
    let mut left = edits(base, current);
    let right = edits(base, generated);
    for a in &left {
        for b in &right {
            let overlap = if a.start == a.end || b.start == b.end {
                a.start <= b.end && b.start <= a.end
            } else {
                a.start < b.end && b.start < a.end
            };
            if overlap {
                return None;
            }
        }
    }
    left.extend(right);
    left.sort_by_key(|edit| (edit.start, edit.end));
    let base: Vec<_> = base.split_inclusive('\n').collect();
    let mut result = String::new();
    let mut cursor = 0;
    for edit in left {
        result.extend(base[cursor..edit.start].iter().copied());
        result.extend(edit.lines);
        cursor = edit.end;
    }
    result.extend(base[cursor..].iter().copied());
    Some(result)
}

pub fn reconcile(
    ctx: &Value,
    desired: BTreeMap<String, OwnedFile>,
    previous: Option<&Receipt>,
) -> crate::Result<Result<Changes, Json<Value>>> {
    let mut changes = Changes {
        receipt: Receipt {
            schema_version: 1,
            generator: "code/compose-v1".into(),
            generator_version: env!("CARGO_PKG_VERSION").into(),
            files: BTreeMap::new(),
        },
        files: Vec::new(),
        deletions: Vec::new(),
        expected: BTreeMap::new(),
        summary: json!({}),
    };
    if let Some(previous) = previous {
        if previous.schema_version != 1
            || previous.generator != "code/compose-v1"
            || previous.generator_version.is_empty()
            || previous.files.len() > 127
            || previous
                .files
                .values()
                .map(|file| file.base.len())
                .sum::<usize>()
                > 1024 * 1024
        {
            return Err(error("invalid generation receipt"));
        }
        for (path, file) in &previous.files {
            relative(path)?;
            if file.sha256 != digest(&file.base) || file.owners.is_empty() {
                return Err(error("invalid generation base or owners"));
            }
        }
    }
    let paths: BTreeSet<_> = desired
        .keys()
        .chain(
            previous
                .into_iter()
                .flat_map(|receipt| receipt.files.keys()),
        )
        .cloned()
        .collect();
    if paths.len() > 127 {
        return Err(error("generation update exceeds 127 resource paths"));
    }
    let mut kept = 0;
    let mut unchanged = 0;
    let mut merged = 0;
    for path in paths {
        let current = local(&path)?;
        let old = previous.and_then(|receipt| receipt.files.get(&path));
        let next = desired.get(&path);
        let generated = next.map(|file| file.base.as_str());
        let base = old.map(|file| file.base.as_str());
        let mut output = generated.map(str::to_owned);
        let mut conflict = false;
        match (base, current.as_deref(), generated) {
            (_, current, next) if current == next => {}
            (Some(base), Some(current), Some(next)) => {
                if let Some(content) = merge(base, current, next) {
                    merged += usize::from(content != next);
                    output = Some(content);
                } else {
                    conflict = true;
                }
            }
            (Some(base), Some(current), None) if current == base => {}
            (Some(base), None, Some(next)) if base == next => output = None,
            (Some(_), None, Some(_)) => conflict = true,
            (None, None, _) => {}
            _ => conflict = true,
        }
        if conflict {
            let id = format!(
                "generation:{}",
                digest(&serde_json::to_string(
                    &json!({"path":path,"base":base,"current":current,"next":generated,"receipt":previous,"input":ctx["input"]})
                )?)
            );
            match ctx["answers"][&id].as_str() {
                Some("keep") => {
                    output = current.clone();
                    kept += 1;
                }
                Some("overwrite") => {}
                Some(_) => {
                    return Err(error(
                        "generation conflict answer must be keep or overwrite",
                    ));
                }
                None => {
                    let diff = similar::TextDiff::from_lines(
                        current.as_deref().unwrap_or(""),
                        generated.unwrap_or(""),
                    )
                    .unified_diff()
                    .header(
                        &format!("current/{path}"),
                        if generated.is_some() {
                            "generated"
                        } else {
                            "/dev/null"
                        },
                    )
                    .to_string();
                    return Ok(Err(question(
                        &id,
                        &format!(
                            "Generated file conflict: {path}. Keep your current file{} or {}?\n{diff}",
                            if next.is_none() {
                                " and release ownership"
                            } else {
                                ""
                            },
                            if next.is_none() {
                                "delete the obsolete file"
                            } else {
                                "use the newly generated content"
                            }
                        ),
                        json!({"type":"select","options":["keep","overwrite"]}),
                    )));
                }
            }
        }
        if output != current {
            if let Some(content) = &output {
                changes.files.push(json!({"path":path,"content":content}));
            } else if current.is_some() {
                changes.deletions.push(path.clone());
            }
        } else {
            unchanged += 1;
        }
        changes.expected.insert(path.clone(), output);
        if let Some(next) = next {
            changes.receipt.files.insert(path, next.clone());
        }
    }
    changes.summary = json!({"written":changes.files.len(),"deleted":changes.deletions.len(),"kept":kept,"unchanged":unchanged,"merged":merged});
    Ok(Ok(changes))
}

#[cfg(test)]
mod tests {
    use super::merge;
    #[test]
    fn three_way_merge_preserves_disjoint_changes_and_line_endings() {
        assert_eq!(
            merge("a\r\nb\r\nc", "A\r\nb\r\nc", "a\r\nb\r\nC"),
            Some("A\r\nb\r\nC".into())
        );
        assert_eq!(
            merge("a\nb\nc\n", "a\nlocal\nb\nc\n", "a\nb\nC\n"),
            Some("a\nlocal\nb\nC\n".into())
        );
        assert_eq!(merge("a\nb\n", "A\nb\n", "OTHER\nb\n"), None);
        assert_eq!(merge("a", "local", "a"), Some("local".into()));
        assert_eq!(merge("", "local", "remote"), None);
    }
}
