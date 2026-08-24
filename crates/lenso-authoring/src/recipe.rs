use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

use crate::{
    AuthoringError, Binding, ContractInput, ExecutionLane, Module, PackageInput, ProjectFile,
    WebProfile,
};

/// Current version of the reusable Composition recipe document.
pub const COMPOSITION_RECIPE_SCHEMA_VERSION: u32 = 1;

fn default_recipe_schema() -> u32 {
    COMPOSITION_RECIPE_SCHEMA_VERSION
}

fn default_root() -> String {
    ".".to_owned()
}

/// One named App variant assembled from ordinary Project fragments.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CompositionVariant {
    fragments: Vec<String>,
    output: String,
    #[serde(default)]
    profile: Option<String>,
}

/// Structured command owned by an App and launched by authoring workflows.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CompositionRunner {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    execution_classes: Vec<String>,
}

impl CompositionRunner {
    /// Returns the executable name or path without shell interpretation.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Returns the exact argument vector supplied to the executable.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns the execution classes required by the product-owned Runner.
    pub fn execution_classes(&self) -> &[String] {
        &self.execution_classes
    }
}

impl CompositionVariant {
    /// Returns fragment paths in deterministic authored order.
    pub fn fragments(&self) -> &[String] {
        &self.fragments
    }

    /// Returns the canonical Plan output path relative to the recipe root.
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Returns the optional pre-resolution Project profile.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }
}

/// Reusable authoring recipe that expands named variants into exact Project files.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CompositionRecipe {
    #[serde(default = "default_recipe_schema")]
    schema_version: u32,
    #[serde(default = "default_root")]
    root: String,
    #[serde(default)]
    runner: Option<CompositionRunner>,
    variants: BTreeMap<String, CompositionVariant>,
}

impl CompositionRecipe {
    /// Returns the recipe schema version.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the authored root path relative to the recipe document.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Returns the product-owned Runner used by `compose run` and `compose dev`.
    pub fn runner(&self) -> Option<&CompositionRunner> {
        self.runner.as_ref()
    }

    /// Returns every named variant in stable lexical order.
    pub fn variants(&self) -> &BTreeMap<String, CompositionVariant> {
        &self.variants
    }

    /// Returns one named variant.
    pub fn variant(&self, name: &str) -> Option<&CompositionVariant> {
        self.variants.get(name)
    }
}

/// Filesystem handle for one Composition recipe document.
#[derive(Clone, Debug)]
pub struct CompositionRecipePath {
    path: PathBuf,
}

impl CompositionRecipePath {
    /// Creates a handle without reading the document.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the recipe document path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads and validates the recipe document.
    pub fn load(&self) -> Result<CompositionRecipe, AuthoringError> {
        let recipe: CompositionRecipe = read_json(&self.path)?;
        if recipe.schema_version != COMPOSITION_RECIPE_SCHEMA_VERSION {
            return Err(recipe_error(
                &self.path,
                format!(
                    "unsupported schema {}; expected {COMPOSITION_RECIPE_SCHEMA_VERSION}",
                    recipe.schema_version
                ),
            ));
        }
        if recipe.variants.is_empty() {
            return Err(recipe_error(&self.path, "defines no variants"));
        }
        if recipe
            .runner()
            .is_some_and(|runner| runner.program().trim().is_empty())
        {
            return Err(recipe_error(&self.path, "Runner program cannot be empty"));
        }
        validate_root_path(&self.path, &recipe.root)?;
        let mut outputs = BTreeSet::new();
        for (name, variant) in &recipe.variants {
            if !valid_variant_name(name) {
                return Err(recipe_error(
                    &self.path,
                    format!("invalid variant name `{name}`"),
                ));
            }
            if variant.fragments.is_empty() {
                return Err(recipe_error(
                    &self.path,
                    format!("variant `{name}` selects no fragments"),
                ));
            }
            for fragment in &variant.fragments {
                validate_relative_path(&self.path, fragment, "fragment")?;
            }
            validate_relative_path(&self.path, &variant.output, "output")?;
            if !outputs.insert(&variant.output) {
                return Err(recipe_error(
                    &self.path,
                    format!("duplicate Plan output `{}`", variant.output),
                ));
            }
        }
        Ok(recipe)
    }

    /// Returns the filesystem root against which fragment contents are interpreted.
    pub fn root(&self, recipe: &CompositionRecipe) -> Result<PathBuf, AuthoringError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let root = parent.join(recipe.root());
        root.canonicalize()
            .map_err(|source| AuthoringError::Io { path: root, source })
    }

    /// Expands one variant into an exact ordinary Project file.
    pub fn materialize(
        &self,
        recipe: &CompositionRecipe,
        name: &str,
    ) -> Result<MaterializedVariant, AuthoringError> {
        self.materialize_without(recipe, name, &[])
    }

    /// Expands one variant after removing explicitly selected fragments.
    pub fn materialize_without(
        &self,
        recipe: &CompositionRecipe,
        name: &str,
        excluded_fragments: &[String],
    ) -> Result<MaterializedVariant, AuthoringError> {
        let variant = recipe
            .variant(name)
            .ok_or_else(|| recipe_error(&self.path, format!("variant `{name}` is not defined")))?;
        for excluded in excluded_fragments {
            if !variant.fragments().contains(excluded) {
                return Err(recipe_error(
                    &self.path,
                    format!("variant `{name}` does not select fragment `{excluded}`"),
                ));
            }
        }
        let root = self.root(recipe)?;
        let mut project = ProjectFile::default();
        let mut merge = MergeState::default();

        for relative in variant.fragments() {
            if excluded_fragments.contains(relative) {
                continue;
            }
            let path = root.join(relative);
            let canonical = path.canonicalize().map_err(|source| AuthoringError::Io {
                path: path.clone(),
                source,
            })?;
            if !canonical.starts_with(&root) {
                return Err(recipe_error(
                    &self.path,
                    format!("fragment `{relative}` escapes the recipe root"),
                ));
            }
            let fragment: ProjectFragment = read_json(&canonical)?;
            merge_fragment(&mut project, &mut merge, fragment, &canonical)?;
        }

        for contract in load_cargo_contracts(&root, &merge.cargo_contracts)? {
            merge_contract(&mut project, &mut merge.contract_keys, contract, &self.path)?;
        }

        Ok(MaterializedVariant {
            name: name.to_owned(),
            output: root.join(variant.output()),
            profile: variant.profile().map(ToOwned::to_owned),
            project,
            root,
        })
    }
}

/// Exact Project file and output selected by one materialized recipe variant.
#[derive(Clone, Debug)]
pub struct MaterializedVariant {
    name: String,
    root: PathBuf,
    output: PathBuf,
    profile: Option<String>,
    project: ProjectFile,
}

impl MaterializedVariant {
    /// Returns the variant name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the Project root used for package, Schema, and contract paths.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the canonical Plan output path.
    pub fn output(&self) -> &Path {
        &self.output
    }

    /// Returns the selected pre-resolution profile, if any.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// Returns the exact ordinary Project file.
    pub fn project(&self) -> &ProjectFile {
        &self.project
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ProjectFragment {
    #[serde(default)]
    composition: CompositionFragment,
    #[serde(default)]
    packages: BTreeMap<String, PackageInput>,
    #[serde(default)]
    contracts: Vec<ContractInput>,
    #[serde(default)]
    cargo_contracts: Vec<String>,
    #[serde(default)]
    profiles: BTreeMap<String, WebProfile>,
}

fn load_cargo_contracts(
    root: &Path,
    packages: &BTreeSet<String>,
) -> Result<Vec<ContractInput>, AuthoringError> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }
    let manifest = root.join("Cargo.toml");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|source| AuthoringError::Io {
            path: manifest.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(recipe_error(
            &manifest,
            format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|source| AuthoringError::Json {
            path: manifest.clone(),
            source,
        })?;
    let metadata_packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| recipe_error(&manifest, "cargo metadata has no packages array"))?;
    let mut contracts = Vec::new();
    for package in packages {
        let matches = metadata_packages
            .iter()
            .filter(|candidate| candidate["name"].as_str() == Some(package))
            .collect::<Vec<_>>();
        let [selected] = matches.as_slice() else {
            return Err(recipe_error(
                &manifest,
                format!(
                    "Cargo contract package `{package}` must resolve exactly once; found {}",
                    matches.len()
                ),
            ));
        };
        let package_manifest = selected["manifest_path"]
            .as_str()
            .ok_or_else(|| recipe_error(&manifest, "Cargo package has no manifest_path"))?;
        let package_root = Path::new(package_manifest)
            .parent()
            .ok_or_else(|| recipe_error(&manifest, "Cargo package manifest has no parent"))?;
        let descriptor_path = package_root.join("capability.json");
        let descriptor =
            lenso_contract_codegen::load_descriptor(&descriptor_path).map_err(|error| {
                recipe_error(
                    &descriptor_path,
                    format!("Cargo contract Descriptor is invalid: {error}"),
                )
            })?;
        let mut contract = ContractInput::descriptor_only(
            descriptor.capability_id(),
            descriptor.version(),
            descriptor_path.to_string_lossy(),
        );
        let rust = package_root.join("src/generated.rs");
        if rust.is_file() {
            contract = contract.with_rust_projection(rust.to_string_lossy());
        }
        let typescript = package_root.join("generated/bindings.ts");
        if typescript.is_file() {
            contract = contract.with_typescript_projection(typescript.to_string_lossy());
        }
        contracts.push(contract);
    }
    Ok(contracts)
}

#[derive(Clone, Debug, Default, Deserialize)]
struct CompositionFragment {
    #[serde(default)]
    modules: Vec<Module>,
    #[serde(default)]
    bindings: Vec<Binding>,
    #[serde(default)]
    execution_lanes: Vec<ExecutionLane>,
}

#[derive(Debug, Default)]
struct MergeState {
    module_keys: BTreeSet<String>,
    binding_keys: BTreeSet<(String, String, String, String)>,
    contract_keys: BTreeSet<(String, String)>,
    cargo_contracts: BTreeSet<String>,
    lane_ids: BTreeSet<String>,
}

fn merge_fragment(
    project: &mut ProjectFile,
    merge: &mut MergeState,
    fragment: ProjectFragment,
    path: &Path,
) -> Result<(), AuthoringError> {
    for module in fragment.composition.modules {
        if !merge.module_keys.insert(module.key().to_owned()) {
            return Err(recipe_error(
                path,
                format!("duplicate Module Instance `{}`", module.key()),
            ));
        }
        project.composition_mut().add_module(module);
    }
    for binding in fragment.composition.bindings {
        let key = (
            binding.consumer().to_owned(),
            binding.capability_id().to_owned(),
            binding.descriptor_version().to_owned(),
            binding.provider().to_owned(),
        );
        if !merge.binding_keys.insert(key) {
            return Err(recipe_error(path, "duplicate Capability binding"));
        }
        project.composition_mut().add_binding(binding);
    }
    for lane in fragment.composition.execution_lanes {
        if merge.lane_ids.insert(lane.id().to_owned()) {
            project.composition_mut().add_execution_lane(lane);
        }
    }
    for (name, package) in fragment.packages {
        if let Some(existing) = project.packages().get(&name) {
            if existing != &package {
                return Err(recipe_error(
                    path,
                    format!("conflicting package input `{name}`"),
                ));
            }
        } else {
            project.packages_mut().insert(name, package);
        }
    }
    for contract in fragment.contracts {
        merge_contract(project, &mut merge.contract_keys, contract, path)?;
    }
    merge.cargo_contracts.extend(fragment.cargo_contracts);
    for (name, profile) in fragment.profiles {
        if let Some(existing) = project.profile(&name) {
            if existing != &profile {
                return Err(recipe_error(path, format!("conflicting profile `{name}`")));
            }
        } else {
            project.profiles_mut().insert(name, profile);
        }
    }
    Ok(())
}

fn merge_contract(
    project: &mut ProjectFile,
    keys: &mut BTreeSet<(String, String)>,
    contract: ContractInput,
    path: &Path,
) -> Result<(), AuthoringError> {
    let key = (
        contract.capability_id().to_owned(),
        contract.descriptor_version().to_owned(),
    );
    if keys.insert(key) {
        project.contracts_mut().push(contract);
        return Ok(());
    }
    if project.contracts().contains(&contract) {
        return Ok(());
    }
    Err(recipe_error(
        path,
        format!(
            "conflicting contract input `{} {}`",
            contract.capability_id(),
            contract.descriptor_version()
        ),
    ))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, AuthoringError> {
    let bytes = fs::read(path).map_err(|source| AuthoringError::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| AuthoringError::Json {
        path: path.to_owned(),
        source,
    })
}

fn validate_relative_path(recipe: &Path, value: &str, kind: &str) -> Result<(), AuthoringError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(recipe_error(
            recipe,
            format!("{kind} path `{value}` must be relative and cannot contain `..`"),
        ));
    }
    Ok(())
}

fn validate_root_path(recipe: &Path, value: &str) -> Result<(), AuthoringError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
    {
        return Err(recipe_error(
            recipe,
            format!("root path `{value}` must be relative"),
        ));
    }
    Ok(())
}

fn valid_variant_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && name.as_bytes()[0].is_ascii_alphanumeric()
}

fn recipe_error(path: &Path, detail: impl Into<String>) -> AuthoringError {
    AuthoringError::Recipe {
        path: path.to_owned(),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn fixture_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lenso-composition-recipe-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("composition")).unwrap();
        fs::create_dir_all(root.join("fragments")).unwrap();
        root
    }

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn fragments_materialize_one_exact_project() {
        let root = fixture_root();
        write(
            &root.join("composition/recipes.json"),
            r#"{
              "schema_version": 1,
              "root": "..",
              "variants": {
                "example": {
                  "fragments": ["fragments/consumer.json", "fragments/provider.json"],
                  "output": "plans/example.json"
                }
              }
            }"#,
        );
        write(
            &root.join("fragments/consumer.json"),
            r#"{
              "composition": { "modules": [{ "key": "consumer", "package": "consumer" }] },
              "packages": {
                "consumer": { "name": "consumer", "source": "cargo", "version": "1.0.0" }
              }
            }"#,
        );
        write(
            &root.join("fragments/provider.json"),
            r#"{
              "composition": { "modules": [{ "key": "provider", "package": "provider" }] },
              "packages": {
                "provider": { "name": "provider", "source": "cargo", "version": "2.0.0" }
              }
            }"#,
        );

        let path = CompositionRecipePath::new(root.join("composition/recipes.json"));
        let recipe = path.load().unwrap();
        let materialized = path.materialize(&recipe, "example").unwrap();

        assert_eq!(materialized.name(), "example");
        let canonical_root = root.canonicalize().unwrap();
        assert_eq!(materialized.root(), canonical_root);
        assert_eq!(
            materialized.output(),
            canonical_root.join("plans/example.json")
        );
        assert_eq!(materialized.project().composition().modules().len(), 2);
        assert_eq!(materialized.project().packages().len(), 2);
        let reduced = path
            .materialize_without(&recipe, "example", &["fragments/provider.json".to_owned()])
            .unwrap();
        assert_eq!(reduced.project().composition().modules().len(), 1);
        assert_eq!(reduced.project().packages().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recipe_exposes_one_structured_product_runner() {
        let root = fixture_root();
        write(
            &root.join("composition/recipes.json"),
            r#"{
              "root": "..",
              "runner": {
                "program": "cargo",
                "args": ["run", "-p", "example-app", "--"],
                "execution_classes": ["lenso.native-rust@1"]
              },
              "variants": {
                "example": {
                  "fragments": ["fragments/app.json"],
                  "output": "plans/example.json"
                }
              }
            }"#,
        );
        write(&root.join("fragments/app.json"), "{}");

        let path = CompositionRecipePath::new(root.join("composition/recipes.json"));
        let recipe = path.load().unwrap();
        let runner = recipe.runner().unwrap();

        assert_eq!(runner.program(), "cargo");
        assert_eq!(runner.args(), ["run", "-p", "example-app", "--"]);
        assert_eq!(runner.execution_classes(), ["lenso.native-rust@1"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recipe_rejects_an_empty_product_runner_program() {
        let root = fixture_root();
        write(
            &root.join("composition/recipes.json"),
            r#"{
              "root": "..",
              "runner": { "program": "", "args": [] },
              "variants": {
                "example": {
                  "fragments": ["fragments/app.json"],
                  "output": "plans/example.json"
                }
              }
            }"#,
        );
        write(&root.join("fragments/app.json"), "{}");

        let path = CompositionRecipePath::new(root.join("composition/recipes.json"));
        let error = path.load().unwrap_err();

        assert!(error.to_string().contains("Runner program cannot be empty"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_module_keys_fail_at_the_recipe_seam() {
        let root = fixture_root();
        write(
            &root.join("composition/recipes.json"),
            r#"{
              "root": "..",
              "variants": {
                "duplicate": {
                  "fragments": ["fragments/first.json", "fragments/second.json"],
                  "output": "plans/duplicate.json"
                }
              }
            }"#,
        );
        for name in ["first", "second"] {
            write(
                &root.join(format!("fragments/{name}.json")),
                r#"{
                  "composition": { "modules": [{ "key": "same", "package": "example" }] },
                  "packages": {
                    "example": { "name": "example", "source": "cargo", "version": "1.0.0" }
                  }
                }"#,
            );
        }

        let path = CompositionRecipePath::new(root.join("composition/recipes.json"));
        let recipe = path.load().unwrap();
        let error = path.materialize(&recipe, "duplicate").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("duplicate Module Instance `same`")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cargo_contracts_use_the_package_owned_descriptor() {
        let root = fixture_root();
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"capability\"]\n",
        );
        fs::create_dir_all(root.join("capability/src")).unwrap();
        write(
            &root.join("capability/Cargo.toml"),
            "[package]\nname = \"example-capability\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        );
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contracts/greeting");
        copy_tree(&fixture, &root.join("capability"));
        write(&root.join("capability/src/lib.rs"), "");
        write(
            &root.join("composition/recipes.json"),
            r#"{
              "root": "..",
              "variants": {
                "contract": {
                  "fragments": ["fragments/contract.json"],
                  "output": "plans/contract.json"
                }
              }
            }"#,
        );
        write(
            &root.join("fragments/contract.json"),
            r#"{ "cargo_contracts": ["example-capability"] }"#,
        );

        let path = CompositionRecipePath::new(root.join("composition/recipes.json"));
        let recipe = path.load().unwrap();
        let materialized = path.materialize(&recipe, "contract").unwrap();

        assert_eq!(materialized.project().contracts().len(), 1);
        assert_eq!(
            materialized.project().contracts()[0].capability_id(),
            "example.greeting@1"
        );
        assert!(
            materialized.project().contracts()[0]
                .descriptor()
                .ends_with("capability/capability.json")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
