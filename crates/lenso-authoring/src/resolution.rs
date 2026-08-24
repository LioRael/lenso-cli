use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use lenso_app_plan::{
    AppComposition, CapabilityBinding as PlanBinding, CapabilityCardinality,
    CapabilityEndpointPlan, CapabilityOperationKind, CapabilityRequirementPlan, EventAdmissionPlan,
    ExecutionClassId, ExecutionLaneId, ExecutionLanePlan, ModuleInstancePlan, RequestAdmissionPlan,
};

use crate::package_manager::{ResolvedPackage, resolve_package};
use crate::validation::validate_configuration;
use crate::{
    AuthoringError, CapabilityEndpoint, Cardinality, CheckOptions, ContractInput, InteractionKind,
    Module, ModuleRole, PROJECT_SCHEMA_VERSION, PackageSource, ProjectFile, ResolutionOptions,
    ResolvedProject, canonical_json_bytes, canonical_json_string,
};
use lenso_contract_codegen::ProjectionLanguage;

const UI_CONTRIBUTION_CAPABILITY_ID: &str = "lenso.ui.contribution@1";
const WEB_SHELL_CAPABILITY_ID: &str = "lenso.web.shell@1";

#[derive(Clone, Debug)]
struct ContractFacts {
    operations: BTreeSet<String>,
    cross_lane_transfer: bool,
}

type ContractFactsByIdentity = BTreeMap<(String, String), ContractFacts>;

fn check_owned_projections(
    contract: &ContractInput,
    root: &Path,
    descriptor: &Path,
) -> Result<(), AuthoringError> {
    for (language, projection) in [
        (ProjectionLanguage::Rust, contract.rust_projection()),
        (
            ProjectionLanguage::TypeScript,
            contract.typescript_projection(),
        ),
    ] {
        let Some(projection) = projection else {
            continue;
        };
        lenso_contract_codegen::check_projection(descriptor, language, &root.join(projection))
            .map_err(|error| AuthoringError::Contract {
                path: descriptor.to_owned(),
                detail: error.to_string(),
            })?;
    }
    Ok(())
}

/// Authoring operations implemented above the pure App Plan data model.
pub trait ProjectAuthoring {
    fn check(&self, root: &Path, options: &CheckOptions) -> Result<CheckReport, AuthoringError>;

    fn resolve(
        &self,
        root: &Path,
        options: &ResolutionOptions,
    ) -> Result<ResolvedProject, AuthoringError>;
}

impl ProjectAuthoring for ProjectFile {
    /// Checks project data, package locks, generated artifacts, configuration,
    /// execution classes, and explicit Capability bindings.
    fn check(&self, root: &Path, options: &CheckOptions) -> Result<CheckReport, AuthoringError> {
        validate_schema(self)?;
        let contracts = check_contracts(self, root)?;
        let modules = selected_modules(self, None)?;
        let packages = check_packages(self, root, &modules, options)?;
        let composition = build_composition(self, &modules, &packages, &contracts)?;
        composition
            .resolve()
            .map_err(|error| AuthoringError::Plan {
                detail: error.to_string(),
            })?;
        Ok(CheckReport {
            modules: modules.len(),
            bindings: self.composition().bindings().len(),
            contracts: self.contracts().len(),
            execution_classes: options.available_execution_classes().clone(),
        })
    }

    /// Resolves one deterministic immutable Plan from Composition and lock state.
    fn resolve(
        &self,
        root: &Path,
        options: &ResolutionOptions,
    ) -> Result<ResolvedProject, AuthoringError> {
        validate_schema(self)?;
        let contracts = check_contracts(self, root)?;
        let modules = selected_modules(self, options.profile())?;
        let packages = check_packages(self, root, &modules, options.check())?;
        let composition = build_composition(self, &modules, &packages, &contracts)?;
        let plan = composition
            .resolve()
            .map_err(|error| AuthoringError::Plan {
                detail: error.to_string(),
            })?;
        let canonical_bytes = canonical_json_bytes(&plan);
        Ok(ResolvedProject {
            plan,
            canonical_bytes,
        })
    }
}

fn validate_schema(project: &ProjectFile) -> Result<(), AuthoringError> {
    if project.schema_version() != PROJECT_SCHEMA_VERSION {
        return Err(AuthoringError::UnsupportedProjectSchema {
            actual: project.schema_version(),
        });
    }
    Ok(())
}

fn check_contracts(
    project: &ProjectFile,
    root: &Path,
) -> Result<ContractFactsByIdentity, AuthoringError> {
    let mut contracts = BTreeMap::new();
    for contract in project.contracts() {
        let descriptor = root.join(contract.descriptor());
        let loaded = lenso_contract_codegen::load_descriptor(&descriptor).map_err(|error| {
            AuthoringError::Contract {
                path: descriptor.clone(),
                detail: error.to_string(),
            }
        })?;
        if loaded.capability_id() != contract.capability_id()
            || loaded.version() != contract.descriptor_version()
        {
            return Err(AuthoringError::Contract {
                path: descriptor,
                detail: format!(
                    "declared {} {} but Descriptor is {} {}",
                    contract.capability_id(),
                    contract.descriptor_version(),
                    loaded.capability_id(),
                    loaded.version()
                ),
            });
        }
        check_owned_projections(contract, root, &descriptor)?;
        let descriptor_operations = loaded
            .operation_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        if contracts
            .insert(
                (
                    contract.capability_id().to_owned(),
                    contract.descriptor_version().to_owned(),
                ),
                ContractFacts {
                    operations: descriptor_operations,
                    cross_lane_transfer: loaded.cross_lane_transfer(),
                },
            )
            .is_some()
        {
            return Err(AuthoringError::Contract {
                path: descriptor,
                detail: "duplicate Capability Descriptor input".to_owned(),
            });
        }
    }
    for module in project.composition().modules() {
        for (capability_id, descriptor_version) in module
            .provides()
            .iter()
            .map(|endpoint| (endpoint.capability_id(), endpoint.descriptor_version()))
            .chain(module.requires().iter().map(|requirement| {
                (
                    requirement.capability_id(),
                    requirement.descriptor_version(),
                )
            }))
        {
            if !contracts.contains_key(&(capability_id.to_owned(), descriptor_version.to_owned())) {
                return Err(AuthoringError::Contract {
                    path: root.to_owned(),
                    detail: format!(
                        "Module Instance {} uses {capability_id} {descriptor_version} without a Descriptor input",
                        module.key()
                    ),
                });
            }
        }
        for endpoint in module.provides() {
            let expected = &contracts[&(
                endpoint.capability_id().to_owned(),
                endpoint.descriptor_version().to_owned(),
            )]
                .operations;
            let actual = endpoint.operations().iter().cloned().collect();
            if expected != &actual {
                return Err(AuthoringError::Contract {
                    path: root.to_owned(),
                    detail: format!(
                        "Module Instance {} endpoint {} operations do not match Descriptor {}",
                        module.key(),
                        endpoint.capability_id(),
                        endpoint.descriptor_version()
                    ),
                });
            }
        }
    }
    Ok(contracts)
}

fn selected_modules(
    project: &ProjectFile,
    profile: Option<&str>,
) -> Result<Vec<Module>, AuthoringError> {
    let mut modules = project.composition().modules().to_vec();
    let Some(profile_name) = profile else {
        return Ok(modules);
    };
    let Some(profile) = project.profile(profile_name) else {
        return Err(AuthoringError::InvalidProfile {
            profile: profile_name.to_owned(),
            detail: "profile is not defined".to_owned(),
        });
    };
    let selected: BTreeSet<_> = profile.selected_modules().collect();
    if profile.shell() == profile.browser_adapter() {
        return Err(AuthoringError::InvalidProfile {
            profile: profile_name.to_owned(),
            detail: "Web Shell and Browser Adapter must be different Module Instances".to_owned(),
        });
    }
    for key in &selected {
        if !modules.iter().any(|module| module.key() == *key) {
            return Err(AuthoringError::InvalidProfile {
                profile: profile_name.to_owned(),
                detail: format!("unknown Module Instance {key}"),
            });
        }
    }
    modules.retain(|module| selected.contains(module.key()));
    if modules.is_empty() {
        return Err(AuthoringError::InvalidProfile {
            profile: profile_name.to_owned(),
            detail: "profile selects no Module Instances".to_owned(),
        });
    }
    validate_profile_role(
        profile_name,
        &modules,
        profile.shell(),
        ModuleRole::WebShell,
    )?;
    validate_profile_role(
        profile_name,
        &modules,
        profile.browser_adapter(),
        ModuleRole::BrowserAdapter,
    )?;
    for contribution in profile.ui_contributions() {
        validate_profile_role(
            profile_name,
            &modules,
            contribution,
            ModuleRole::UiContribution,
        )?;
    }
    validate_web_profile_interfaces(
        profile_name,
        &modules,
        project.composition().bindings(),
        profile,
    )?;
    Ok(modules)
}

fn validate_web_profile_interfaces(
    profile_name: &str,
    modules: &[Module],
    bindings: &[crate::Binding],
    profile: &lenso_app_plan::authoring::WebProfile,
) -> Result<(), AuthoringError> {
    let module = |instance: &str| {
        modules
            .iter()
            .find(|module| module.key() == instance)
            .expect("profile Instance existence was validated")
    };
    if !module(profile.shell())
        .requires()
        .iter()
        .any(|requirement| {
            requirement.capability_id() == UI_CONTRIBUTION_CAPABILITY_ID
                && requirement.cardinality() == Cardinality::Many
        })
    {
        return Err(AuthoringError::InvalidProfile {
            profile: profile_name.to_owned(),
            detail: format!(
                "Web Shell {} must require many {UI_CONTRIBUTION_CAPABILITY_ID}",
                profile.shell()
            ),
        });
    }
    if !module(profile.browser_adapter())
        .requires()
        .iter()
        .any(|requirement| {
            requirement.capability_id() == WEB_SHELL_CAPABILITY_ID
                && requirement.cardinality() == Cardinality::One
        })
    {
        return Err(AuthoringError::InvalidProfile {
            profile: profile_name.to_owned(),
            detail: format!(
                "Browser Adapter {} must require one {WEB_SHELL_CAPABILITY_ID}",
                profile.browser_adapter()
            ),
        });
    }
    for contribution in profile.ui_contributions() {
        if !module(contribution)
            .provides()
            .iter()
            .any(|endpoint| endpoint.capability_id() == UI_CONTRIBUTION_CAPABILITY_ID)
        {
            return Err(AuthoringError::InvalidProfile {
                profile: profile_name.to_owned(),
                detail: format!(
                    "UI Contribution {contribution} must provide {UI_CONTRIBUTION_CAPABILITY_ID}"
                ),
            });
        }
        validate_projected_requirements(
            profile_name,
            module(contribution),
            module(profile.browser_adapter()),
            bindings,
        )?;
    }
    Ok(())
}

fn validate_projected_requirements(
    profile_name: &str,
    contribution: &Module,
    browser: &Module,
    bindings: &[crate::Binding],
) -> Result<(), AuthoringError> {
    for requirement in contribution.requires() {
        let capability_id = requirement.capability_id();
        let descriptor_version = requirement.descriptor_version();
        if requirement.cardinality() != Cardinality::One {
            return Err(AuthoringError::InvalidProfile {
                profile: profile_name.to_owned(),
                detail: format!(
                    "UI Contribution {} requirement {capability_id} {descriptor_version} must be exactly-one for v1 browser projection",
                    contribution.key()
                ),
            });
        }
        let mirrored = browser.requires().iter().any(|candidate| {
            candidate.capability_id() == capability_id
                && candidate.descriptor_version() == descriptor_version
                && candidate.cardinality() == requirement.cardinality()
        });
        let providers_for = |consumer: &str| {
            bindings
                .iter()
                .filter(|binding| {
                    binding.consumer() == consumer
                        && binding.capability_id() == capability_id
                        && binding.descriptor_version() == descriptor_version
                })
                .map(crate::Binding::provider)
                .collect::<BTreeSet<_>>()
        };
        let contribution_providers = providers_for(contribution.key());
        let browser_providers = providers_for(browser.key());
        let same_provider =
            contribution_providers.len() == 1 && contribution_providers == browser_providers;
        if !mirrored || !same_provider {
            return Err(AuthoringError::InvalidProfile {
                profile: profile_name.to_owned(),
                detail: format!(
                    "Browser Adapter {} must project UI Contribution {} requirement {capability_id} {descriptor_version} with the same cardinality and resolved provider",
                    browser.key(),
                    contribution.key()
                ),
            });
        }
    }
    Ok(())
}

fn check_packages(
    project: &ProjectFile,
    root: &Path,
    modules: &[Module],
    options: &CheckOptions,
) -> Result<BTreeMap<String, ResolvedPackage>, AuthoringError> {
    let mut resolved = BTreeMap::new();
    for module in modules {
        let Some(input) = project.packages().get(module.package()) else {
            return Err(AuthoringError::MissingPackageInput {
                package: module.package().to_owned(),
            });
        };
        if input.name() != module.package() {
            return Err(AuthoringError::LockMismatch {
                package: module.package().to_owned(),
                detail: "package map key and package identity disagree".to_owned(),
            });
        }
        let locked = resolve_package(root, input)?;
        let execution_class = module
            .execution_class()
            .or_else(|| input.source().default_execution_class())
            .ok_or_else(|| AuthoringError::UnavailableExecutionClass {
                instance: module.key().to_owned(),
                execution_class: "<module-selected>".to_owned(),
            })?;
        if !options
            .available_execution_classes()
            .contains(execution_class)
        {
            return Err(AuthoringError::UnavailableExecutionClass {
                instance: module.key().to_owned(),
                execution_class: execution_class.to_owned(),
            });
        }
        if matches!(input.source(), PackageSource::Bun | PackageSource::Npm)
            && module.entrypoint() == "default"
        {
            return Err(AuthoringError::MissingEntrypoint {
                instance: module.key().to_owned(),
            });
        }
        if matches!(input.source(), PackageSource::Bun | PackageSource::Npm) {
            validate_entrypoint(root, module)?;
        }
        validate_configuration(root, module)?;
        resolved.insert(module.package().to_owned(), locked);
    }
    Ok(resolved)
}

fn validate_profile_role(
    profile: &str,
    modules: &[Module],
    instance: &str,
    expected: ModuleRole,
) -> Result<(), AuthoringError> {
    let module = modules
        .iter()
        .find(|module| module.key() == instance)
        .expect("selected profile instances were checked above");
    if module.role() != Some(expected) {
        return Err(AuthoringError::InvalidProfile {
            profile: profile.to_owned(),
            detail: format!(
                "Module Instance {instance} must declare role {}",
                match expected {
                    ModuleRole::WebShell => "web_shell",
                    ModuleRole::BrowserAdapter => "browser_adapter",
                    ModuleRole::UiContribution => "ui_contribution",
                }
            ),
        });
    }
    Ok(())
}

fn validate_entrypoint(root: &Path, module: &Module) -> Result<(), AuthoringError> {
    let entrypoint = root.join(module.entrypoint());
    if !entrypoint.is_file() {
        return Err(AuthoringError::PackageManager {
            package: module.package().to_owned(),
            detail: format!("Bun entrypoint {} does not exist", entrypoint.display()),
        });
    }
    Ok(())
}

fn build_composition(
    project: &ProjectFile,
    modules: &[Module],
    packages: &BTreeMap<String, ResolvedPackage>,
    contracts: &ContractFactsByIdentity,
) -> Result<AppComposition, AuthoringError> {
    let selected: BTreeSet<_> = modules.iter().map(Module::key).collect();
    let all_keys: BTreeSet<_> = project
        .composition()
        .modules()
        .iter()
        .map(Module::key)
        .collect();
    let mut instances = Vec::with_capacity(modules.len());
    for module in modules {
        let input = project.packages().get(module.package()).ok_or_else(|| {
            AuthoringError::MissingPackageInput {
                package: module.package().to_owned(),
            }
        })?;
        let locked =
            packages
                .get(module.package())
                .ok_or_else(|| AuthoringError::PackageManager {
                    package: module.package().to_owned(),
                    detail: "package was not resolved".to_owned(),
                })?;
        let execution_class = module
            .execution_class()
            .or_else(|| input.source().default_execution_class())
            .ok_or_else(|| AuthoringError::UnavailableExecutionClass {
                instance: module.key().to_owned(),
                execution_class: "<module-selected>".to_owned(),
            })?;
        let mut instance = ModuleInstancePlan::new(module.key(), module.package())
            .with_entrypoint(module.entrypoint())
            .with_configuration(canonical_json_string(module.configuration()))
            .with_execution_class(ExecutionClassId::new(execution_class))
            .with_execution_lane(ExecutionLaneId::new(
                module.execution_lane().unwrap_or("main"),
            ))
            .with_package_revision(locked.revision());
        for endpoint in module.provides() {
            let facts = &contracts[&(
                endpoint.capability_id().to_owned(),
                endpoint.descriptor_version().to_owned(),
            )];
            instance =
                instance.with_capability(to_plan_endpoint(endpoint, facts.cross_lane_transfer));
        }
        for requirement in module.requires() {
            instance = instance.with_requirement(CapabilityRequirementPlan::new(
                requirement.capability_id(),
                requirement.descriptor_version(),
                to_plan_cardinality(requirement.cardinality()),
            ));
        }
        instances.push(instance);
    }
    let mut bindings = Vec::new();
    for binding in project.composition().bindings() {
        if !all_keys.contains(binding.consumer()) || !all_keys.contains(binding.provider()) {
            return Err(AuthoringError::Plan {
                detail: format!(
                    "binding {} -> {} references an unknown Module Instance",
                    binding.consumer(),
                    binding.provider()
                ),
            });
        }
        if selected.contains(binding.consumer()) && selected.contains(binding.provider()) {
            let mut plan_binding = PlanBinding::new(
                binding.consumer(),
                binding.capability_id(),
                binding.descriptor_version(),
                binding.provider(),
            );
            if let Some(admission) = binding.admission() {
                plan_binding = plan_binding.with_admission(RequestAdmissionPlan::new(
                    admission.queue_capacity(),
                    admission.max_concurrency(),
                ));
            }
            if let Some(capacity) = binding.event_capacity() {
                plan_binding = plan_binding.with_event_admission(EventAdmissionPlan::new(capacity));
            }
            bindings.push(plan_binding);
        }
    }
    Ok(
        AppComposition::new(instances, bindings).with_execution_lanes(
            project
                .composition()
                .execution_lanes()
                .iter()
                .map(|lane| ExecutionLanePlan::new(lane.id()))
                .collect(),
        ),
    )
}

fn to_plan_cardinality(cardinality: Cardinality) -> CapabilityCardinality {
    match cardinality {
        Cardinality::One => CapabilityCardinality::One,
        Cardinality::Optional => CapabilityCardinality::Optional,
        Cardinality::Many => CapabilityCardinality::Many,
    }
}

fn to_plan_endpoint(
    endpoint: &CapabilityEndpoint,
    cross_lane_transfer: bool,
) -> CapabilityEndpointPlan {
    let mut plan = CapabilityEndpointPlan::new(
        endpoint.capability_id(),
        endpoint.descriptor_version(),
        endpoint.operations(),
    );
    for (operation, kind) in endpoint.operation_kinds() {
        let kind = match kind {
            InteractionKind::Request => CapabilityOperationKind::Request,
            InteractionKind::Stream => CapabilityOperationKind::Stream,
            InteractionKind::Event => CapabilityOperationKind::Event,
        };
        plan = plan.with_operation_kind(operation, kind);
    }
    if let Some(admission) = endpoint.admission() {
        plan = plan.with_admission(RequestAdmissionPlan::new(
            admission.queue_capacity(),
            admission.max_concurrency(),
        ));
    }
    for (operation, admission) in endpoint.operation_admissions() {
        plan = plan.with_operation_admission(
            operation,
            RequestAdmissionPlan::new(admission.queue_capacity(), admission.max_concurrency()),
        );
    }
    if let Some(capacity) = endpoint.event_capacity() {
        plan = plan.with_event_admission(EventAdmissionPlan::new(capacity));
    }
    if cross_lane_transfer {
        plan = plan.with_cross_lane_transfer();
    }
    plan
}

/// A successful authoring check summary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckReport {
    /// Number of Module Instances checked.
    pub modules: usize,
    /// Number of explicit bindings checked.
    pub bindings: usize,
    /// Number of generated contract inputs checked.
    pub contracts: usize,
    /// Available host Execution Adapter classes.
    pub execution_classes: BTreeSet<String>,
}
