use std::{collections::BTreeSet, path::PathBuf};

use crate::{Module, PackageInput};

#[derive(Clone, Debug)]
pub struct AddModule {
    module: Module,
    package: PackageInput,
}

impl AddModule {
    /// Creates one add request.
    pub fn new(module: Module, package: PackageInput) -> Self {
        Self { module, package }
    }
    /// Returns the Module to add.
    pub fn module(&self) -> &Module {
        &self.module
    }
    /// Returns the package input to add.
    pub fn package(&self) -> &PackageInput {
        &self.package
    }
}

/// Authoring checks that depend on the host's installed Execution Adapters.
#[derive(Clone, Debug)]
pub struct CheckOptions {
    available_execution_classes: BTreeSet<String>,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            available_execution_classes: ["lenso.native-rust@1".to_owned()].into_iter().collect(),
        }
    }
}

impl CheckOptions {
    /// Creates checks for an explicit host Adapter set.
    pub fn new(classes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            available_execution_classes: classes.into_iter().map(Into::into).collect(),
        }
    }
    /// Adds one available Execution Adapter class.
    #[must_use]
    pub fn with_execution_class(mut self, class: impl Into<String>) -> Self {
        self.available_execution_classes.insert(class.into());
        self
    }
    /// Replaces the available Execution Adapter classes while preserving other checks.
    #[must_use]
    pub fn with_available_execution_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.available_execution_classes = classes.into_iter().map(Into::into).collect();
        self
    }
    /// Returns available Execution Adapter classes.
    pub fn available_execution_classes(&self) -> &BTreeSet<String> {
        &self.available_execution_classes
    }
}

/// Resolution options, including a selected authoring profile.
#[derive(Clone, Debug, Default)]
pub struct ResolutionOptions {
    profile: Option<String>,
    check: CheckOptions,
}

impl ResolutionOptions {
    /// Selects one profile to materialize before Plan resolution.
    #[must_use]
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }
    /// Supplies host Execution Adapter availability checks.
    #[must_use]
    pub fn with_check_options(mut self, check: CheckOptions) -> Self {
        self.check = check;
        self
    }
    /// Returns the selected profile, if any.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }
    /// Returns the host check options.
    pub fn check(&self) -> &CheckOptions {
        &self.check
    }
}

/// Result of materializing an immutable Plan.
#[derive(Clone, Debug)]
pub struct ResolvedProject {
    pub(crate) plan: lenso_app_plan::ResolvedAppPlan,
    pub(crate) canonical_bytes: Vec<u8>,
}

impl ResolvedProject {
    /// Returns the typed immutable Plan passed to Kernel.
    pub fn plan(&self) -> &lenso_app_plan::ResolvedAppPlan {
        &self.plan
    }
    /// Returns byte-stable canonical Plan bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
    /// Returns the SHA-256 fingerprint of the canonical Plan document.
    pub fn fingerprint(&self) -> String {
        crate::sha256_bytes(&self.canonical_bytes)
    }

    /// Decodes and validates one canonical Resolved App Plan file.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, crate::AuthoringError> {
        let plan: lenso_app_plan::ResolvedAppPlan = serde_json::from_slice(bytes)
            .map_err(|source| crate::AuthoringError::PlanJson { source })?;
        plan.validate()
            .map_err(|error| crate::AuthoringError::Plan {
                detail: error.to_string(),
            })?;
        let normalized = lenso_app_plan::ResolvedAppPlan::new(
            plan.module_instances().to_vec(),
            plan.capability_bindings().to_vec(),
        );
        if normalized != plan {
            return Err(crate::AuthoringError::NonCanonicalPlan);
        }
        let canonical_bytes = crate::canonical_json_bytes(&plan);
        if canonical_bytes != bytes {
            return Err(crate::AuthoringError::NonCanonicalPlan);
        }
        Ok(Self {
            plan,
            canonical_bytes,
        })
    }
}

/// A path relative to a project document, used by the CLI and add workflow.
#[derive(Clone, Debug)]
pub struct ProjectPath {
    path: PathBuf,
}

impl ProjectPath {
    /// Creates a project handle for one JSON project document.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    /// Returns the project document path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}
