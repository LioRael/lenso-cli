use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Result, error, read, string};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub dir: String,
    pub name: String,
    pub package: Value,
    pub config: Value,
    pub paths: BTreeMap<String, Vec<String>>,
    pub initialize: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub config: Value,
    pub ui: String,
    pub utils: String,
    pub registry_style: String,
    pub initialize: bool,
    pub package_manager: String,
    pub active: String,
    pub packages: Vec<Package>,
}

/// Paths in configuration may refer to sibling workspaces, but never outside the Lenso project.
pub fn normalize(base: &str, path: &str) -> Result<String> {
    if path.starts_with('/') || path.contains(['\\', ':', '\0']) {
        return Err(error("paths must remain relative to the Lenso project"));
    }
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| error("path escapes the Lenso project"))?;
            }
            ".git" | ".lenso" | "node_modules" => {
                return Err(error("path accesses a reserved directory"));
            }
            value => parts.push(value),
        }
    }
    Ok(parts.join("/"))
}

pub fn file(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

pub fn load_package(dir: &str, fallback: Option<Value>) -> Result<Package> {
    let text = read(&file(dir, "package.json"))?
        .ok_or_else(|| error("workspace package.json is missing"))?;
    let package: Value = serde_json::from_str(&text)?;
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        if package.get(section).is_some_and(|value| {
            !value
                .as_object()
                .is_some_and(|values| values.values().all(Value::is_string))
        }) {
            return Err(error(format!(
                "package.json {section} must contain version strings"
            )));
        }
    }
    let config = read(&file(dir, "components.json"))?;
    let initialize = config.is_none();
    let mut config: Value = match config {
        Some(text) => serde_json::from_str(&text)?,
        None => fallback.unwrap_or(Value::Null),
    };
    // Authentication stays in the host. Handler reports retain only the public URL templates.
    if let Some(registries) = config.get_mut("registries").and_then(Value::as_object_mut) {
        for registry in registries.values_mut() {
            if let Some(template) = registry.as_str() {
                *registry = Value::String(template.split('?').next().unwrap_or(template).into());
            }
            if let Some(registry) = registry.as_object_mut() {
                if let Some(Value::String(template)) = registry.get_mut("url") {
                    *template = template.split('?').next().unwrap_or(template).into();
                }
                registry.remove("headers");
                registry.remove("params");
            }
        }
    }
    let mut paths = BTreeMap::new();
    if !config.is_null() {
        for name in ["tsconfig.json", "tsconfig.app.json"] {
            if let Some(found) = load_paths(&file(dir, name), &mut Vec::new())?.paths {
                paths = found;
            }
        }
    }
    Ok(Package {
        dir: dir.into(),
        name: package["name"].as_str().unwrap_or("").into(),
        package,
        config,
        paths,
        initialize,
    })
}

#[derive(Default)]
struct TsPaths {
    base: Option<String>,
    paths: Option<BTreeMap<String, Vec<String>>>,
}

fn load_paths(path: &str, stack: &mut Vec<String>) -> Result<TsPaths> {
    if stack.len() >= 16 || stack.iter().any(|entry| entry == path) {
        return Err(error("tsconfig inheritance cycle or depth limit exceeded"));
    }
    let Some(text) = read(path)? else {
        return Ok(TsPaths::default());
    };
    stack.push(path.into());
    let config: Value = json5::from_str(&text)?;
    let directory = path.rsplit_once('/').map_or("", |(base, _)| base);
    let extends: Vec<&str> = match config.get("extends") {
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| error("invalid tsconfig extends"))
            })
            .collect::<Result<_>>()?,
        None => Vec::new(),
        _ => return Err(error("invalid tsconfig extends")),
    };
    let mut inherited = TsPaths::default();
    for parent in extends {
        let parent = if parent.starts_with('.') {
            normalize(directory, parent)?
        } else {
            let mut found = None;
            for workspace in crate::workspace_paths()? {
                let package: Value = serde_json::from_str(
                    &read(&file(&workspace, "package.json"))?
                        .ok_or_else(|| error("workspace package disappeared"))?,
                )?;
                if let Some(name) = package["name"].as_str()
                    && let Some(suffix) = parent.strip_prefix(&format!("{name}/"))
                {
                    found = Some(normalize(&workspace, suffix)?);
                }
            }
            found.ok_or_else(|| {
                error("external tsconfig extends requires a local or workspace configuration")
            })?
        };
        let parent = if parent.ends_with(".json") {
            parent
        } else {
            format!("{parent}.json")
        };
        let parent = load_paths(&parent, stack)?;
        if parent.base.is_some() {
            inherited.base = parent.base;
        }
        if parent.paths.is_some() {
            inherited.paths = parent.paths;
        }
    }
    if let Some(base) = config["compilerOptions"]["baseUrl"].as_str() {
        inherited.base = Some(normalize(directory, base)?);
    }
    if let Some(values) = config["compilerOptions"].get("paths") {
        let values: BTreeMap<String, Vec<String>> = serde_json::from_value(values.clone())?;
        let base = inherited.base.as_deref().unwrap_or(directory);
        inherited.paths = Some(
            values
                .into_iter()
                .map(|(key, targets)| {
                    targets
                        .iter()
                        .map(|target| normalize(base, target))
                        .collect::<Result<Vec<_>>>()
                        .map(|targets| (key, targets))
                })
                .collect::<Result<_>>()?,
        );
    }
    stack.pop();
    Ok(inherited)
}

fn target_string(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| {
        ["source", "development", "import", "default", "types"]
            .iter()
            .find_map(|key| value.get(*key).and_then(target_string))
    })
}

fn mapping(alias: &str, key: &str, target: &str) -> Option<String> {
    if let Some((prefix, suffix)) = key.split_once('*') {
        let capture = alias
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix));
        let directory = suffix.is_empty() && alias == prefix.trim_end_matches('/');
        let capture = capture.or_else(|| directory.then_some(""))?;
        if directory {
            let (prefix, _) = target.split_once('*')?;
            Some(prefix.trim_end_matches('/').into())
        } else {
            Some(target.replace('*', capture))
        }
    } else if key == alias {
        Some(target.into())
    } else {
        None
    }
}

impl Project {
    pub fn active_package(&self) -> Result<&Package> {
        self.package(&self.active)
    }

    pub fn package(&self, dir: &str) -> Result<&Package> {
        self.packages
            .iter()
            .find(|package| package.dir == dir)
            .ok_or_else(|| error("workspace package is missing"))
    }

    pub fn owner(&self, path: &str) -> Result<&Package> {
        self.packages
            .iter()
            .filter(|package| {
                package.dir.is_empty() || path.starts_with(&format!("{}/", package.dir))
            })
            .max_by_key(|package| package.dir.len())
            .ok_or_else(|| error("file has no owning workspace"))
    }

    pub fn alias(&self, from: &Package, alias: &str) -> Result<String> {
        let mut mapped = Vec::new();
        for (key, targets) in &from.paths {
            if targets.len() != 1 {
                continue;
            }
            if let Some(path) = mapping(alias, key, &targets[0]) {
                mapped.push((key.len(), path));
            }
        }
        if let Some((_, path)) = mapped.into_iter().max_by_key(|(length, _)| *length) {
            return normalize("", &path);
        }
        if let Some(imports) = from.package["imports"].as_object() {
            for (key, target) in imports {
                if let Some(path) =
                    target_string(target).and_then(|target| mapping(alias, key, target))
                {
                    return normalize(&from.dir, &path);
                }
            }
        }
        let package = self
            .packages
            .iter()
            .filter(|package| {
                !package.name.is_empty()
                    && (alias == package.name || alias.starts_with(&format!("{}/", package.name)))
            })
            .max_by_key(|package| package.name.len())
            .ok_or_else(|| {
                error(format!(
                    "cannot resolve alias {alias}; configure paths, imports, or workspace exports"
                ))
            })?;
        let suffix = alias
            .strip_prefix(&package.name)
            .ok_or_else(|| error("invalid workspace alias"))?;
        let key = if suffix.is_empty() {
            ".".into()
        } else {
            format!(".{suffix}")
        };
        let exports = package.package["exports"]
            .as_object()
            .ok_or_else(|| error(format!("workspace {} needs source exports", package.name)))?;
        for (export, target) in exports {
            if let Some(path) =
                target_string(target).and_then(|target| mapping(&key, export, target))
            {
                if path.contains("/dist/") || path.starts_with("./dist/") {
                    return Err(error(
                        "workspace exports must identify editable source files",
                    ));
                }
                return normalize(&package.dir, &path);
            }
        }
        Err(error(format!(
            "workspace {} does not export {key}",
            package.name
        )))
    }

    pub fn alias_kind(&self, kind: &str) -> Result<String> {
        let active = self.active_package()?;
        self.alias(active, string(&active.config["aliases"], kind)?)
    }

    pub fn css(&self) -> Result<String> {
        let active = self.active_package()?;
        normalize(&active.dir, string(&active.config["tailwind"], "css")?)
    }

    pub fn shared_kind(&self, kind: &str) -> Result<String> {
        let owner = self.owner(&self.ui)?;
        self.alias(owner, string(&owner.config["aliases"], kind)?)
    }

    fn cross_alias(&self, from: &Package, kind: &str, target: &str) -> Result<String> {
        if let Some(alias) = from.config["aliases"][kind].as_str()
            && self
                .alias(from, alias)
                .is_ok_and(|path| path.trim_end_matches(".ts") == target.trim_end_matches(".ts"))
        {
            return Ok(alias.into());
        }
        let owner = self.owner(target)?;
        if owner.dir == from.dir {
            return Ok(string(&owner.config["aliases"], kind)?.into());
        }
        let relative = target
            .strip_prefix(&format!("{}/", owner.dir))
            .ok_or_else(|| error("invalid workspace target"))?;
        let exports = owner.package["exports"]
            .as_object()
            .ok_or_else(|| error("cross-workspace imports require package exports"))?;
        for (key, value) in exports {
            if let Some(value) = target_string(value) {
                let value = value.trim_start_matches("./");
                if let Some((prefix, suffix)) = value.split_once('*') {
                    if let Some(capture) = relative
                        .strip_prefix(prefix)
                        .and_then(|rest| rest.strip_suffix(suffix))
                    {
                        return Ok(format!(
                            "{}/{}",
                            owner.name,
                            key.trim_start_matches("./").replace('*', capture)
                        ));
                    }
                    if relative == prefix.trim_end_matches('/') {
                        return Ok(format!(
                            "{}/{}",
                            owner.name,
                            key.trim_start_matches("./").trim_end_matches("/*")
                        ));
                    }
                } else if value == relative {
                    return Ok(format!("{}/{}", owner.name, key.trim_start_matches("./")));
                }
            }
        }
        Err(error(format!(
            "workspace {} must export the installed target",
            owner.name
        )))
    }

    fn import_target(&self, from: &str, target: &str) -> Result<String> {
        let from_owner = self.owner(from)?;
        let target_owner = self.owner(target)?;
        let module = module_path(target);
        let mut aliases = Vec::new();
        if let Some(values) = from_owner.config["aliases"].as_object() {
            for value in values.values().filter_map(Value::as_str) {
                if let Ok(base) = self.alias(from_owner, value) {
                    let base = module_path(&base);
                    if module == base {
                        aliases.push((base.len(), value.to_owned()));
                    } else if let Some(suffix) = module.strip_prefix(&format!("{base}/")) {
                        aliases.push((base.len(), format!("{value}/{suffix}")));
                    }
                }
            }
        }
        if let Some((_, value)) = aliases.into_iter().max_by_key(|(length, _)| *length) {
            return Ok(value);
        }
        if from_owner.dir != target_owner.dir {
            let relative = target
                .strip_prefix(&format!("{}/", target_owner.dir))
                .ok_or_else(|| error("invalid workspace target"))?;
            if let Some(exports) = target_owner.package["exports"].as_object() {
                for (key, value) in exports {
                    if let Some(value) = target_string(value)
                        && let Some(public) = mapping(
                            relative,
                            value.trim_start_matches("./"),
                            key.trim_start_matches("./"),
                        )
                    {
                        return Ok(format!("{}/{}", target_owner.name, module_path(&public)));
                    }
                }
            }
            return Err(error(format!(
                "workspace {} must export {relative}",
                target_owner.name
            )));
        }
        let directory = from.rsplit_once('/').map_or("", |(dir, _)| dir);
        let left: Vec<&str> = directory
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let right: Vec<&str> = module.split('/').collect();
        let shared = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
        let mut parts = vec![".."; left.len() - shared];
        parts.extend_from_slice(&right[shared..]);
        let result = parts.join("/");
        Ok(if result.starts_with('.') {
            result
        } else {
            format!("./{result}")
        })
    }

    pub fn rewrite(
        &self,
        content: &str,
        target: &str,
        source: &str,
        sources: &BTreeMap<String, String>,
    ) -> Result<String> {
        let imports = regex_lite::Regex::new(
            r#"\b(?:from\s*|import\s*(?:\(\s*)?|require\s*\(\s*)(?:"([^"\r\n]*)"|'([^'\r\n]*)'|`([^`\r\n]*)`)"#,
        )?;
        let mut edits = Vec::new();
        for capture in imports.captures_iter(content) {
            let Some(value) = (1..=3).find_map(|index| capture.get(index)) else {
                continue;
            };
            let module = value.as_str();
            let relative = if module.starts_with('.') {
                normalize(source.rsplit_once('/').map_or("", |(dir, _)| dir), module)
                    .ok()
                    .map(|path| format!("@/{}", module_path(&path)))
            } else {
                None
            };
            let source_key = relative.as_deref().unwrap_or(module);
            if let Some(destination) = sources.get(module_path(source_key)) {
                edits.push((
                    value.start()..value.end(),
                    self.import_target(target, destination)?,
                ));
                continue;
            }
            let owner = self.owner(target)?;
            let registry_module = module.strip_prefix("@/registry/").and_then(|path| {
                let parts: Vec<&str> = path.split('/').collect();
                let index = parts
                    .iter()
                    .rposition(|part| matches!(*part, "ui" | "components" | "hooks" | "lib"))?;
                let prefix = match parts[index] {
                    "ui" => "@/components/ui",
                    "components" => "@/components",
                    "hooks" => "@/hooks",
                    "lib" => "@/lib",
                    _ => return None,
                };
                Some(format!("{prefix}/{}", parts[index + 1..].join("/")))
            });
            let module = registry_module.as_deref().unwrap_or(module);
            let Some((canonical, kind)) = [
                ("@/lib/utils", "utils"),
                ("@/components/ui", "ui"),
                ("@/components", "components"),
                ("@/hooks", "hooks"),
                ("@/lib", "lib"),
            ]
            .into_iter()
            .find(|(canonical, _)| {
                module == *canonical || module.starts_with(&format!("{canonical}/"))
            }) else {
                continue;
            };
            let target = match kind {
                "utils" => self.utils.clone(),
                "ui" => self.ui.clone(),
                "hooks" | "lib" => self.shared_kind(kind)?,
                _ => self.alias(owner, string(&owner.config["aliases"], kind)?)?,
            };
            let alias = self.cross_alias(owner, kind, &target)?;
            let suffix = module
                .strip_prefix(canonical)
                .ok_or_else(|| error("invalid import prefix"))?;
            edits.push((value.start()..value.end(), format!("{alias}{suffix}")));
        }
        let mut content = content.to_owned();
        for (range, replacement) in edits.into_iter().rev() {
            content.replace_range(range, &replacement);
        }
        Ok(content)
    }
}

pub fn build(active: &str, packages: Vec<Package>, manager: &str) -> Result<Project> {
    let package = packages
        .iter()
        .find(|package| package.dir == active)
        .ok_or_else(|| error("active workspace is missing"))?;
    let config = package.config.clone();
    if config["tsx"] != true
        || config["tailwind"]["cssVariables"] != true
        || config["tailwind"]["prefix"]
            .as_str()
            .is_some_and(|prefix| !prefix.is_empty())
    {
        return Err(error(
            "requires TypeScript, CSS variables, and no Tailwind prefix",
        ));
    }
    let style = string(&config, "style")?;
    let registry_style = match style {
        "new-york" | "default" => format!("{style}-v4"),
        style => style.into(),
    };
    if !registry_style
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(error("invalid Registry style"));
    }
    let mut project = Project {
        config,
        ui: String::new(),
        utils: String::new(),
        registry_style,
        initialize: package.initialize,
        package_manager: manager.into(),
        active: active.into(),
        packages,
    };
    project.ui = project.alias_kind("ui")?;
    let utils = project.alias_kind("utils")?;
    project.utils = if utils.ends_with(".ts") {
        utils
    } else {
        format!("{utils}.ts")
    };
    for owner in [project.owner(&project.ui)?, project.owner(&project.utils)?] {
        if owner.config.is_null() {
            return Err(error("target workspace needs components.json"));
        }
        for key in ["style", "iconLibrary"] {
            if !owner.config.is_null() && owner.config.get(key) != project.config.get(key) {
                return Err(error(format!("workspace configurations must share {key}")));
            }
        }
        if !owner.config.is_null()
            && owner.config["tailwind"]["baseColor"] != project.config["tailwind"]["baseColor"]
        {
            return Err(error("workspace configurations must share baseColor"));
        }
    }
    let css = read(&project.css()?)?.ok_or_else(|| error("configured CSS entry is missing"))?;
    if !css.contains("tailwindcss")
        || project.config["tailwind"]["config"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    {
        return Err(error("requires an existing Tailwind 4 CSS entry"));
    }
    Ok(project)
}

pub fn module_path(path: &str) -> &str {
    [".tsx", ".ts", ".jsx", ".js", ".mts", ".cts", ".mjs", ".cjs"]
        .iter()
        .find_map(|suffix| path.strip_suffix(suffix))
        .unwrap_or(path)
}

pub fn default_config() -> Value {
    json!({"$schema":"https://ui.shadcn.com/schema.json","style":"new-york","rsc":false,"tsx":true,
        "tailwind":{"config":"","css":"src/index.css","baseColor":"zinc","cssVariables":true,"prefix":""},
        "aliases":{"components":"@/components","ui":"@/components/ui","utils":"@/lib/utils","lib":"@/lib","hooks":"@/hooks"}})
}
