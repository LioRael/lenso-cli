//! Deterministic generation from a resolved graph. Source loading and project writes belong to callers.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use lenso_composition::{Request, Resolution};
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Generation failures are data/author errors, not project mutations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Unsupported schema, unsafe path, conflicting destinations or invalid metadata.
    #[error("invalid generator: {0}")]
    Invalid(String),
    /// A template could not be compiled or rendered.
    #[error(transparent)]
    Template(#[from] minijinja::Error),
}

/// Ownership and repetition scope of one output declaration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Emit once for the whole graph, under `assembly/`.
    Composition,
    /// Emit once per selected module definition, under `modules/<module>/`.
    #[default]
    Module,
    /// Emit once per instance, under `instances/<address>/`.
    Instance,
}

/// A source template and a static, project-independent output destination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFile {
    /// Repetition and ownership scope.
    pub scope: Scope,
    /// Optional module eligibility filter; invalid for composition scope.
    #[serde(default)]
    pub module: Option<String>,
    /// Catalog-relative UTF-8 template source.
    pub template: String,
    /// Relative path within the selected scope's output directory.
    pub target: String,
}

/// A native dependency declaration; installation is a separate, authorized toolchain operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// Ecosystem identity, such as `cargo` or `npm`.
    pub ecosystem: String,
    /// Native package identity.
    pub package: String,
    /// Native package manager's version requirement, not a CLI extension constraint.
    pub version: String,
    /// Module activating this requirement; omission makes it composition-wide.
    #[serde(default)]
    pub module: Option<String>,
    /// Ecosystem-specific options, interpreted only by the consuming extension.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, serde_json::Value>,
}

/// Versioned template generator declaration, distributed with a module catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorSpec {
    /// Contract schema, currently 1.
    pub schema_version: u32,
    /// Dedicated generated directory under the profile target, default `generated`.
    #[serde(default = "generated_directory")]
    pub directory: String,
    /// Output declarations; order has no semantic meaning.
    pub files: Vec<TemplateFile>,
    /// Selected native dependency proposals, also available to manifest templates.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}
fn generated_directory() -> String {
    "generated".into()
}

/// Immutable input for arbitrary extension-owned generators.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorInput {
    /// Input contract schema, currently 1.
    pub schema_version: u32,
    /// User intent, including roots and explicit instance sharing.
    pub request: Request,
    /// Validated effective graph in dependency-first order.
    pub resolution: Resolution,
}

/// Exact output bytes with explicit owners; callers reconcile these against project baselines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedFile {
    /// Path relative to the dedicated generated directory.
    pub path: String,
    /// Complete UTF-8 contents.
    pub content: String,
    /// Ownership/repetition scope.
    pub scope: Scope,
    /// Source module for module and instance scopes.
    pub module: Option<String>,
    /// All effective instance owners.
    pub owners: BTreeSet<String>,
}

/// A proposal, never a permission to mutate files or install native packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorOutput {
    /// Output contract schema, currently 1.
    pub schema_version: u32,
    /// Deterministically ordered output files.
    pub files: Vec<GeneratedFile>,
    /// Selected native requirements; generated manifests can consume these.
    pub dependencies: Vec<Dependency>,
}

/// An extension-owned implementation receives data and returns proposed exact bytes.
pub trait Generator {
    /// Render without ambient I/O, environment values, processes or project writes.
    /// # Errors
    /// Returns invalid metadata, unsafe output or render errors.
    fn generate(&self, input: &GeneratorInput) -> Result<GeneratorOutput, Error>;
}

/// Reference generator using explicitly loaded, strict MiniJinja templates.
pub struct TemplateGenerator<'a> {
    /// Declarative generator configuration.
    pub spec: &'a GeneratorSpec,
    /// Template source map, keyed by exactly the catalog-relative template path.
    pub templates: &'a BTreeMap<String, String>,
}

/// Validate a portable relative output/source path.
/// # Errors
/// Rejects traversal, platform-specific separators and reserved directories.
pub fn relative(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.contains(['\\', ':', '\0'])
        || path.bytes().any(|byte| byte < 32 || byte == 127)
        || path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part == ".git"
                || part == ".lenso"
                || part == "node_modules"
                || part.ends_with(['.', ' '])
                || part.contains(['<', '>', '"', '|', '?', '*'])
                || part.split('.').next().is_some_and(|stem| {
                    matches!(
                        stem.to_ascii_uppercase().as_str(),
                        "CON"
                            | "PRN"
                            | "AUX"
                            | "NUL"
                            | "COM1"
                            | "COM2"
                            | "COM3"
                            | "COM4"
                            | "COM5"
                            | "COM6"
                            | "COM7"
                            | "COM8"
                            | "COM9"
                            | "LPT1"
                            | "LPT2"
                            | "LPT3"
                            | "LPT4"
                            | "LPT5"
                            | "LPT6"
                            | "LPT7"
                            | "LPT8"
                            | "LPT9"
                    )
                })
        })
    {
        return Err(Error::Invalid(format!("unsafe portable path {path:?}")));
    }
    Ok(())
}
/// Stable injective native identifier; dots and hyphens never collide.
pub fn symbol(id: &str) -> String {
    use std::fmt::Write as _;
    let mut result = String::from("i_");
    for byte in id.bytes() {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
impl GeneratorSpec {
    /// Validate declaration shape and references, including unselected modules.
    /// # Errors
    /// Rejects unknown modules, invalid paths, oversized or ambiguous declarations.
    pub fn validate(&self, modules: &BTreeSet<String>) -> Result<(), Error> {
        relative(&self.directory)?;
        if self.schema_version != 1
            || self.files.is_empty()
            || self.files.len() > 127
            || self.dependencies.len() > 128
            || self
                .directory
                .split('/')
                .next()
                .is_some_and(|name| name == "modules" || name == "composition.json")
        {
            return Err(Error::Invalid(
                "schema 1 requires 1-127 outputs and an isolated directory".into(),
            ));
        }
        for file in &self.files {
            relative(&file.template)?;
            relative(&file.target)?;
            if file
                .module
                .as_ref()
                .is_some_and(|module| !modules.contains(module))
                || file.scope == Scope::Composition && file.module.is_some()
            {
                return Err(Error::Invalid(format!(
                    "invalid module filter for {}",
                    file.template
                )));
            }
        }
        for (index, file) in self.files.iter().enumerate() {
            for other in &self.files[index + 1..] {
                let same_scope = file.scope == other.scope
                    && (file.module.is_none()
                        || other.module.is_none()
                        || file.module == other.module);
                let a = file.target.to_lowercase();
                let b = other.target.to_lowercase();
                if same_scope
                    && (a == b
                        || a.starts_with(&format!("{b}/"))
                        || b.starts_with(&format!("{a}/")))
                {
                    return Err(Error::Invalid(format!(
                        "conflicting output declarations {}",
                        file.target
                    )));
                }
            }
        }
        for dependency in &self.dependencies {
            if dependency.ecosystem.is_empty()
                || dependency.package.is_empty()
                || dependency.version.is_empty()
                || dependency
                    .module
                    .as_ref()
                    .is_some_and(|module| !modules.contains(module))
            {
                return Err(Error::Invalid("invalid dependency declaration".into()));
            }
        }
        Ok(())
    }
}
fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_fuel(Some(100_000));
    env.add_filter("symbol", |value: String| symbol(&value));
    env.add_filter("rust_string", |value: String| format!("{value:?}"));
    env
}
impl TemplateGenerator<'_> {
    /// Compile every supplied template, including inactive output declarations.
    /// # Errors
    /// Rejects missing or malformed source templates and excessive template bytes.
    pub fn validate_templates(&self) -> Result<(), Error> {
        if self.templates.values().map(String::len).sum::<usize>() > 1024 * 1024 {
            return Err(Error::Invalid("templates exceed 1 MiB".into()));
        }
        let env = environment();
        for file in &self.spec.files {
            let template = self
                .templates
                .get(&file.template)
                .ok_or_else(|| Error::Invalid(format!("missing template {}", file.template)))?;
            env.template_from_str(template)?;
        }
        Ok(())
    }
}
struct BoundedWriter {
    bytes: Vec<u8>,
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len() + bytes.len() > 1024 * 1024 {
            return Err(io::Error::other("generated output exceeds 1 MiB"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Generator for TemplateGenerator<'_> {
    fn generate(&self, input: &GeneratorInput) -> Result<GeneratorOutput, Error> {
        if input.schema_version != 1
            || input.resolution.schema_version != 1
            || input.resolution.instances.is_empty()
            || input.resolution.instances.len() > 128
        {
            return Err(Error::Invalid("invalid resolved generator input".into()));
        }
        let selected: BTreeSet<_> = input
            .resolution
            .instances
            .values()
            .map(|instance| instance.module.clone())
            .collect();
        // Full-catalog reference validation is also performed by the author loader.
        let declared: BTreeSet<_> = selected
            .iter()
            .cloned()
            .chain(
                self.spec
                    .files
                    .iter()
                    .filter_map(|file| file.module.clone()),
            )
            .chain(
                self.spec
                    .dependencies
                    .iter()
                    .filter_map(|dependency| dependency.module.clone()),
            )
            .collect();
        self.spec.validate(&declared)?;
        self.validate_templates()?;
        for (id, instance) in &input.resolution.instances {
            if id.split('.').any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            }) || instance.module.contains('.')
            {
                return Err(Error::Invalid("invalid instance address".into()));
            }
            relative(id)?;
            relative(&instance.module)?;
        }
        let dependencies: Vec<_> = self
            .spec
            .dependencies
            .iter()
            .filter(|dependency| {
                dependency
                    .module
                    .as_ref()
                    .is_none_or(|module| selected.contains(module))
            })
            .cloned()
            .collect();
        let env = environment();
        let mut output = BTreeMap::new();
        let mut total = 0;
        for file in &self.spec.files {
            let variants: Vec<(Option<&str>, Option<&str>)> = match file.scope {
                Scope::Composition => vec![(None, None)],
                Scope::Module => selected
                    .iter()
                    .filter(|module| file.module.as_ref().is_none_or(|filter| filter == *module))
                    .map(|module| (Some(module.as_str()), None))
                    .collect(),
                Scope::Instance => input
                    .resolution
                    .instances
                    .iter()
                    .filter(|(_, instance)| {
                        file.module
                            .as_ref()
                            .is_none_or(|filter| filter == &instance.module)
                    })
                    .map(|(id, instance)| (Some(instance.module.as_str()), Some(id.as_str())))
                    .collect(),
            };
            for (module, id) in variants {
                let path = match file.scope {
                    Scope::Composition => format!("assembly/{}", file.target),
                    Scope::Module => {
                        format!("modules/{}/{}", module.unwrap_or_default(), file.target)
                    }
                    Scope::Instance => {
                        format!("instances/{}/{}", id.unwrap_or_default(), file.target)
                    }
                };
                relative(&path)?;
                let owners: BTreeSet<_> = input
                    .resolution
                    .instances
                    .iter()
                    .filter(|(owner, instance)| {
                        id.map_or_else(
                            || module.is_none_or(|module| module == instance.module),
                            |id| id == owner.as_str(),
                        )
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                let instance = id.map(|id| { let instance = &input.resolution.instances[id]; json!({"id":id,"symbol":symbol(id),"module":instance.module,"version":instance.version,"configuration":instance.configuration,"bindings":instance.bindings}) });
                let context = json!({"schema_version":1,"request":input.request,"resolution":input.resolution,"module":module,"modules":selected,"instance":instance,"dependencies":dependencies});
                let template = env.template_from_str(&self.templates[&file.template])?;
                let mut writer = BoundedWriter { bytes: Vec::new() };
                template.render_captured_to(context, &mut writer)?;
                let content = String::from_utf8(writer.bytes)
                    .map_err(|_| Error::Invalid("non UTF-8 template output".into()))?;
                total += content.len();
                if output.len() >= 127
                    || total > 1024 * 1024
                    || output.keys().any(|old: &String| {
                        old.eq_ignore_ascii_case(&path)
                            || old.starts_with(&format!("{path}/"))
                            || path.starts_with(&format!("{old}/"))
                    })
                {
                    return Err(Error::Invalid(format!(
                        "oversized or conflicting output path {path}"
                    )));
                }
                output.insert(
                    path.clone(),
                    GeneratedFile {
                        path,
                        content,
                        scope: file.scope,
                        module: module.map(str::to_owned),
                        owners,
                    },
                );
            }
        }
        Ok(GeneratorOutput {
            schema_version: 1,
            files: output.into_values().collect(),
            dependencies,
        })
    }
}
