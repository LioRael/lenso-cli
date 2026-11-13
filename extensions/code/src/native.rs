//! Native manifest and ownership reconciliation. The Host only reviews files and exact commands.
use crate::generation::digest;
use crate::{error, local, question};
use extism_pdk::Json;
use lenso_codegen::Dependency;
use lenso_composition::Resolution;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const RECEIPT: &str = "lenso/native.lock";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub manifest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<String>,
    #[serde(default)]
    pub offline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    value: Value,
    owners: BTreeSet<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    applied: Option<Value>,
    requested: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestState {
    ecosystem: String,
    target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    installed: Option<String>,
    entries: BTreeMap<String, Entry>,
    requests: BTreeMap<String, BTreeMap<String, Request>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    targets: BTreeMap<String, ManifestState>,
}
#[derive(Default)]
pub struct Plan {
    pub files: Vec<Value>,
    pub finalizations: Vec<Value>,
    pub commands: Vec<Value>,
    pub summary: Value,
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn join(root: &str, name: &str) -> String {
    if root.is_empty() {
        name.into()
    } else {
        format!("{root}/{name}")
    }
}
impl Target {
    fn validate(&self, ecosystem: &str) -> crate::Result<()> {
        lenso_codegen::relative(&self.manifest)?;
        let name = match ecosystem {
            "cargo" => "Cargo.toml",
            "npm" => "package.json",
            _ => {
                return Err(error(format!(
                    "unsupported native ecosystem {ecosystem}; supported: cargo, npm"
                )));
            }
        };
        if self.manifest.rsplit('/').next() != Some(name) {
            return Err(error(format!("{ecosystem} target must name {name}")));
        }
        lenso_codegen::relative(&self.lock_path(ecosystem))?;
        if ecosystem == "npm"
            && self.lock_path(ecosystem) != join(parent(&self.manifest), "package-lock.json")
        {
            return Err(error(
                "npm requires package-lock.json beside the configured manifest",
            ));
        }

        if self.lock_path(ecosystem).rsplit('/').next()
            != Some(if ecosystem == "cargo" {
                "Cargo.lock"
            } else {
                "package-lock.json"
            })
        {
            return Err(error("invalid native lock path"));
        }
        if self
            .program
            .as_ref()
            .is_some_and(|program| program.trim().is_empty() || program.contains('\0'))
        {
            return Err(error("invalid native program"));
        }
        Ok(())
    }
    fn lock_path(&self, ecosystem: &str) -> String {
        self.lock.clone().unwrap_or_else(|| {
            join(
                parent(&self.manifest),
                if ecosystem == "cargo" {
                    "Cargo.lock"
                } else {
                    "package-lock.json"
                },
            )
        })
    }
    fn command(&self, ecosystem: &str, offline: bool) -> Value {
        let mut args: Vec<String> = if ecosystem == "cargo" {
            vec![
                "fetch".into(),
                "--manifest-path".into(),
                self.manifest.clone(),
            ]
        } else {
            vec![
                "install".into(),
                "--prefix".into(),
                if parent(&self.manifest).is_empty() {
                    ".".into()
                } else {
                    parent(&self.manifest).into()
                },
                "--ignore-scripts".into(),
                "--no-audit".into(),
                "--no-fund".into(),
                "--package-lock=true".into(),
            ]
        };
        if offline || self.offline {
            args.push("--offline".into());
        }
        json!({"program":self.program.as_deref().unwrap_or(ecosystem),"args":args})
    }
}
/// Explicit setup only; manifests and process grants are separately reviewable from source grants.
pub fn configure(value: &Value) -> crate::Result<(Value, Vec<String>, Vec<Value>)> {
    let targets: BTreeMap<String, Target> = serde_json::from_value(value.clone())?;
    let mut paths = BTreeSet::from([RECEIPT.to_owned()]);
    let mut commands = Vec::new();
    let mut manifests = BTreeSet::new();
    for (ecosystem, target) in &targets {
        target.validate(ecosystem)?;
        if !manifests.insert(&target.manifest) {
            return Err(error("native targets must use distinct manifests"));
        }
        paths.insert(target.manifest.clone());
        paths.insert(target.lock_path(ecosystem));
        for offline in [false, true] {
            let command = target.command(ecosystem, offline);
            if !commands.contains(&command) {
                commands.push(command);
            }
        }
    }
    Ok((
        serde_json::to_value(targets)?,
        paths.into_iter().collect(),
        commands,
    ))
}
fn identity(dependency: &Dependency) -> crate::Result<(String, Value)> {
    if dependency.package.is_empty()
        || dependency.version.trim().is_empty()
        || dependency.package.starts_with('-')
        || !dependency
            .package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@/_.-".contains(&byte))
    {
        return Err(error("invalid native package identity or version"));
    }
    let section = dependency
        .options
        .get("section")
        .and_then(Value::as_str)
        .unwrap_or("dependencies");
    let value = match dependency.ecosystem.as_str() {
        "cargo" => {
            if !["dependencies", "dev-dependencies", "build-dependencies"].contains(&section)
                || dependency.package.contains(['@', '/'])
            {
                return Err(error(
                    "unsupported Cargo dependency section or package name",
                ));
            }
            let mut fields = serde_json::Map::new();
            fields.insert("version".into(), json!(dependency.version));
            for (key, value) in &dependency.options {
                match key.as_str() {
                    "section" if value.is_string() => {}
                    "features"
                        if value.as_array().is_some_and(|features| {
                            features.iter().all(|feature| {
                                feature.as_str().is_some_and(|feature| !feature.is_empty())
                            })
                        }) =>
                    {
                        fields.insert(key.clone(), value.clone());
                    }
                    "default-features" | "optional" if value.is_boolean() => {
                        fields.insert(key.clone(), value.clone());
                    }
                    "path" | "package" | "registry"
                        if value
                            .as_str()
                            .is_some_and(|value| !value.is_empty() && !value.contains('\0')) =>
                    {
                        fields.insert(key.clone(), value.clone());
                    }
                    _ => return Err(error(format!("unsupported Cargo option {key}"))),
                }
            }
            if fields.len() == 1 {
                json!(dependency.version)
            } else {
                Value::Object(fields)
            }
        }
        "npm" => {
            if ![
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ]
            .contains(&section)
                || dependency.options.keys().any(|key| key != "section")
                || dependency
                    .options
                    .get("section")
                    .is_some_and(|value| !value.is_string())
            {
                return Err(error("unsupported npm dependency options"));
            }
            if dependency.package.starts_with('@') && dependency.package.split('/').count() != 2
                || !dependency.package.starts_with('@') && dependency.package.contains('/')
            {
                return Err(error("invalid npm package name"));
            }
            json!(dependency.version)
        }
        ecosystem => return Err(error(format!("unsupported native ecosystem {ecosystem}"))),
    };
    Ok((format!("{section}:{}", dependency.package), value))
}
fn combine(left: &Value, right: &Value) -> crate::Result<Value> {
    if left == right {
        return Ok(left.clone());
    }
    let fields = |value: &Value| -> Option<serde_json::Map<String, Value>> {
        value.as_object().cloned().or_else(|| {
            value
                .as_str()
                .map(|version| serde_json::Map::from_iter([("version".into(), json!(version))]))
        })
    };
    if let (Some(mut left), Some(mut right)) = (fields(left), fields(right)) {
        let features = |map: &mut serde_json::Map<String, Value>| -> BTreeSet<String> {
            map.remove("features")
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        };
        let mut union = features(&mut left);
        union.extend(features(&mut right));
        if left == right {
            if !union.is_empty() {
                left.insert("features".into(), json!(union));
            }
            return Ok(Value::Object(left));
        }
    }
    Err(error(
        "selected native dependency requests conflict; align versions/source/options before generating",
    ))
}
fn requests(
    dependencies: &[Dependency],
    resolution: &Resolution,
) -> crate::Result<BTreeMap<String, BTreeMap<String, Request>>> {
    let mut output: BTreeMap<String, BTreeMap<String, Request>> = BTreeMap::new();
    for dependency in dependencies {
        let (key, value) = identity(dependency)?;
        let owners: BTreeSet<_> = resolution
            .instances
            .iter()
            .filter(|(_, instance)| {
                dependency
                    .module
                    .as_ref()
                    .is_none_or(|module| module == &instance.module)
            })
            .map(|(id, _)| id.clone())
            .collect();
        if owners.is_empty() {
            return Err(error("native dependency has no active owner"));
        }
        let packages = output.entry(dependency.ecosystem.clone()).or_default();
        if let Some(previous) = packages.get_mut(&key) {
            previous.value = combine(&previous.value, &value).map_err(|failure| {
                error(format!(
                    "conflicting {} requests for {key}: {failure}",
                    dependency.ecosystem
                ))
            })?;
            previous.owners.extend(owners);
        } else {
            packages.insert(key, Request { value, owners });
        }
    }
    Ok(output)
}
#[cfg_attr(test, expect(dead_code, reason = "Used only by Wasm entry points"))]
pub fn validate(dependencies: &[Dependency], resolution: &Resolution) -> crate::Result<()> {
    requests(dependencies, resolution).map(|_| ())
}

// Reject ambiguous manifests before rewriting any dependency section.
struct UniqueJson(Value);
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(json!(value)))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(json!(value)))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(json!(value)))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|value| UniqueJson(Value::Number(value)))
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(json!(value)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueJson(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, UniqueJson(value))) = map.next_entry::<String, UniqueJson>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON key {key}"
                        )));
                    }
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

struct Manifest {
    ecosystem: String,
    value: Value,
    cargo: Option<toml_edit::DocumentMut>,
    original: String,
}
impl Manifest {
    fn parse(ecosystem: &str, content: String) -> crate::Result<Self> {
        let (value, cargo) = if ecosystem == "cargo" {
            let document = content.parse::<toml_edit::DocumentMut>()?;
            (
                serde_json::to_value(toml::from_str::<toml::Table>(&content)?)?,
                Some(document),
            )
        } else {
            (serde_json::from_str::<UniqueJson>(&content)?.0, None)
        };
        if !value.is_object() {
            return Err(error("native manifest must be an object"));
        }
        Ok(Self {
            ecosystem: ecosystem.into(),
            value,
            cargo,
            original: content,
        })
    }
    fn entry(&self, key: &str) -> crate::Result<Option<Value>> {
        let (section, package) = key
            .split_once(':')
            .ok_or_else(|| error("invalid native receipt entry key"))?;
        let allowed = if self.ecosystem == "cargo" {
            &["dependencies", "dev-dependencies", "build-dependencies"][..]
        } else {
            &[
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ][..]
        };
        if !allowed.contains(&section) || package.is_empty() {
            return Err(error("invalid managed dependency section"));
        }
        match self.value.get(section) {
            None => Ok(None),
            Some(Value::Object(table)) => {
                let value = table.get(package);
                if value.is_some_and(|value| {
                    !value.is_string() && !(self.ecosystem == "cargo" && value.is_object())
                }) {
                    return Err(error("invalid native dependency entry value"));
                }
                Ok(value.cloned())
            }
            _ => Err(error(format!("native section {section} must be a table"))),
        }
    }
    fn set(&mut self, key: &str, value: Option<Value>) -> crate::Result<()> {
        let (section, package) = key
            .split_once(':')
            .ok_or_else(|| error("invalid entry key"))?;
        let table = self
            .value
            .as_object_mut()
            .ok_or_else(|| error("invalid manifest"))?;
        if value.is_some() && !table.contains_key(section) {
            table.insert(section.into(), json!({}));
        }
        if let Some(table) = table.get_mut(section).and_then(Value::as_object_mut) {
            if let Some(value) = &value {
                table.insert(package.into(), value.clone());
            } else {
                table.remove(package);
            }
        }
        if let Some(document) = &mut self.cargo {
            if value.is_some() && document.get(section).is_none() {
                document[section] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            if let Some(table) = document
                .get_mut(section)
                .and_then(toml_edit::Item::as_table_like_mut)
            {
                if let Some(value) = value {
                    let mut encoded = toml_edit::ser::to_document(&json!({"entry":value}))?;
                    let mut item = encoded
                        .as_table_mut()
                        .remove("entry")
                        .ok_or_else(|| error("invalid dependency encoding"))?;
                    if item.is_table() {
                        item = toml_edit::Item::Value(toml_edit::Value::InlineTable(
                            item.into_table()
                                .map_err(|_| error("invalid dependency table"))?
                                .into_inline_table(),
                        ));
                    }
                    if let (Some(old), Some(new)) = (
                        table.get(package).and_then(toml_edit::Item::as_value),
                        item.as_value_mut(),
                    ) {
                        *new.decor_mut() = old.decor().clone();
                    }
                    table.insert(package, item);
                } else {
                    table.remove(package);
                }
            }
        }
        Ok(())
    }
    fn content(&self, changed: bool) -> crate::Result<String> {
        if !changed {
            return Ok(self.original.clone());
        }
        Ok(if let Some(document) = &self.cargo {
            document.to_string()
        } else {
            format!("{}\n", serde_json::to_string_pretty(&self.value)?)
        })
    }
}

pub fn plan(
    ctx: &Value,
    dependencies: &[Dependency],
    resolution: &Resolution,
    group: &str,
) -> crate::Result<Result<Plan, Json<Value>>> {
    let configuration = &ctx["capabilities"]["extension.options"]["native"];
    if configuration.is_null() {
        if dependencies.is_empty() {
            return Ok(Ok(Plan::default()));
        }
        return Err(error(
            "native dependencies require explicit Code setup: input.native with cargo/npm manifest targets",
        ));
    }
    let settings: BTreeMap<String, Target> = serde_json::from_value(configuration.clone())?;
    configure(configuration)?;
    let selected = requests(dependencies, resolution)?;
    for ecosystem in selected.keys() {
        if !settings.contains_key(ecosystem) {
            return Err(error(format!(
                "configure the {ecosystem} native target before composing"
            )));
        }
    }
    let original = local(RECEIPT)?;
    let mut state: State = original
        .as_ref()
        .map(|content| toml::from_str(content))
        .transpose()?
        .unwrap_or(State {
            schema_version: 1,
            targets: BTreeMap::new(),
        });
    if state.schema_version != 1 || state.targets.len() > 32 {
        return Err(error("invalid native ownership receipt"));
    }
    let mut involved: BTreeSet<String> = state
        .targets
        .iter()
        .filter(|(_, state)| state.requests.contains_key(group))
        .map(|(manifest, _)| manifest.clone())
        .collect();
    involved.extend(
        selected
            .keys()
            .map(|ecosystem| settings[ecosystem].manifest.clone()),
    );
    if involved.is_empty() {
        return Ok(Ok(Plan::default()));
    }
    let mut result = Plan::default();
    let mut changed_entries = 0;
    let mut kept = 0;
    let mut completions = BTreeMap::new();
    for manifest_path in involved {
        let ecosystem = selected
            .keys()
            .find(|ecosystem| settings[*ecosystem].manifest == manifest_path)
            .cloned()
            .or_else(|| {
                state
                    .targets
                    .get(&manifest_path)
                    .map(|state| state.ecosystem.clone())
            })
            .ok_or_else(|| error("native target identity missing"))?;
        let target = settings.get(&ecosystem).ok_or_else(|| {
            error("retire native dependencies before removing their target configuration")
        })?;
        if target.manifest != manifest_path {
            return Err(error(
                "retire native dependencies before moving a native manifest target",
            ));
        }
        let current = local(&manifest_path)?.ok_or_else(|| {
            error(format!(
                "native manifest {manifest_path} must already exist"
            ))
        })?;
        let lock = local(&target.lock_path(&ecosystem))?;
        let mut manifest = Manifest::parse(&ecosystem, current.clone())?;
        let managed = state
            .targets
            .entry(manifest_path.clone())
            .or_insert(ManifestState {
                ecosystem: ecosystem.clone(),
                target: target.clone(),
                installed: None,
                entries: BTreeMap::new(),
                requests: BTreeMap::new(),
            });
        if managed.ecosystem != ecosystem
            || managed.target.manifest != manifest_path
            || managed.requests.len() > 64
            || managed.entries.len() > 256
        {
            return Err(error(
                "invalid or oversized native manifest ownership state",
            ));
        }
        managed.target = target.clone();
        // Keep a zero-owner retry marker until retirement commands have succeeded.
        managed.requests.insert(
            group.into(),
            selected.get(&ecosystem).cloned().unwrap_or_default(),
        );
        let mut desired: BTreeMap<String, Request> = BTreeMap::new();
        for group in managed.requests.values() {
            for (key, request) in group {
                if request.owners.is_empty() {
                    return Err(error("native receipt request has no owner"));
                }
                if let Some(old) = desired.get_mut(key) {
                    old.value = combine(&old.value, &request.value).map_err(|failure|error(format!("conflicting composition requests for {manifest_path} [{key}]: {failure}")))?;
                    old.owners.extend(request.owners.clone());
                } else {
                    desired.insert(key.clone(), request.clone());
                }
            }
        }
        let keys: BTreeSet<_> = managed
            .entries
            .keys()
            .chain(desired.keys())
            .cloned()
            .collect();
        let mut changed = false;
        for key in keys {
            let existing = manifest.entry(&key)?;
            let previous = managed.entries.get(&key);
            let requested = desired.get(&key).map(|request| request.value.clone());
            let mut next = requested
                .clone()
                .or_else(|| previous.and_then(|entry| entry.original.clone()));
            let unchanged_choice = previous.is_some_and(|entry| {
                existing == entry.applied
                    && requested.as_ref() == Some(&entry.requested)
                    && entry.applied.as_ref() != Some(&entry.requested)
            });
            if unchanged_choice {
                next = existing.clone();
            }
            let safe = existing == next
                || unchanged_choice
                || previous.is_some_and(|entry| {
                    existing == entry.applied && entry.applied.as_ref() == Some(&entry.requested)
                })
                || previous.is_none() && existing.is_none();
            if !safe {
                let id = format!(
                    "native:{}",
                    digest(&serde_json::to_string(
                        &json!({"group":group,"manifest":manifest_path,"key":key,"current":existing,"next":next,"state":managed})
                    )?)
                );
                match ctx["answers"][&id].as_str() {
                    Some("keep") => {
                        next = existing.clone();
                        kept += 1;
                    }
                    Some("replace") => {}
                    Some(_) => return Err(error("native conflict answer must be keep or replace")),
                    None => {
                        return Ok(Err(question(
                            &id,
                            &format!(
                                "Native dependency conflict: {manifest_path} [{key}]\nCurrent: {}\nProposed: {}\nKeep the current entry or replace it with the proposal?",
                                serde_json::to_string(&existing)?,
                                serde_json::to_string(&next)?
                            ),
                            json!({"type":"select","options":["keep","replace"]}),
                        )));
                    }
                }
            }
            if existing != next {
                manifest.set(&key, next.clone())?;
                changed = true;
                changed_entries += 1;
            }
            if let Some(requested) = requested {
                let original =
                    previous.map_or_else(|| existing.clone(), |entry| entry.original.clone());
                managed.entries.insert(
                    key,
                    Entry {
                        original,
                        applied: next,
                        requested,
                    },
                );
            } else {
                managed.entries.remove(&key);
            }
        }
        let content = manifest.content(changed)?;
        if content != current {
            result
                .files
                .push(json!({"path":manifest_path,"content":content}));
        }
        let fingerprint = digest(&serde_json::to_string(
            &json!({"manifest":manifest_path,"value":manifest.value,"program":target.program,"ecosystem":ecosystem}),
        )?);
        if managed.installed.as_ref() != Some(&fingerprint)
            || lock.is_none()
            || ctx["input"]["install"].as_bool() == Some(true)
        {
            if !result
                .files
                .iter()
                .any(|file| file["path"] == manifest_path)
            {
                result
                    .files
                    .push(json!({"path":manifest_path,"content":content}));
            }
            result.commands.push(
                target.command(
                    &ecosystem,
                    ctx["capabilities"]["host.offline"]
                        .as_bool()
                        .unwrap_or(false),
                ),
            );
            managed.installed = None;
            completions.insert(manifest_path, fingerprint);
        } else if managed.requests.get(group).is_some_and(BTreeMap::is_empty) {
            managed.requests.remove(group);
        }
    }
    let prepared = format!("{}\n", toml::to_string_pretty(&state)?);
    if original.as_deref() != Some(&prepared) {
        result
            .files
            .insert(0, json!({"path":RECEIPT,"content":prepared}));
    }
    if !completions.is_empty() {
        if !result.files.iter().any(|file| file["path"] == RECEIPT) {
            result
                .files
                .insert(0, json!({"path":RECEIPT,"content":prepared}));
        }
        for (manifest, fingerprint) in completions {
            let completed = state
                .targets
                .get_mut(&manifest)
                .ok_or_else(|| error("native completion target missing"))?;
            completed.installed = Some(fingerprint);
            completed
                .requests
                .retain(|_, requests| !requests.is_empty());
        }
        result.finalizations.push(
            json!({"path":RECEIPT,"content":format!("{}\n",toml::to_string_pretty(&state)?)}),
        );
    }
    result.summary =
        json!({"changed":changed_entries,"kept":kept,"installations":result.commands.len()});
    Ok(Ok(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cargo_edits_preserve_comments_unrelated_fields_and_table_style() {
        let text = "# project note\n[package]\nname = \"demo\" # keep\nversion=\"1.0.0\"\n[dependencies]\nold = \"1\" # dependency note\n[dependencies.other]\nversion = \"2\"\nfeatures = [\"extra\"]\n";
        let mut manifest = Manifest::parse("cargo", text.into()).unwrap();
        assert_eq!(
            manifest.entry("dependencies:old").unwrap(),
            Some(json!("1"))
        );
        manifest
            .set(
                "dependencies:old",
                Some(json!({"version":"3","features":["a"]})),
            )
            .unwrap();
        manifest.set("dependencies:new", Some(json!("1"))).unwrap();
        let updated = manifest.content(true).unwrap();
        assert!(
            updated.contains("# project note")
                && updated.contains("# dependency note")
                && updated.contains("# keep")
        );
        assert!(updated.contains("[dependencies.other]"));
        let mut parsed = Manifest::parse("cargo", updated).unwrap();
        parsed.set("dependencies:new", None).unwrap();
        assert_eq!(
            parsed.entry("dependencies:other").unwrap(),
            Some(json!({"version":"2","features":["extra"]}))
        );
        assert_eq!(
            Manifest::parse("cargo", parsed.content(true).unwrap())
                .unwrap()
                .entry("dependencies:new")
                .unwrap(),
            None
        );
    }
    #[test]
    fn npm_edits_preserve_scripts_and_unmanaged_dependencies() {
        let mut manifest=Manifest::parse("npm",json!({"name":"demo","scripts":{"build":"keep"},"dependencies":{"owned":"1","unrelated":"2"}}).to_string()).unwrap();
        manifest
            .set("dependencies:owned", Some(json!("3")))
            .unwrap();
        manifest
            .set("devDependencies:test", Some(json!("1")))
            .unwrap();
        manifest.set("dependencies:owned", None).unwrap();
        assert_eq!(manifest.value["scripts"]["build"], "keep");
        assert_eq!(manifest.value["dependencies"]["unrelated"], "2");
        assert_eq!(manifest.value["devDependencies"]["test"], "1");
    }
    #[test]
    fn feature_union_and_incompatible_sources_are_explicit() {
        assert_eq!(
            combine(
                &json!({"version":"1","features":["a"]}),
                &json!({"version":"1","features":["b"]})
            )
            .unwrap(),
            json!({"version":"1","features":["a","b"]})
        );
        assert!(combine(&json!("1"), &json!("2")).is_err());
        assert!(
            combine(
                &json!({"version":"1","default-features":false}),
                &json!({"version":"1","default-features":true})
            )
            .is_err()
        );
        assert!(
            Manifest::parse("npm", "{\"dependencies\":[]}".into())
                .unwrap()
                .entry("dependencies:x")
                .is_err()
        );
    }
    #[test]
    fn ambiguous_or_invalid_native_entries_are_rejected_before_mutation() {
        assert!(Manifest::parse("npm", r#"{"dependencies":{"a":"1","a":"2"}}"#.into()).is_err());
        assert!(Manifest::parse("npm", r#"{"dependencies":{},"dependencies":{}}"#.into()).is_err());
        assert!(
            Manifest::parse("npm", r#"{"dependencies":{"a":null}}"#.into())
                .unwrap()
                .entry("dependencies:a")
                .is_err()
        );
    }
    #[test]
    fn settings_and_dependency_options_cannot_name_arbitrary_targets() {
        assert!(configure(&json!({"cargo":{"manifest":"../Cargo.toml"}})).is_err());
        assert!(configure(&json!({"cargo":{"manifest":"lenso.toml"}})).is_err());
        let (_, paths, commands) = configure(&json!({"cargo":{"manifest":"Cargo.toml"}})).unwrap();
        assert!(paths.contains(&RECEIPT.into()));
        assert_eq!(commands.len(), 2);
        let dependency = Dependency {
            ecosystem: "cargo".into(),
            package: "example".into(),
            version: "1".into(),
            module: None,
            options: BTreeMap::from([("unknown".into(), json!(true))]),
        };
        assert!(identity(&dependency).is_err());
    }
}
