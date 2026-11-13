//! Project conventions and overridable defaults, owned by Code.
use crate::{Result, error, local};
use lenso_composition::{Binding, Choice, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub const FILE: &str = "lenso.code.toml";
fn version() -> u32 {
    1
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    pub modules: Option<String>,
    pub import_alias: Option<String>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Defaults {
    pub providers: BTreeMap<String, String>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Module {
    pub options: BTreeMap<String, Value>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(default = "version")]
    pub schema_version: u32,
    pub project: BTreeMap<String, String>,
    pub paths: Paths,
    pub conventions: BTreeMap<String, Value>,
    pub defaults: Defaults,
    pub modules: BTreeMap<String, Module>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            project: Default::default(),
            paths: Default::default(),
            conventions: Default::default(),
            defaults: Default::default(),
            modules: Default::default(),
        }
    }
}
pub(crate) fn scalar(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        Some(text.into())
    } else if value.is_boolean() || value.is_i64() {
        Some(value.to_string())
    } else {
        None
    }
}
impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > 65536 {
            return Err(error("Code project configuration exceeds 64 KiB"));
        }
        let value: Self = toml::from_str(text)?;
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.project.len() + self.conventions.len() > 32
            || self.modules.len() > 64
            || self.defaults.providers.len() > 64
        {
            return Err(error("invalid Code project configuration"));
        }
        if self.conventions.values().any(|v| scalar(v).is_none()) {
            return Err(error("conventions must be strings, booleans or integers"));
        }
        if toml::to_string(self)?.len() > 65536 {
            return Err(error("Code project configuration exceeds 64 KiB"));
        }
        for (key, value) in self.fixed() {
            if key.is_empty() || value.trim().is_empty() || key.len() > 128 || value.len() > 256 {
                return Err(error("invalid project convention"));
            }
            if self
                .project
                .get(&key)
                .zip(self.conventions.get(&key))
                .is_some_and(|(a, b)| scalar(b).as_ref() != Some(a))
            {
                return Err(error("contradictory project conventions"));
            }
        }
        if let Some(path) = &self.paths.modules {
            crate::relative(path)?;
        }
        if self
            .paths
            .import_alias
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 256)
        {
            return Err(error("invalid import alias"));
        }
        for module in self.defaults.providers.values() {
            crate::relative(module)?;
            if module.contains('/') {
                return Err(error("default providers must name catalog modules"));
            }
        }
        Ok(())
    }
    pub fn fixed(&self) -> BTreeMap<String, String> {
        let mut context = self.project.clone();
        context.extend(
            self.conventions
                .iter()
                .filter_map(|(k, v)| scalar(v).map(|v| (k.clone(), v))),
        );
        if let Some(path) = &self.paths.modules {
            context.insert("modules_path".into(), path.clone());
        }
        if let Some(alias) = &self.paths.import_alias {
            context.insert("import_alias".into(), alias.clone());
        }
        context
    }
    pub fn constrain(&self, request: &mut Request, reconfigure: bool) -> Result<()> {
        for (key, value) in self.fixed() {
            if request.context.get(&key).is_some_and(|old| old != &value)
                && (!reconfigure || matches!(key.as_str(), "language" | "runtime"))
            {
                return Err(error(format!(
                    "project convention {key} conflicts with the installed or requested value; preview with --reconfigure to review the change"
                )));
            }
            request.context.insert(key, value);
        }
        Ok(())
    }
    pub fn module_defaults(&self, request: &mut Request) {
        for (id, module) in &request.instances {
            if let Some(defaults) = self.modules.get(module) {
                let config = request.configuration.entry(id.clone()).or_default();
                for (key, value) in &defaults.options {
                    config.entry(key.clone()).or_insert(value.clone());
                }
            }
        }
        for (address, binding) in &request.bindings {
            let module = match binding {
                Binding::Module { module } => Some(module),
                Binding::Instance { instance } => request.instances.get(instance),
                Binding::Disabled => None,
            };
            if let Some(defaults) = module.and_then(|name| self.modules.get(name)) {
                let config = request.configuration.entry(address.clone()).or_default();
                for (key, value) in &defaults.options {
                    config.entry(key.clone()).or_insert(value.clone());
                }
            }
        }
    }
    pub fn provider(&self, choice: &Choice) -> Result<Option<Binding>> {
        if let Choice::Provider {
            contract, modules, ..
        } = choice
            && let Some(module) = self.defaults.providers.get(contract)
        {
            if !modules.contains(module) {
                return Err(error(format!(
                    "project default provider {module} does not satisfy {contract}"
                )));
            }
            return Ok(Some(Binding::Module {
                module: module.clone(),
            }));
        }
        Ok(None)
    }
}
pub fn load(ctx: &Value) -> Result<Config> {
    // Older installations retain their original grants until Code setup is reviewed again.
    if !ctx["capabilities"]["host.reads"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|p| p == FILE))
    {
        return Ok(Config::default());
    }
    local(FILE)?
        .map(|text| Config::parse(&text))
        .transpose()
        .map(|v| v.unwrap_or_default())
}
#[extism_pdk::plugin_fn]
pub fn config(
    extism_pdk::Json(ctx): extism_pdk::Json<Value>,
) -> extism_pdk::FnResult<extism_pdk::Json<Value>> {
    if !ctx["capabilities"]["host.reads"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|p| p == FILE))
    {
        return Err(error("review Code setup to grant access to lenso.code.toml").into());
    }
    let current = load(&ctx)?;
    for flag in ["language", "runtime", "modules-path", "import-alias"] {
        if !ctx["input"][flag].is_null() && !ctx["input"][flag].is_string() {
            return Err(error(format!("{flag} must be a string")).into());
        }
    }
    let changed = [
        "language",
        "runtime",
        "modules-path",
        "import-alias",
        "convention",
        "provider",
    ]
    .iter()
    .any(|name| !ctx["input"][*name].is_null());
    if ctx["input"]["settings"].is_null() && !changed {
        return Ok(crate::complete(
            json!({"config":current,"files":[],"commands":[]}),
        ));
    }
    if changed && !ctx["input"]["settings"].is_null() {
        return Err(error("choose settings JSON or individual configuration flags").into());
    }
    let mut proposed: Config = if let Some(settings) = ctx["input"].get("settings") {
        serde_json::from_value(settings.clone())?
    } else {
        current.clone()
    };
    for field in ["language", "runtime"] {
        if let Some(value) = ctx["input"][field].as_str() {
            proposed.project.insert(field.into(), value.into());
        }
    }
    if let Some(value) = ctx["input"]["modules-path"].as_str() {
        proposed.paths.modules = Some(value.into());
    }
    if let Some(value) = ctx["input"]["import-alias"].as_str() {
        proposed.paths.import_alias = Some(value.into());
    }
    for flag in ["convention", "provider"] {
        if let Some(input) = ctx["input"].get(flag) {
            let items: Vec<String> = if let Some(text) = input.as_str() {
                vec![text.into()]
            } else {
                serde_json::from_value(input.clone())?
            };
            for item in items {
                let (key, value) = item
                    .split_once('=')
                    .ok_or_else(|| error("configuration entries must use key=value"))?;
                if key.is_empty() || value.is_empty() {
                    return Err(error("configuration key and value must not be empty").into());
                }
                if flag == "convention" {
                    let parsed = serde_json::from_str::<Value>(value)
                        .ok()
                        .filter(|v| v.is_boolean() || v.is_i64())
                        .unwrap_or_else(|| json!(value));
                    proposed.conventions.insert(key.into(), parsed);
                } else {
                    proposed.defaults.providers.insert(key.into(), value.into());
                }
            }
        }
    }
    proposed.validate()?;
    for field in ["language", "runtime"] {
        if let Some(installed) =
            ctx["capabilities"]["extension.options"]["selection"][field].as_str()
            && proposed.fixed().get(field).is_some_and(|v| v != installed)
        {
            return Err(
                error("changing installed language/runtime requires a new project target").into(),
            );
        }
    }
    let mut affected = std::collections::BTreeSet::new();
    if let Some(profiles) = ctx["capabilities"]["extension.options"]["registries"].as_object() {
        for profile in profiles.values() {
            if let Some(target) = profile["target"].as_str().filter(|p| !p.is_empty()) {
                crate::relative(target)?;
                if let Some(index) = local(&format!("{target}/code.modules.json"))? {
                    let index: Value = serde_json::from_str(&index)?;
                    for field in ["language", "runtime"] {
                        if proposed
                            .project
                            .get(field)
                            .is_some_and(|v| index[field].as_str().is_some_and(|old| old != v))
                        {
                            return Err(error(
                                "changing installed language/runtime requires a new project target",
                            )
                            .into());
                        }
                    }
                    if current.fixed() != proposed.fixed()
                        && let Some(modules) = index["modules"].as_object()
                    {
                        affected.extend(modules.keys().cloned());
                    }
                }
            }
        }
    }
    let text = if changed {
        let mut document = local(FILE)?
            .map(|text| text.parse::<toml_edit::DocumentMut>())
            .transpose()?
            .unwrap_or_default();
        let fresh = toml_edit::ser::to_document(&proposed)?;
        fn replace(current: &mut toml_edit::Item, mut new: toml_edit::Item) {
            if let (Some(old), Some(value)) = (current.as_value(), new.as_value_mut()) {
                *value.decor_mut() = old.decor().clone();
            }
            *current = new;
        }
        for (flag, section, key) in [
            ("language", "project", "language"),
            ("runtime", "project", "runtime"),
            ("modules-path", "paths", "modules"),
            ("import-alias", "paths", "import_alias"),
        ] {
            if !ctx["input"][flag].is_null() {
                replace(&mut document[section][key], fresh[section][key].clone());
            }
        }
        for (flag, section) in [("convention", "conventions"), ("provider", "providers")] {
            if let Some(input) = ctx["input"].get(flag) {
                let entries: Vec<String> = if let Some(value) = input.as_str() {
                    vec![value.into()]
                } else {
                    serde_json::from_value(input.clone())?
                };
                for entry in entries {
                    let key = entry
                        .split_once('=')
                        .ok_or_else(|| error("invalid configuration entry"))?
                        .0;
                    if flag == "provider" {
                        replace(
                            &mut document["defaults"][section][key],
                            fresh["defaults"][section][key].clone(),
                        );
                    } else {
                        replace(&mut document[section][key], fresh[section][key].clone());
                    }
                }
            }
        }
        if document.get("schema_version").is_none() {
            document["schema_version"] = toml_edit::value(1);
        }
        document.to_string()
    } else {
        toml::to_string_pretty(&proposed)?
    };
    let files = if local(FILE)?.as_deref() == Some(&text) {
        vec![]
    } else {
        vec![json!({"path":FILE,"content":text})]
    };
    Ok(crate::complete(
        json!({"message":"Review Code project conventions and defaults","affected_modules":affected,"config":proposed,"files":files,"commands":[]}),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conventions_reject_changes_until_explicit_review() {
        let c = Config::parse("[conventions]\nstyles='tailwind'").unwrap();
        let mut r = Request::default();
        r.context.insert("styles".into(), "css".into());
        assert!(c.constrain(&mut r, false).is_err());
        c.constrain(&mut r, true).unwrap();
        assert_eq!(r.context["styles"], "tailwind");
    }
    #[test]
    fn target_language_cannot_be_migrated_in_place() {
        let c = Config::parse("[project]\nlanguage='rust'").unwrap();
        let mut r = Request::default();
        r.context.insert("language".into(), "typescript".into());
        assert!(c.constrain(&mut r, true).is_err());
    }
    #[test]
    fn malformed_configuration_and_paths_fail_closed() {
        assert!(Config::parse("schema_version=2").is_err());
        assert!(Config::parse("[paths]\nmodules='../outside'").is_err());
        assert!(Config::parse("[project]\nlanguage=42").is_err());
    }
    #[test]
    fn existing_and_explicit_options_win_over_defaults() {
        let c = Config::parse("[modules.auth.options]\nttl_ms=60000").unwrap();
        let mut r = Request::default();
        r.instances.insert("customer".into(), "auth".into());
        r.configuration
            .entry("customer".into())
            .or_default()
            .insert("ttl_ms".into(), json!(120000));
        c.module_defaults(&mut r);
        assert_eq!(r.configuration["customer"]["ttl_ms"], 120000);
    }
}
