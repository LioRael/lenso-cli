use std::collections::{BTreeMap, BTreeSet};

use extism_pdk::Json;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::layout::{Project, file, normalize};
use crate::{Result, complete, dependency, error, get, namespace, npm, question, read};

#[derive(Debug, Clone)]
pub enum Reference {
    Official(String),
    Namespace { registry: String, name: String },
    Remote(Url),
    Local(String),
}

impl Reference {
    pub fn parse(value: &str, parent: Option<&Self>) -> Result<Self> {
        if value.starts_with("https://") || value.starts_with("http://") {
            let url = Url::parse(value)?;
            let loopback = url
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|ip| ip.is_loopback());
            if (!loopback && url.scheme() != "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(error(
                    "Registry URLs require HTTPS without credentials or fragments",
                ));
            }
            return Ok(Self::Remote(url));
        }
        if let Some((registry, name)) = value
            .strip_prefix('@')
            .and_then(|value| value.split_once('/'))
        {
            if valid_name(registry) && name.split('/').all(valid_name) {
                return Ok(Self::Namespace {
                    registry: format!("@{registry}"),
                    name: name.into(),
                });
            }
            return Err(error("invalid Registry namespace address"));
        }
        if value.starts_with('.') || value.ends_with(".json") {
            return match parent {
                Some(Self::Remote(url)) => Self::parse(url.join(value)?.as_str(), None),
                Some(Self::Namespace { registry, name }) => {
                    let directory = name.rsplit_once('/').map_or("", |(dir, _)| dir);
                    let name = normalize(directory, value)?
                        .trim_end_matches(".json")
                        .to_owned();
                    if !name.split('/').all(valid_name) {
                        return Err(error("invalid relative Registry dependency"));
                    }
                    Ok(Self::Namespace {
                        registry: registry.clone(),
                        name,
                    })
                }
                Some(Self::Local(path)) => Ok(Self::Local(normalize(
                    path.rsplit_once('/').map_or("", |(dir, _)| dir),
                    value,
                )?)),
                _ => Ok(Self::Local(normalize("", value)?)),
            };
        }
        if valid_name(value) {
            Ok(Self::Official(value.into()))
        } else {
            Err(error(
                "expected a resource name, @namespace/name, Registry URL, or local JSON path",
            ))
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Official(name) => name.clone(),
            Self::Namespace { registry, name } => format!("{registry}/{name}"),
            Self::Local(path) => format!("./{path}"),
            Self::Remote(url) => {
                let mut safe = url.clone();
                safe.set_query(None);
                if url.query().is_some() {
                    format!("{safe}#{}", hex(url.query().unwrap_or_default().as_bytes()))
                } else {
                    safe.into()
                }
            }
        }
    }

    fn fetch(&self, project: &Project) -> Result<Value> {
        match self {
            Self::Official(name) => get(&format!(
                "https://ui.shadcn.com/r/styles/{}/{name}.json",
                project.registry_style
            )),
            Self::Namespace { registry, name } => {
                namespace(&file(&project.active, "components.json"), registry, name)
            }
            Self::Remote(url) => get(url.as_str()),
            Self::Local(path) => Ok(serde_json::from_str(
                &read(path)?.ok_or_else(|| error("local Registry item is missing"))?,
            )?),
        }
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}
fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Deserialize)]
struct RegistryFile {
    path: String,
    content: String,
    #[serde(rename = "type")]
    kind: String,
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryItem {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    files: Vec<RegistryFile>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    dev_dependencies: Vec<String>,
    #[serde(default)]
    registry_dependencies: Vec<String>,
    #[serde(default)]
    css: Value,
    #[serde(default)]
    css_vars: Value,
    #[serde(default)]
    env_vars: BTreeMap<String, String>,
    docs: Option<String>,
    #[serde(default)]
    tailwind: Value,
    #[serde(default)]
    font: Value,
    extends: Option<String>,
}

#[derive(Default)]
struct Dependency {
    requirements: BTreeSet<String>,
    development: bool,
}

#[derive(Default)]
struct Resolution {
    files: BTreeMap<String, String>,
    sources: BTreeMap<String, String>,
    source_paths: BTreeMap<String, String>,
    dependencies: BTreeMap<String, BTreeMap<String, Dependency>>,
    visited: BTreeSet<String>,
    active: BTreeSet<String>,
    merged: BTreeSet<String>,
    docs: Vec<String>,
    item_paths: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Add,
    Diff,
    Update,
}

fn payload_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains(['\\', ':', '\0'])
        || value.split('/').any(|part| {
            part == ".." || part == ".git" || part == ".lenso" || part == "node_modules"
        })
    {
        return Err(error("Registry file path escapes configured destinations"));
    }
    Ok(())
}

fn destination(file: &RegistryFile, project: &Project) -> Result<String> {
    payload_path(&file.path)?;
    if let Some(target) = file.target.as_ref().filter(|target| !target.is_empty()) {
        payload_path(target)?;
        for kind in ["components", "ui", "lib", "hooks"] {
            if let Some(rest) = target.strip_prefix(&format!("@{kind}/")) {
                return normalize(&project.alias_kind(kind)?, rest);
            }
        }
        return normalize(&project.active, target.strip_prefix("~/").unwrap_or(target));
    }
    let (kind, marker) = match file.kind.as_str() {
        "registry:ui" => ("ui", "ui"),
        "registry:component" | "registry:block" => ("components", "components"),
        "registry:hook" => ("hooks", "hooks"),
        "registry:lib" => ("lib", "lib"),
        "registry:page" | "registry:file" => {
            return Err(error("Registry pages and files require a target"));
        }
        _ => {
            return Err(error(format!(
                "unsupported Registry file type {}",
                file.kind
            )));
        }
    };
    let parts: Vec<&str> = file.path.split('/').collect();
    let relative = parts
        .iter()
        .rposition(|part| *part == marker)
        .map(|index| parts[index + 1..].join("/"))
        .unwrap_or_else(|| parts.last().copied().unwrap_or("").into());
    if kind == "lib" && relative == "utils.ts" {
        return Ok(project.utils.clone());
    }
    let root = if kind == "lib" || kind == "hooks" {
        project.shared_kind(kind)?
    } else {
        project.alias_kind(kind)?
    };
    normalize(&root, &relative)
}

fn package_spec(value: &str) -> Result<(&str, &str)> {
    let (name, requirement) = value
        .rfind('@')
        .filter(|index| *index > 0)
        .map_or((value, "latest"), |index| {
            (&value[..index], &value[index + 1..])
        });
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@/-_.".contains(&b))
        || requirement.is_empty()
    {
        return Err(error("invalid npm dependency specification"));
    }
    Ok((name, requirement))
}

fn add_dependency(
    state: &mut Resolution,
    owner: &str,
    spec: &str,
    development: bool,
) -> Result<()> {
    let (name, requirement) = package_spec(spec)?;
    let dependencies = state.dependencies.entry(owner.into()).or_default();
    let dependency = dependencies
        .entry(name.into())
        .or_insert_with(|| Dependency {
            development,
            ..Default::default()
        });
    dependency.development &= development;
    dependency.requirements.insert(requirement.into());
    Ok(())
}

fn css_rules(value: &Value, output: &mut String) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| error("Registry CSS must be an object"))?;
    for (key, value) in object {
        if key.contains(['{', '}', ';']) {
            return Err(error("invalid Registry CSS key"));
        }
        match value {
            Value::String(value) => {
                output.push_str(&format!("{key}: {value};\n"));
            }
            Value::Object(value) if value.is_empty() && key.starts_with('@') => {
                output.push_str(&format!("{key};\n"));
            }
            Value::Object(_) => {
                output.push_str(&format!("{key} {{\n"));
                css_rules(value, output)?;
                output.push_str("}\n");
            }
            _ => {
                return Err(error(
                    "Registry CSS values must be strings or nested objects",
                ));
            }
        }
    }
    Ok(())
}

fn merge_css(
    item: &RegistryItem,
    reference: &Reference,
    project: &Project,
    state: &mut Resolution,
) -> Result<()> {
    if item.css.is_null() && item.css_vars.is_null() {
        return Ok(());
    }
    let path = project.css()?;
    let mut additions = String::new();
    if let Some(vars) = item.css_vars.as_object() {
        for (group, vars) in vars {
            let (prefix, suffix) = match group.as_str() {
                "theme" => ("@theme inline {\n", "}\n"),
                "light" => ("@layer base { :root {\n", "} }\n"),
                "dark" => ("@layer base { .dark {\n", "} }\n"),
                _ => return Err(error("unsupported Registry CSS variable group")),
            };
            additions.push_str(prefix);
            for (name, value) in vars
                .as_object()
                .ok_or_else(|| error("CSS variable groups must be objects"))?
            {
                let value = value
                    .as_str()
                    .ok_or_else(|| error("CSS variable values must be strings"))?;
                if !name
                    .trim_start_matches("--")
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
                {
                    return Err(error("invalid CSS variable name"));
                }
                additions.push_str(&format!("--{}: {value};\n", name.trim_start_matches("--")));
            }
            additions.push_str(suffix);
        }
    } else if !item.css_vars.is_null() {
        return Err(error("cssVars must be an object"));
    }
    if !item.css.is_null() {
        css_rules(&item.css, &mut additions)?;
    }
    if additions.is_empty() {
        return Ok(());
    }
    let key = hex(reference.key().as_bytes());
    let start = format!("/* lenso:registry {key} */");
    let end = format!("/* /lenso:registry {key} */");
    let mut css = state
        .files
        .get(&path)
        .cloned()
        .or(read(&path)?)
        .ok_or_else(|| error("CSS entry disappeared"))?;
    let block = format!("{start}\n{additions}{end}");
    if let Some(first) = css.find(&start) {
        let last = css[first..]
            .find(&end)
            .map(|index| first + index + end.len())
            .ok_or_else(|| error("unterminated existing Registry CSS block"))?;
        css.replace_range(first..last, &block);
    } else {
        css = format!("{}\n\n{block}\n", css.trim_end());
    }
    state.files.insert(path.clone(), css);
    state.merged.insert(path);
    Ok(())
}

fn merge_env(item: &RegistryItem, project: &Project, state: &mut Resolution) -> Result<()> {
    if item.env_vars.is_empty() {
        return Ok(());
    }
    let path = file(&project.active, ".env");
    let mut text = state
        .files
        .get(&path)
        .cloned()
        .or(read(&path)?)
        .unwrap_or_default();
    for (key, value) in &item.env_vars {
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(error("invalid environment variable name"));
        }
        if text.lines().any(|line| {
            line.trim()
                .strip_prefix("export ")
                .unwrap_or(line.trim())
                .split_once('=')
                .is_some_and(|(name, _)| name.trim() == key)
        }) {
            continue;
        }
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("{key}={}\n", serde_json::to_string(value)?));
    }
    state.files.insert(path.clone(), text);
    state.merged.insert(path);
    Ok(())
}

fn visit(reference: Reference, project: &Project, state: &mut Resolution) -> Result<()> {
    let key = reference.key();
    if state.visited.contains(&key) {
        return Ok(());
    }
    if state.active.len() >= 32 || state.visited.len() >= 128 || !state.active.insert(key.clone()) {
        return Err(error(
            "Registry dependency cycle, depth, or item limit exceeded",
        ));
    }
    let item: RegistryItem = serde_json::from_value(reference.fetch(project)?)?;
    if !valid_name(&item.name) {
        return Err(error("invalid Registry item name"));
    }
    match &reference {
        Reference::Official(name) | Reference::Namespace { name, .. }
            if name.rsplit('/').next() != Some(item.name.as_str()) =>
        {
            return Err(error("Registry item identity mismatch"));
        }
        _ => {}
    }
    if ![
        "registry:ui",
        "registry:lib",
        "registry:component",
        "registry:hook",
        "registry:block",
        "registry:page",
        "registry:file",
        "registry:item",
        "registry:style",
        "registry:theme",
    ]
    .contains(&item.kind.as_str())
    {
        return Err(error(format!(
            "unsupported Registry item type {}",
            item.kind
        )));
    }
    if !item.tailwind.is_null() && item.tailwind != json!({}) {
        return Err(error(
            "Tailwind config payloads require Tailwind 3; use css/cssVars for this Tailwind 4 project",
        ));
    }
    if !item.font.is_null() || item.extends.as_deref().is_some_and(|value| value != "none") {
        return Err(error(
            "font setup and style inheritance are not supported by this consumer",
        ));
    }
    for dependency in &item.registry_dependencies {
        visit(
            Reference::parse(dependency, Some(&reference))?,
            project,
            state,
        )?;
    }
    let mut destinations = Vec::new();
    state.item_paths.insert(
        key.clone(),
        item.files
            .iter()
            .map(|entry| destination(entry, project))
            .collect::<Result<_>>()?,
    );
    for entry in &item.files {
        let target = destination(entry, project)?;
        let source = normalize("", &entry.path)?;
        state.sources.insert(
            format!("@/{}", crate::layout::module_path(&source)),
            target.clone(),
        );
        state.source_paths.insert(target, source);
    }
    for entry in &item.files {
        let target = destination(entry, project)?;
        let owner = project.owner(&target)?;
        let content = entry.content.clone();
        if (content.contains("@/lib/utils")
            || entry.content.contains("@/lib/utils")
            || (entry.content.contains("@/registry/") && entry.content.contains("/lib/utils")))
            && !matches!(&reference, Reference::Official(name) if name == "utils")
        {
            visit(Reference::Official("utils".into()), project, state)?;
        }
        for dependency in [
            "class-variance-authority",
            "lucide-react",
            "@radix-ui/react-icons",
            "@tabler/icons-react",
        ] {
            if ['\'', '"']
                .iter()
                .any(|quote| content.contains(&format!("{quote}{dependency}{quote}")))
            {
                add_dependency(state, &owner.dir, dependency, false)?;
            }
        }
        destinations.push((owner.dir.clone(), content.clone()));
        state.files.insert(target, content);
    }
    for (development, dependencies) in [(false, &item.dependencies), (true, &item.dev_dependencies)]
    {
        for spec in dependencies {
            let (name, _) = package_spec(spec)?;
            let mut owners: BTreeSet<String> = destinations
                .iter()
                .filter(|(_, content)| {
                    ['\'', '"'].iter().any(|quote| {
                        content.contains(&format!("{quote}{name}{quote}"))
                            || content.contains(&format!("{quote}{name}/"))
                    })
                })
                .map(|(owner, _)| owner.clone())
                .collect();
            if owners.is_empty() {
                if destinations.is_empty() {
                    owners.insert(project.active.clone());
                } else {
                    owners.extend(destinations.iter().map(|(owner, _)| owner.clone()));
                }
            }
            for owner in owners {
                add_dependency(state, &owner, spec, development)?;
            }
        }
    }
    merge_css(&item, &reference, project, state)?;
    merge_env(&item, project, state)?;
    if let Some(docs) = item.docs {
        state.docs.push(docs);
    }
    state.active.remove(&key);
    state.visited.insert(key);
    Ok(())
}

pub fn resolve(ctx: &Value, mode: Mode) -> Result<Json<Value>> {
    let project: Project = serde_json::from_value(dependency(ctx, "project")?.clone())?;
    let selection = dependency(ctx, "selection")?;
    let items: Vec<String> = selection
        .get("items")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_else(|| vec![selection["item"].as_str().unwrap_or("").into()]);
    let mut state = Resolution::default();
    for item in &items {
        let reference = Reference::parse(item, None)?;
        let key = reference.key();
        visit(reference, &project, &mut state)?;
        if mode == Mode::Update {
            let mut installed = false;
            for path in &state.item_paths[&key] {
                installed |= read(path)?.is_some();
            }
            if !installed {
                return Err(error(format!(
                    "{key} has no installed component files in this workspace; use add to install it"
                )));
            }
        }
    }
    if project.initialize {
        let path = file(&project.active, "components.json");
        state.files.insert(
            path.clone(),
            format!("{}\n", serde_json::to_string_pretty(&project.config)?),
        );
        state.merged.insert(path);
    }
    for (target, source) in &state.source_paths {
        if (crate::layout::module_path(source) != source
            || crate::layout::module_path(target) != target)
            && let Some(content) = state.files.get(target)
        {
            let content = project.rewrite(content, target, source, &state.sources)?;
            state.files.insert(target.clone(), content);
        }
    }
    for (owner, dependencies) in &state.dependencies {
        let target = file(owner, "package.json");
        let mut package = project.package(owner)?.package.clone();
        let mut changed = false;
        for (name, dependency) in dependencies {
            if package["dependencies"].get(name).is_some()
                || package["devDependencies"].get(name).is_some()
                || package["peerDependencies"].get(name).is_some()
            {
                continue;
            }
            let version = npm(
                name,
                &dependency.requirements.iter().cloned().collect::<Vec<_>>(),
            )?;
            let section = if dependency.development {
                "devDependencies"
            } else {
                "dependencies"
            };
            if package.get(section).is_none() {
                package[section] = json!({});
            }
            package[section][name] = json!(version);
            changed = true;
        }
        if changed {
            state.files.insert(
                target.clone(),
                format!("{}\n", serde_json::to_string_pretty(&package)?),
            );
            state.merged.insert(target);
        }
    }
    let mut files = Vec::new();
    let checked = state.files.len();
    let mut unchanged = 0;
    let mut kept = Vec::new();
    for (path, content) in state.files {
        if let Some(existing) = read(&path)? {
            if existing == content {
                unchanged += 1;
                continue;
            }
            if mode != Mode::Diff && !state.merged.contains(&path) {
                let id = format!("conflict:{path}");
                match ctx["answers"][&id].as_str() {
                    None => {
                        return Ok(question(
                            &id,
                            format!(
                                "Existing {path} differs.\n--- current\n{existing}\n+++ proposed\n{content}\nChoose overwrite or keep."
                            ),
                            json!({"type":"select","options":["overwrite","keep"]}),
                        ));
                    }
                    Some("keep") => {
                        kept.push(path);
                        continue;
                    }
                    Some("overwrite") => {}
                    Some(_) => return Err(error("invalid conflict decision")),
                }
            }
        }
        files.push(json!({"path":path,"content":content}));
    }
    let (program, args) = match project.package_manager.as_str() {
        "npm" => (
            "npm",
            vec!["install", "--ignore-scripts", "--no-audit", "--no-fund"],
        ),
        "pnpm" => ("pnpm", vec!["install", "--ignore-scripts"]),
        "bun" => ("bun", vec!["install", "--ignore-scripts"]),
        _ => return Err(error("unsupported package manager")),
    };
    let commands = if state.dependencies.is_empty() {
        Vec::new()
    } else {
        vec![json!({"program":program,"args":args})]
    };
    let installed = items.join(", ");
    Ok(complete(
        json!({"summary":{"checked":checked,"changed":files.len(),"unchanged":unchanged,"kept":kept.len()},"kept":kept,"files":files,"commands":commands,"installed_item":installed,"installed_items":items,"package_manager":project.package_manager,"registry_items":state.visited,"docs":state.docs}),
    ))
}
