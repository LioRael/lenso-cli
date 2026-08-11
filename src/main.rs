mod app_composition;
mod capability;
mod console_composition;
mod console_dev;
mod console_installation;
mod console_operator;
mod delivery;
mod ga;
mod host;
// Retired app-lifecycle internals stay private so existing generated state can still be read.
#[allow(dead_code)]
mod launchpad;
mod module;
mod operator;
mod service;
// Retired System mutation helpers remain internal while the public CLI exposes only dev/check.
#[allow(dead_code)]
mod system;
mod system_sandbox;
mod workload_control_contract;

use clap::{Args, Parser, Subcommand};

/// Lenso command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "lenso",
    version,
    about = "Scaffold and operate Lenso backend projects",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start a Lenso host project locally.
    Serve(ServeArgs),
    /// Compose Lenso applications.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Inspect local application status.
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },
    /// Emit concise context for coding agents.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Author and inspect local reusable capability packs.
    Capability {
        #[command(subcommand)]
        command: CapabilityCommand,
    },
    /// Scaffold and manage Lenso host applications.
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    /// Create and manage Lenso modules.
    Module {
        #[command(subcommand)]
        command: ModuleCommand,
    },
    /// Install, diagnose, and operate Lenso services.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Manage the Lenso Kubernetes Operator.
    Operator {
        #[command(subcommand)]
        command: OperatorCommand,
    },
    /// Run and validate a local Lenso System.
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },
    /// Manage the independent Lenso Console Service.
    Console {
        #[command(subcommand)]
        command: ConsoleCommand,
    },
    /// Evaluate and operate the bounded M6 General Availability support surface.
    Ga {
        #[command(subcommand)]
        command: GaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum GaCommand {
    /// Evaluate an exact installed or proposed component set against the support manifest.
    SupportCheck(GaSupportCheckArgs),
    /// Dry-run or apply an identity-preserving Service or System manifest migration.
    ManifestMigrate(GaManifestMigrateArgs),
    /// Produce the migration-first multi-Workload Service upgrade plan.
    ServiceUpgrade(GaServiceUpgradeArgs),
    /// Plan or apply stale-safe Contract Retirement.
    ContractRetire(GaContractRetireArgs),
    /// Evaluate one versioned Failure Scenario evidence input.
    FailureEvaluate(GaFailureEvaluateArgs),
}

#[derive(Debug, Args, Clone)]
struct GaSupportCheckArgs {
    #[arg(long)]
    manifest: std::path::PathBuf,
    #[arg(long = "component", required = true)]
    components: Vec<String>,
    #[arg(long)]
    state_version: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct GaManifestMigrateArgs {
    #[arg(long)]
    manifest: std::path::PathBuf,
    #[arg(long)]
    source: std::path::PathBuf,
    #[arg(long)]
    target_format: String,
    #[arg(long = "identity-pointer")]
    identity_pointers: Vec<String>,
    #[arg(long)]
    target: Option<std::path::PathBuf>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct GaServiceUpgradeArgs {
    #[arg(long)]
    manifest: std::path::PathBuf,
    #[arg(long)]
    input: std::path::PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct GaContractRetireArgs {
    #[arg(long)]
    input: std::path::PathBuf,
    #[arg(long)]
    approval: Option<std::path::PathBuf>,
    #[arg(long)]
    output: Option<std::path::PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct GaFailureEvaluateArgs {
    #[arg(long)]
    input: std::path::PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum AppCommand {
    /// Create a Lenso application from a product blueprint.
    #[command(hide = true)]
    Create(AppCreateArgs),
    /// List built-in product blueprints.
    #[command(hide = true)]
    List,
    /// Inspect a built-in product blueprint.
    #[command(hide = true)]
    Inspect(AppInspectArgs),
    /// Add a built-in extension to the current application.
    #[command(hide = true)]
    Add(AppAddArgs),
    /// Compose an exact Lenso App Composition.
    Compose(AppComposeArgs),
}

#[derive(Debug, Args, Clone)]
struct AppCreateArgs {
    /// Directory that receives the generated application.
    dir: std::path::PathBuf,

    /// Product blueprint name.
    #[arg(long, default_value = "support-desk")]
    blueprint: String,

    /// Replace an existing host directory.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args, Clone)]
struct AppInspectArgs {
    /// Blueprint name.
    blueprint: String,
}

#[derive(Debug, Args, Clone)]
struct AppAddArgs {
    /// Addon name.
    addon: String,

    /// Expected App Composition revision for the atomic update.
    #[arg(long)]
    observed_revision: Option<u64>,
}

#[derive(Debug, Args, Clone)]
struct AppComposeArgs {
    /// New app directory. Omit when composing an existing app with --repo-root.
    dir: Option<std::path::PathBuf>,

    /// Existing Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Product blueprint name for new apps.
    #[arg(long, default_value = "support-desk")]
    blueprint: String,

    /// Local capability pack to compose into the app. Can be repeated.
    #[arg(long = "pack")]
    packs: Vec<std::path::PathBuf>,

    /// Atomically materialize the exact App Composition.
    #[arg(long)]
    apply: bool,

    /// Override a Module implementation. Can be repeated as MODULE=linked or MODULE=service:REF.
    #[arg(long = "implementation", value_name = "MODULE=linked|service:REF")]
    implementations: Vec<String>,

    /// Expected App Composition revision for the atomic update.
    #[arg(long)]
    observed_revision: Option<u64>,
}

#[derive(Debug, Subcommand)]
enum DevCommand {
    /// Start services and host for local development.
    #[command(hide = true)]
    Up(DevUpArgs),
    /// Inspect local application status.
    Status(DevStatusArgs),
    /// Diagnose local development state.
    #[command(hide = true)]
    Doctor(DevDoctorArgs),
    /// Explain how to stop the foreground dev process.
    #[command(hide = true)]
    Stop,
}

#[derive(Debug, Args, Clone)]
struct DevUpArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Do not start service workspace entries.
    #[arg(long)]
    no_workspace: bool,

    /// Do not start the template Postgres service.
    #[arg(long)]
    skip_db: bool,

    /// Do not run migrations before starting services.
    #[arg(long)]
    skip_migrate: bool,

    /// Run API and worker as separate local processes.
    #[arg(long)]
    separate_worker: bool,
}

#[derive(Debug, Args, Clone)]
struct DevStatusArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct DevDoctorArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Probe service ready URLs in addition to static checks.
    #[arg(long)]
    live: bool,

    /// Write .lenso/dev-doctor.json.
    #[arg(long)]
    write_state: bool,
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// Print application, system, and workspace context for an agent.
    Context(AgentContextArgs),
    /// Print agent context with a task appended.
    Task(AgentTaskArgs),
}

#[derive(Debug, Args, Clone)]
struct AgentContextArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Write the context to a file instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Scope handoff to one module when known.
    #[arg(long = "for-module")]
    for_module: Option<String>,

    /// Scope handoff to one capability pack when known.
    #[arg(long = "for-capability")]
    for_capability: Option<String>,
}

#[derive(Debug, Args, Clone)]
struct AgentTaskArgs {
    /// Task text to append to the generated context.
    task: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Write the context to a file instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Scope handoff to one module when known.
    #[arg(long = "for-module")]
    for_module: Option<String>,

    /// Scope handoff to one capability pack when known.
    #[arg(long = "for-capability")]
    for_capability: Option<String>,
}

#[derive(Debug, Subcommand)]
enum CapabilityCommand {
    /// Create a local capability pack.
    Init(CapabilityInitArgs),
    /// Validate a local capability pack.
    Check(CapabilityCheckArgs),
    /// Inspect a local capability pack.
    Inspect(CapabilityInspectArgs),
    /// Manage the local capability pack library.
    Library {
        #[command(subcommand)]
        command: CapabilityLibraryCommand,
    },
    /// Check whether a capability pack fits the current App Composition.
    Fit(CapabilityFitArgs),
}

#[derive(Debug, Subcommand)]
enum CapabilityLibraryCommand {
    /// Create .lenso/lenso.capability-library.json.
    Init(CapabilityLibraryInitArgs),
    /// Add a local capability pack to the library.
    Add(CapabilityLibraryAddArgs),
    /// List local capability packs from the library.
    List(CapabilityLibraryListArgs),
    /// Check every pack recorded in the library.
    Check(CapabilityLibraryCheckArgs),
}

#[derive(Debug, Args, Clone)]
struct CapabilityInitArgs {
    /// Capability pack name.
    name: String,

    /// Directory that receives the capability pack.
    #[arg(long)]
    dir: std::path::PathBuf,

    /// Service SDK language.
    #[arg(long, default_value = "ts")]
    lang: String,

    /// Supported product blueprint. Can be repeated.
    #[arg(long = "for-blueprint")]
    for_blueprint: Vec<String>,
}

#[derive(Debug, Args, Clone)]
struct CapabilityCheckArgs {
    /// Capability pack directory or lenso.capability.json path.
    path: std::path::PathBuf,

    /// Print JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct CapabilityInspectArgs {
    /// Capability pack directory or lenso.capability.json path.
    path: std::path::PathBuf,
}

#[derive(Debug, Args, Clone)]
struct CapabilityLibraryInitArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct CapabilityLibraryAddArgs {
    /// Capability pack directory or lenso.capability.json path.
    path: std::path::PathBuf,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct CapabilityLibraryListArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct CapabilityLibraryCheckArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct CapabilityFitArgs {
    /// Capability pack name from the library, directory, or manifest path.
    pack: std::path::PathBuf,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum SystemCommand {
    /// Launch a clusterless local multi-Service System Sandbox.
    Dev(SystemDevArgs),
    /// Validate the service system graph.
    Check(SystemCheckArgs),
}

#[derive(Debug, Args, Clone)]
struct SystemDevArgs {
    /// Service System v2 definition.
    #[arg(long)]
    system_file: Option<std::path::PathBuf>,

    /// Local Workload launch declarations.
    #[arg(long)]
    sandbox_file: Option<std::path::PathBuf>,

    /// Run one declared Failure Scenario and exit after deterministic cleanup.
    #[arg(long, value_name = "SCENARIO_ID")]
    scenario: Option<String>,

    /// Validate and print the exact local launch preview without allocating or starting anything.
    #[arg(long)]
    dry_run: bool,

    /// Stop the recorded sandbox and remove only its owned resources.
    #[arg(long)]
    cleanup: bool,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,

    /// Internal Local Control Adapter worker mode.
    #[arg(long, hide = true)]
    adapter_child: bool,
}

#[derive(Debug, Args, Clone)]
struct SystemCheckArgs {
    /// Service system file.
    #[arg(long)]
    system_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum OperatorCommand {
    /// Export the Lenso Kubernetes Operator install bundle.
    ExportCrd(OperatorExportCrdArgs),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct OperatorExportCrdArgs {
    /// Output directory for CRD, RBAC, deployment, kustomization, and README.
    #[arg(long)]
    output: std::path::PathBuf,

    /// Operator image to put in deployment.yaml.
    #[arg(long, default_value = "ghcr.io/lenso-dev/lenso-operator:latest")]
    image: String,

    /// Namespace for operator install resources.
    #[arg(long, default_value = "lenso-system")]
    namespace: String,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ServeArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Run API and worker as separate local processes.
    #[arg(long)]
    separate_worker: bool,

    /// Do not start the template Postgres service.
    #[arg(long)]
    skip_db: bool,

    /// Do not run migrations before starting services.
    #[arg(long)]
    skip_migrate: bool,
}

#[derive(Debug, Subcommand)]
enum HostCommand {
    /// Create a new Lenso host application in a target directory.
    Init {
        /// Target directory for the new project.
        dir: String,

        /// Package name for the generated Cargo crate.
        ///
        /// Defaults to the target directory name. Must be a valid Cargo crate name.
        #[arg(long)]
        name: Option<String>,

        /// Allow scaffolding into a non-empty directory.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args, Clone)]
struct ConsoleOperatorBootstrapArgs {
    /// Lenso Console repository root. Defaults to the current directory.
    #[arg(long)]
    console_root: Option<std::path::PathBuf>,

    /// Console Service URL used to create the first password user.
    ///
    /// When no password input option is supplied, an interactive terminal
    /// prompts for and confirms the password without echoing it.
    #[arg(long)]
    console_url: Option<String>,

    /// Environment file to read for `DATABASE_URL`.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Auth user id, such as `usr_abc`.
    #[arg(long)]
    user_id: Option<String>,

    /// Password-auth identifier, such as an email address.
    #[arg(long)]
    identifier: Option<String>,

    /// Read the new password from a private regular file instead of prompting.
    #[arg(long, conflicts_with = "password_stdin")]
    password_file: Option<std::path::PathBuf>,

    /// Read the new password from standard input instead of prompting.
    #[arg(long, conflicts_with = "password_file")]
    password_stdin: bool,

    /// Additional scope to grant beyond the Console Minimum operator scopes.
    #[arg(long = "scope")]
    scopes: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum ConsoleOperatorCommand {
    /// Bootstrap the first operator in an independent Lenso Console Service.
    Bootstrap(ConsoleOperatorBootstrapArgs),
}

#[derive(Debug, Subcommand)]
enum ConsoleRecoveryCommand {
    /// Validate and record reviewed post-restore reconciliation evidence.
    Reconcile(ConsoleReconcileArgs),
    /// Activate a reconciled Console recovery with explicit authority transfer.
    Activate(ConsoleActivateArgs),
    /// Re-establish recovery-mode authority after an interrupted activation.
    RecoverActivation(ConsoleRecoverActivationArgs),
}

#[derive(Debug, Subcommand)]
enum ConsoleCompositionCommand {
    /// Build a digest-bound Console Composition Change Plan.
    Plan(ConsoleCompositionPlanArgs),
    /// Apply an exactly approved Console Composition Change Plan.
    Apply(ConsoleCompositionApplyArgs),
}

#[derive(Debug, Subcommand)]
enum ConsoleCommand {
    /// Plan or apply an independent Lenso Console installation.
    Install(ConsoleChangeArgs),
    /// Plan or apply an immutable Lenso Console Release upgrade.
    Upgrade(ConsoleChangeArgs),
    /// Create an encrypted Console Recovery Set.
    Backup(ConsoleBackupArgs),
    /// Plan or apply a fenced Console Recovery Set restore.
    Restore(ConsoleRestoreArgs),
    /// Advance an externally fenced Console recovery workflow.
    Recovery {
        #[command(subcommand)]
        command: ConsoleRecoveryCommand,
    },
    /// Validate exact Console installation evidence and optional readiness.
    Doctor(ConsoleDoctorArgs),
    /// Manage operators in the independent Lenso Console Service.
    Operator {
        #[command(subcommand)]
        command: ConsoleOperatorCommand,
    },
    /// Plan or apply the Console Service's own Module composition.
    Composition {
        #[command(subcommand)]
        command: ConsoleCompositionCommand,
    },
    /// Start the complete local Console Service.
    Dev(ConsoleDevArgs),
}

#[derive(Debug, Args, Clone)]
struct ConsoleBackupArgs {
    /// External Console installation root.
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,

    /// Secret-bearing environment file containing CONSOLE_DATABASE_URL.
    #[arg(long)]
    env_file: std::path::PathBuf,

    /// New directory that will contain the encrypted Recovery Set.
    #[arg(long)]
    output: std::path::PathBuf,

    /// age recipient that can decrypt the Store payload.
    #[arg(long)]
    recipient: String,

    /// Emit the Recovery Set manifest as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsoleRestoreArgs {
    /// External Console installation root.
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,

    /// Directory containing recovery-set.json and store.dump.age.
    #[arg(long)]
    recovery_set: std::path::PathBuf,

    /// Current deployment environment file used to fence the old workload.
    #[arg(long)]
    current_env_file: std::path::PathBuf,

    /// Recovery environment file pointing to a distinct clean Store.
    #[arg(long)]
    recovery_env_file: std::path::PathBuf,

    /// Write the deterministic restore plan to this path.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Apply the reviewed restore plan.
    #[arg(long)]
    apply: bool,

    /// Exact restore plan digest approved by the operator.
    #[arg(long, requires = "apply")]
    approve_plan_digest: Option<String>,

    /// Private age identity file used only during apply.
    #[arg(long, requires = "apply")]
    identity_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ConsoleReconcileArgs {
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,
    #[arg(long)]
    env_file: std::path::PathBuf,
    #[arg(long)]
    evidence: std::path::PathBuf,
    #[arg(long)]
    output: Option<std::path::PathBuf>,
    #[arg(long)]
    apply: bool,
    #[arg(long, requires = "apply")]
    approve_plan_digest: Option<String>,
}

#[derive(Debug, Args, Clone)]
struct ConsoleActivateArgs {
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,
    #[arg(long)]
    recovery_env_file: std::path::PathBuf,
    #[arg(long)]
    active_env_file: std::path::PathBuf,
    #[arg(long)]
    output: Option<std::path::PathBuf>,
    #[arg(long)]
    apply: bool,
    #[arg(long, requires = "apply")]
    approve_plan_digest: Option<String>,
    #[arg(long, requires = "apply")]
    approve_authority_transfer: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsoleRecoverActivationArgs {
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,
    #[arg(long)]
    recovery_env_file: std::path::PathBuf,
    #[arg(long)]
    active_env_file: std::path::PathBuf,
    #[arg(long)]
    output: Option<std::path::PathBuf>,
    #[arg(long)]
    apply: bool,
    #[arg(long, requires = "apply")]
    approve_plan_digest: Option<String>,
    #[arg(long, requires = "apply")]
    approve_authority_reset: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsoleChangeArgs {
    /// GitHub-attested Console Service Release Manifest.
    #[arg(long)]
    manifest: std::path::PathBuf,

    /// External Console installation root.
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,

    /// Secret-bearing environment file used only by Docker Compose.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Write the deterministic installation plan to this path.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Apply the reviewed plan through the local Docker Compose adapter.
    #[arg(long)]
    apply: bool,

    /// Exact plan digest approved by the operator.
    #[arg(long, requires = "apply")]
    approve_plan_digest: Option<String>,

    /// Separately approve irreversible Store migrations.
    #[arg(long, requires = "apply")]
    approve_irreversible: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsoleDoctorArgs {
    /// External Console installation root.
    #[arg(long, default_value = ".lenso-console")]
    root: std::path::PathBuf,

    /// Console URL to include an HTTPS or loopback readiness check.
    #[arg(long)]
    live_url: Option<String>,

    /// Emit the versioned doctor report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsoleDevArgs {
    /// Console repository root. Defaults to the current repository.
    #[arg(long = "console-root")]
    console_root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ConsoleCompositionPlanArgs {
    /// Desired Console composition document.
    #[arg(long)]
    composition: std::path::PathBuf,

    /// Console Service environment file containing CONSOLE_DATABASE_URL.
    #[arg(long)]
    env_file: std::path::PathBuf,

    /// Write the immutable plan to a new file.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Print the plan as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsoleCompositionApplyArgs {
    /// Immutable Console composition plan.
    #[arg(long)]
    plan: std::path::PathBuf,

    /// Console Service environment file containing CONSOLE_DATABASE_URL.
    #[arg(long)]
    env_file: std::path::PathBuf,

    /// Exact plan digest approved by the installation authority.
    #[arg(long)]
    approve_plan_digest: String,
}

#[derive(Debug, Subcommand)]
enum ModuleCommand {
    /// Create a linked module or service scaffold.
    Create(ModuleCreateArgs),
    /// Start module-local development helpers.
    Dev(ModuleDevArgs),
    /// Install a module capability from a release, catalog entry, service, or linked source.
    Install(ServiceModuleInstallArgs),
    /// Reapply an installed module from its install receipt.
    Update(ModuleUpdateArgs),
    /// Remove a Module from the application composition.
    Remove(ServiceModuleUninstallArgs),
    /// Disable a module capability.
    Disable(ServiceModuleUninstallArgs),
    /// Diagnose installed services.
    Doctor(ModuleDoctorArgs),
}

#[derive(Debug, Args, Clone)]
struct ModuleDevArgs {
    /// Start the Module-owned Console UI artifact development server.
    #[arg(long = "console-ui")]
    console_ui: bool,

    /// Module repository root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Create a service provider scaffold.
    Create(ServiceCreateArgs),
    /// Manage the local service workspace file.
    Workspace {
        #[command(subcommand)]
        command: ServiceWorkspaceCommand,
    },
    /// Manage deployment environments for service providers.
    Env {
        #[command(subcommand)]
        command: ServiceEnvCommand,
    },
    /// Export and inspect service deployments.
    Deploy {
        #[command(subcommand)]
        command: ServiceDeployCommand,
    },
    /// Start service providers, then run the generated host.
    Dev(ServiceDevArgs),
    /// Package a service provider project for distribution.
    Package(ServicePackageArgs),
    /// Install a service manifest.
    Install(ServiceInstallArgs),
    /// Remove a service provider and its provided modules.
    Uninstall(ServiceModuleUninstallArgs),
    /// Show changes between installed and candidate service manifests.
    Diff(ServiceDiffArgs),
    /// Preview the upgrade impact for an installed service.
    UpgradePlan(ServiceDiffArgs),
    /// Upgrade an installed service from a candidate manifest.
    Upgrade(ServiceUpgradeArgs),
    /// Roll back a service to the previous installed manifest snapshot.
    Rollback(ServiceRollbackArgs),
    /// Plan, check, and apply service releases.
    Release {
        #[command(subcommand)]
        command: ServiceReleaseCommand,
    },
    /// Run service delivery policy gates.
    Policy {
        #[command(subcommand)]
        command: ServicePolicyCommand,
    },
    /// Assemble and inspect Autonomous Service production delivery artifacts.
    Delivery {
        #[command(subcommand)]
        command: ServiceDeliveryCommand,
    },
    /// Diagnose installed services and their provided modules.
    Doctor(ModuleDoctorArgs),
    /// Check a service manifest or configured service state.
    Check(ServiceCheckArgs),
    /// Verify a service manifest, package, or installed provider before release.
    Verify(ServiceCheckArgs),
    /// List declared services.
    List(ModuleServiceListArgs),
    /// Export a deployment fragment for declared services.
    Export(ModuleServiceExportArgs),
    /// Show one service with local state.
    Status(ModuleServiceStatusArgs),
    /// Show local logs for a declared service.
    Logs(ModuleServiceLogsArgs),
    /// Start a declared service in the background.
    Start(ModuleServiceStartArgs),
    /// Stop a declared service started by the CLI or host.
    Stop(ModuleServiceStopArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ServiceLanguage {
    Rust,
    Ts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ServiceDeploymentTargetArg {
    Kubernetes,
    Operator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ServiceDeploymentSourceArg {
    Kubernetes,
    Operator,
}

#[derive(Debug, Args, Clone)]
struct ServiceCreateArgs {
    /// Service provider name, such as support-suite-provider.
    name: String,

    /// Generated service language.
    #[arg(long, value_enum)]
    lang: ServiceLanguage,

    /// Directory that receives the service directory.
    #[arg(long)]
    output_dir: Option<std::path::PathBuf>,

    /// Local service port used in generated manifests.
    #[arg(long, default_value_t = 4100)]
    port: u16,

    /// Service workspace file to update.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Do not register the service in lenso.workspace.json.
    #[arg(long)]
    no_workspace: bool,

    /// Print files without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Subcommand)]
enum ServiceWorkspaceCommand {
    /// Create an empty service workspace file.
    Init(ServiceWorkspaceInitArgs),
    /// Add or update a service in the workspace file.
    Add(ServiceWorkspaceAddArgs),
    /// List services in the workspace file.
    List(ServiceWorkspaceListArgs),
    /// Check service workspace readiness and manifest reachability.
    Check(ServiceWorkspaceCheckArgs),
    /// Export workspace services as host service-start state.
    Export(ServiceWorkspaceExportArgs),
}

#[derive(Debug, Subcommand)]
enum ServiceEnvCommand {
    /// List configured service deployment environments.
    List(ServiceEnvListArgs),
    /// Add or update a service deployment environment.
    Add(ServiceEnvAddArgs),
    /// Remove a service deployment environment.
    Remove(ServiceEnvRemoveArgs),
    /// Verify a service deployment environment.
    Verify(ServiceEnvVerifyArgs),
}

#[derive(Debug, Subcommand)]
enum ServiceDeployCommand {
    /// Export deployment files for a service provider.
    Export(ServiceDeployExportArgs),
    /// Read deployment status for a service provider.
    Status(ServiceDeployStatusArgs),
    /// Wait until a service deployment is ready.
    Wait(ServiceDeployWaitArgs),
}

#[derive(Debug, Args, Clone)]
struct ServiceWorkspaceInitArgs {
    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Replace an existing workspace file.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceWorkspaceAddArgs {
    /// Service provider name.
    name: String,

    /// Service directory.
    #[arg(long)]
    cwd: std::path::PathBuf,

    /// Service language label.
    #[arg(long, value_enum)]
    lang: ServiceLanguage,

    /// Service start command.
    #[arg(long)]
    command: String,

    /// Service readiness URL.
    #[arg(long)]
    ready_url: String,

    /// Module provided by this service. Can be repeated.
    #[arg(long = "module")]
    modules: Vec<String>,

    /// Service manifest path, relative to --cwd.
    #[arg(long, default_value = "lenso.service.json")]
    manifest: String,

    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceWorkspaceListArgs {
    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceWorkspaceCheckArgs {
    /// Optional service name to check.
    service_name: Option<String>,

    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceWorkspaceExportArgs {
    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Output file. Prints JSON when omitted.
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceEnvListArgs {
    /// Filter by service provider name.
    #[arg(long = "service")]
    service_name: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceEnvAddArgs {
    /// Environment name, such as staging or prod.
    name: String,

    /// Service provider name.
    #[arg(long = "service")]
    service_name: String,

    /// Deployment target.
    #[arg(long, value_enum)]
    target: ServiceDeploymentTargetArg,

    /// Kubernetes namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Kubernetes context.
    #[arg(long)]
    kube_context: Option<String>,

    /// Desired service image.
    #[arg(long)]
    image: Option<String>,

    /// Public service base URL.
    #[arg(long)]
    public_base_url: Option<String>,

    /// Service manifest URL/path.
    #[arg(long)]
    manifest_reference: Option<String>,

    /// Release track label.
    #[arg(long)]
    release_track: Option<String>,

    /// Desired Kubernetes replicas.
    #[arg(long)]
    replicas: Option<u32>,

    /// Service container port.
    #[arg(long)]
    port: Option<u16>,

    /// Kubernetes ingress host.
    #[arg(long)]
    ingress_host: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceEnvRemoveArgs {
    /// Environment name.
    name: String,

    /// Service provider name.
    #[arg(long = "service")]
    service_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print changes without writing them.
    #[arg(long)]
    dry_run: bool,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceEnvVerifyArgs {
    /// Environment name.
    name: String,

    /// Service provider name.
    #[arg(long = "service")]
    service_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeployExportArgs {
    /// Service provider name.
    service_name: String,

    /// Environment name.
    #[arg(long = "env")]
    environment_name: String,

    /// Deployment target.
    #[arg(long, value_enum, default_value_t = ServiceDeploymentTargetArg::Kubernetes)]
    target: ServiceDeploymentTargetArg,

    /// Output directory for generated deployment files.
    #[arg(long)]
    output_dir: std::path::PathBuf,

    /// Override desired service image.
    #[arg(long)]
    image: Option<String>,

    /// Override Kubernetes namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Override Kubernetes ingress host.
    #[arg(long)]
    ingress_host: Option<String>,

    /// Override service container port.
    #[arg(long)]
    port: Option<u16>,

    /// Override desired replicas.
    #[arg(long)]
    replicas: Option<u32>,

    /// Generate an example HorizontalPodAutoscaler.
    #[arg(long)]
    hpa: bool,

    /// Generate a PodDisruptionBudget.
    #[arg(long)]
    pdb: bool,

    /// Generate a default-deny NetworkPolicy with ingress to the service port.
    #[arg(long)]
    network_policy: bool,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeployStatusArgs {
    /// Service provider name.
    service_name: String,

    /// Environment name.
    #[arg(long = "env")]
    environment_name: String,

    /// Read deployment/provider status JSON from a file instead of a source adapter.
    #[arg(long)]
    from_file: Option<std::path::PathBuf>,

    /// Deployment status source.
    #[arg(long, value_enum, default_value_t = ServiceDeploymentSourceArg::Kubernetes)]
    source: ServiceDeploymentSourceArg,

    /// Persist the observation to .lenso/service-deployments.json.
    #[arg(long)]
    write_state: bool,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeployWaitArgs {
    /// Service provider name.
    service_name: String,

    /// Environment name.
    #[arg(long = "env")]
    environment_name: String,

    /// Read deployment JSON from a file instead of a provider adapter.
    #[arg(long)]
    from_file: Option<std::path::PathBuf>,

    /// Deployment status source.
    #[arg(long, value_enum, default_value_t = ServiceDeploymentSourceArg::Kubernetes)]
    source: ServiceDeploymentSourceArg,

    /// Timeout in seconds.
    #[arg(long, default_value_t = 120)]
    timeout_seconds: u64,

    /// Poll interval in seconds.
    #[arg(long, default_value_t = 5)]
    interval_seconds: u64,

    /// Persist every observation to .lenso/service-deployments.json.
    #[arg(long)]
    write_state: bool,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceDevArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Service workspace file.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,

    /// Do not start service workspace entries.
    #[arg(long)]
    no_workspace: bool,

    /// Do not start the template Postgres service.
    #[arg(long)]
    skip_db: bool,

    /// Do not run migrations before starting services.
    #[arg(long)]
    skip_migrate: bool,

    /// Run API and worker as separate local processes.
    #[arg(long)]
    separate_worker: bool,
}

#[derive(Debug, Args, Clone)]
struct ServicePackageArgs {
    /// Service provider project directory.
    #[arg(default_value = ".")]
    service_dir: std::path::PathBuf,

    /// Service manifest path or URL. Paths are relative to the service directory unless absolute.
    #[arg(long, default_value = "lenso.service.json")]
    manifest: String,

    /// Directory that receives package artifacts, relative to the service directory unless absolute.
    #[arg(long, default_value = "dist/lenso-service")]
    output_dir: std::path::PathBuf,

    /// Validate the package inputs and planned artifact without writing files.
    #[arg(long)]
    check: bool,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceCheckArgs {
    /// Service manifest URL/path, or optional module name for installed-service checks.
    manifest_reference: Option<String>,

    /// Start this command before checking readiness and manifest fetch.
    #[arg(long)]
    serve_command: Option<String>,

    /// Working directory for --serve-command.
    #[arg(long)]
    cwd: Option<std::path::PathBuf>,

    /// Ready/status URL to poll when using --serve-command.
    #[arg(long)]
    ready_url: Option<String>,

    /// Manifest URL to fetch after --serve-command becomes ready.
    #[arg(long)]
    manifest_url: Option<String>,

    /// Only check one operation id.
    #[arg(long)]
    operation: Option<String>,

    /// JSON sample input used for explicit safe probes.
    #[arg(long)]
    sample_input: Option<std::path::PathBuf>,

    /// Ready wait timeout in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    ready_timeout_ms: u64,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to read when checking installed services.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceDiffArgs {
    /// Installed service provider name.
    service_name: String,

    /// Candidate service manifest URL/path.
    manifest_reference: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceUpgradeArgs {
    /// Installed service provider name.
    service_name: String,

    /// Candidate service manifest URL/path.
    manifest_reference: String,

    /// Provider Service base URL for local manifest files.
    #[arg(long)]
    base_url: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print changes without writing them.
    #[arg(long)]
    dry_run: bool,

    /// Allow upgrade when compatibility metadata does not match this host.
    #[arg(long)]
    allow_incompatible: bool,

    /// Print the dry-run proposal as machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceRollbackArgs {
    /// Installed service provider name.
    service_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print changes without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Subcommand)]
enum ServiceReleaseCommand {
    /// Build a reusable service release plan from an installed service and candidate manifest.
    Plan(ServiceReleasePlanArgs),
    /// Check a service release plan without applying it.
    Check(ServiceReleaseCheckArgs),
    /// Apply a checked service release plan and record it in the service release ledger.
    Apply(ServiceReleaseApplyArgs),
    /// Create a target-environment release plan from the latest source-environment release.
    Promote(ServiceReleasePromoteArgs),
    /// Create a rollback release plan for one environment.
    Rollback(ServiceReleaseRollbackArgs),
}

#[derive(Debug, Subcommand)]
enum ServicePolicyCommand {
    /// Check a service release plan against built-in delivery policy.
    Check(ServicePolicyCheckArgs),
}

#[derive(Debug, Subcommand)]
enum ServiceDeliveryCommand {
    /// Assemble one immutable environment-independent Service Release.
    Assemble(ServiceDeliveryAssembleArgs),
    /// Validate and render one Service Release.
    Check(ServiceDeliveryArtifactArgs),
    /// Diff two immutable Service Releases.
    Diff(ServiceDeliveryDiffArgs),
    /// Evaluate canonical Policy source inputs without trusting precomputed decisions.
    Policy(ServiceDeliveryArtifactArgs),
    /// Evaluate production eligibility evidence without mutation.
    CanIDeploy(ServiceDeliveryArtifactArgs),
    /// Validate and render a shared Deployment Adapter plan.
    DeploymentPlan(ServiceDeliveryArtifactArgs),
    /// Dry-run an Autonomous Service Operator resource and reviewable diff.
    OperatorExport(ServiceDeliveryOperatorExportArgs),
    /// Authorize an exact production Operator resource at the human Approval Boundary.
    PromotionApply(ServiceDeliveryPromotionApplyArgs),
}

#[derive(Debug, Args, Clone)]
struct ServiceDeliveryAssembleArgs {
    /// Service Release input JSON produced from existing CI artifacts.
    input: std::path::PathBuf,

    /// Write stable JSON to this path instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeliveryArtifactArgs {
    /// Versioned delivery artifact JSON.
    artifact: std::path::PathBuf,

    /// Write normalized stable JSON to this path instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeliveryDiffArgs {
    /// Previous Service Release JSON.
    from: std::path::PathBuf,

    /// Candidate Service Release JSON.
    to: std::path::PathBuf,

    /// Write stable diff JSON to this path instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeliveryOperatorExportArgs {
    /// Shared Kubernetes Deployment Adapter plan JSON.
    deployment_plan: std::path::PathBuf,

    /// Previous dry-run export JSON for a deterministic review diff.
    #[arg(long)]
    previous: Option<std::path::PathBuf>,

    /// Write stable export JSON to this path instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceDeliveryPromotionApplyArgs {
    /// Content-addressed Promotion plan JSON.
    promotion_plan: std::path::PathBuf,
    /// Provider-authorized human approval JSON.
    approval: std::path::PathBuf,
    /// Current protected evidence JSON.
    protected_evidence: std::path::PathBuf,
    /// Exact source Environment Verification JSON.
    environment_verification: std::path::PathBuf,
    /// Fresh credential-free source Operator observation JSON.
    source_observation: std::path::PathBuf,
    /// Fresh approval-challenge-bound source Gateway observation JSON.
    source_gateway_observation: std::path::PathBuf,
    /// Fresh credential-free target Operator observation JSON with Kubernetes CAS identity.
    target_observation: std::path::PathBuf,
    /// Dry-run target Operator export JSON.
    operator_export: std::path::PathBuf,
    /// Write the authorized resource envelope to this path instead of stdout.
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceReleasePlanArgs {
    /// Installed service provider name.
    service_name: String,

    /// Candidate service manifest or service package URL/path.
    manifest_reference: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service deployment environment.
    #[arg(long = "env")]
    environment_name: Option<String>,

    /// Write the release plan JSON to this path.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Fail when policy risk is at or above this level: needs_attention, breaking, blocked.
    #[arg(long)]
    fail_on: Option<String>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceReleaseCheckArgs {
    /// Service release plan JSON path.
    plan_file: std::path::PathBuf,

    /// Require the plan to match this service deployment environment.
    #[arg(long = "env")]
    environment_name: Option<String>,

    /// Fail when policy risk is at or above this level: needs_attention, breaking, blocked.
    #[arg(long)]
    fail_on: Option<String>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceReleaseApplyArgs {
    /// Service release plan JSON path.
    plan_file: std::path::PathBuf,

    /// Require the plan to match this service deployment environment.
    #[arg(long = "env")]
    environment_name: Option<String>,

    /// Provider Service base URL for local manifest files.
    #[arg(long)]
    base_url: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print changes without writing them.
    #[arg(long)]
    dry_run: bool,

    /// Allow apply when compatibility metadata does not match this host.
    #[arg(long)]
    allow_incompatible: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceReleasePromoteArgs {
    /// Installed service provider name.
    service_name: String,

    /// Source environment name.
    #[arg(long = "from")]
    from_environment: String,

    /// Target environment name.
    #[arg(long = "to")]
    to_environment: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Write the release plan JSON to this path.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Fail when policy risk is at or above this level: needs_attention, breaking, blocked.
    #[arg(long)]
    fail_on: Option<String>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceReleaseRollbackArgs {
    /// Installed service provider name.
    service_name: String,

    /// Environment name.
    #[arg(long = "env")]
    environment_name: String,

    /// Roll back to this release id instead of the previous same-environment release.
    #[arg(long = "to")]
    release_id: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Write the rollback plan JSON to this path.
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ServicePolicyCheckArgs {
    /// Service release plan JSON path.
    plan_file: std::path::PathBuf,

    /// Fail when policy risk is at or above this level: needs_attention, breaking, blocked.
    #[arg(long)]
    fail_on: Option<String>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum ModuleServiceCommand {
    /// List declared services.
    List(ModuleServiceListArgs),
    /// Export a deployment fragment for declared services.
    Export(ModuleServiceExportArgs),
    /// Show one service with local state.
    Status(ModuleServiceStatusArgs),
    /// Show local logs for a declared service.
    Logs(ModuleServiceLogsArgs),
    /// Start a declared service in the background.
    Start(ModuleServiceStartArgs),
    /// Stop a declared service started by the CLI or host.
    Stop(ModuleServiceStopArgs),
}

#[derive(Debug, Args, Clone)]
struct ModuleServiceListArgs {
    /// Optional module name to list.
    module_name: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ModuleServiceExportArgs {
    /// Module name.
    #[arg(long = "module")]
    module_name: String,

    /// Export format.
    #[arg(long, default_value = "compose")]
    format: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ModuleServiceStatusArgs {
    /// Module name.
    module_name: String,

    /// Service name.
    service_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ModuleServiceLogsArgs {
    /// Module name.
    module_name: String,

    /// Service name.
    service_name: String,

    /// Number of log lines to print.
    #[arg(long, default_value_t = 100)]
    tail: usize,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ModuleServiceStartArgs {
    /// Module name.
    module_name: String,

    /// Service name.
    service_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ModuleServiceStopArgs {
    /// Module name.
    module_name: String,

    /// Service name.
    service_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceModuleInstallArgs {
    /// Exact Module Release reference, catalog entry, Service export, or linked Module name.
    manifest_reference: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Service module base URL.
    #[arg(long)]
    base_url: Option<String>,

    /// Catalog registry URL used when installing by name.
    #[arg(long)]
    catalog_url: Option<String>,

    /// Install descriptor profile to apply.
    #[arg(long = "profile", alias = "with", value_delimiter = ',')]
    install_profiles: Vec<String>,

    /// Execute manifest-declared install.commands.
    #[arg(long)]
    run_install_commands: bool,

    /// Print install changes without writing them.
    #[arg(long)]
    dry_run: bool,

    /// Allow install when manifest compatibility metadata does not match this host.
    #[arg(long)]
    allow_incompatible: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceInstallArgs {
    #[command(flatten)]
    install: ServiceModuleInstallArgs,

    /// Service workspace file used to infer --base-url for local service manifests.
    #[arg(long)]
    workspace_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ServiceModuleUninstallArgs {
    /// Module name.
    module_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print uninstall changes without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args, Clone)]
struct ModuleUpdateArgs {
    /// Installed module name.
    module_name: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Service module base URL override.
    #[arg(long)]
    base_url: Option<String>,

    /// Install descriptor profile to apply.
    #[arg(long = "profile", alias = "with", value_delimiter = ',')]
    install_profiles: Vec<String>,

    /// Execute manifest-declared install.commands.
    #[arg(long)]
    run_install_commands: bool,

    /// Print update changes without writing them.
    #[arg(long)]
    dry_run: bool,

    /// Allow update when manifest compatibility metadata does not match this host.
    #[arg(long)]
    allow_incompatible: bool,
}

#[derive(Debug, Args, Clone)]
struct ModuleDoctorArgs {
    /// Optional module name to check.
    module_name: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to read.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Service module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct ModuleCreateArgs {
    /// Module id, such as billing or support.
    module_id: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Display label.
    #[arg(long)]
    label: Option<String>,

    /// Console route.
    #[arg(long)]
    route: Option<String>,

    /// Required capability.
    #[arg(long)]
    capability: Option<String>,

    /// Lucide icon name.
    #[arg(long)]
    icon: Option<String>,

    /// Create a Console UI artifact bound to the same Module Release.
    #[arg(long = "with-console-ui")]
    with_console_ui: bool,

    /// Console surface name.
    #[arg(long)]
    surface_name: Option<String>,

    /// Print files without writing them.
    #[arg(long)]
    dry_run: bool,
}

impl From<&ServiceModuleInstallArgs> for module::ServiceModuleInstallOptions {
    fn from(args: &ServiceModuleInstallArgs) -> Self {
        Self {
            allow_incompatible: args.allow_incompatible,
            base_url: args.base_url.clone(),
            catalog_url: args.catalog_url.clone(),
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            install_profiles: args.install_profiles.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            run_install_commands: args.run_install_commands,
            source: "service".to_owned(),
        }
    }
}

impl From<&ServiceModuleUninstallArgs> for module::ServiceModuleUninstallOptions {
    fn from(args: &ServiceModuleUninstallArgs) -> Self {
        Self {
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            source: None,
        }
    }
}

impl From<&ModuleUpdateArgs> for module::ModuleUpdateOptions {
    fn from(args: &ModuleUpdateArgs) -> Self {
        Self {
            allow_incompatible: args.allow_incompatible,
            base_url: args.base_url.clone(),
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            install_profiles: args.install_profiles.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            run_install_commands: args.run_install_commands,
        }
    }
}

impl From<&ModuleDoctorArgs> for module::ModuleDoctorOptions {
    fn from(args: &ModuleDoctorArgs) -> Self {
        Self {
            env_file: args.env_file.clone(),
            json: args.json,
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
        }
    }
}

impl From<&ServiceCheckArgs> for module::ModuleDoctorOptions {
    fn from(args: &ServiceCheckArgs) -> Self {
        Self {
            env_file: args.env_file.clone(),
            json: args.json,
            module_name: args.manifest_reference.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
        }
    }
}

impl From<&ServiceDiffArgs> for module::ServiceDiffOptions {
    fn from(args: &ServiceDiffArgs) -> Self {
        Self {
            json: args.json,
            manifest_reference: args.manifest_reference.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServiceUpgradeArgs> for module::ServiceUpgradeOptions {
    fn from(args: &ServiceUpgradeArgs) -> Self {
        Self {
            allow_incompatible: args.allow_incompatible,
            base_url: args.base_url.clone(),
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            json: args.json,
            manifest_reference: args.manifest_reference.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServiceRollbackArgs> for module::ServiceRollbackOptions {
    fn from(args: &ServiceRollbackArgs) -> Self {
        Self {
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServiceEnvListArgs> for module::ServiceEnvListOptions {
    fn from(args: &ServiceEnvListArgs) -> Self {
        Self {
            json: args.json,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServiceEnvAddArgs> for module::ServiceEnvAddOptions {
    fn from(args: &ServiceEnvAddArgs) -> Self {
        Self {
            environment_name: args.name.clone(),
            image: args.image.clone(),
            ingress_host: args.ingress_host.clone(),
            json: args.json,
            kube_context: args.kube_context.clone(),
            manifest_reference: args.manifest_reference.clone(),
            namespace: args.namespace.clone(),
            port: args.port,
            public_base_url: args.public_base_url.clone(),
            release_track: args.release_track.clone(),
            replicas: args.replicas,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
            target: service_deployment_target_arg(args.target).to_owned(),
        }
    }
}

impl From<&ServiceEnvRemoveArgs> for module::ServiceEnvRemoveOptions {
    fn from(args: &ServiceEnvRemoveArgs) -> Self {
        Self {
            dry_run: args.dry_run,
            environment_name: args.name.clone(),
            json: args.json,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServiceEnvVerifyArgs> for module::ServiceEnvVerifyOptions {
    fn from(args: &ServiceEnvVerifyArgs) -> Self {
        Self {
            environment_name: args.name.clone(),
            json: args.json,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServiceDeployExportArgs> for module::ServiceDeployExportOptions {
    fn from(args: &ServiceDeployExportArgs) -> Self {
        Self {
            environment_name: args.environment_name.clone(),
            image: args.image.clone(),
            ingress_host: args.ingress_host.clone(),
            json: args.json,
            namespace: args.namespace.clone(),
            output_dir: args.output_dir.clone(),
            hpa: args.hpa,
            port: args.port,
            pdb: args.pdb,
            network_policy: args.network_policy,
            replicas: args.replicas,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
            target: service_deployment_target_arg(args.target).to_owned(),
        }
    }
}

impl From<&ServiceDeployStatusArgs> for module::ServiceDeployStatusOptions {
    fn from(args: &ServiceDeployStatusArgs) -> Self {
        Self {
            environment_name: args.environment_name.clone(),
            from_file: args.from_file.clone(),
            json: args.json,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
            source: service_deployment_source_arg(args.source).to_owned(),
            write_state: args.write_state,
        }
    }
}

impl From<&ServiceDeployWaitArgs> for module::ServiceDeployWaitOptions {
    fn from(args: &ServiceDeployWaitArgs) -> Self {
        Self {
            environment_name: args.environment_name.clone(),
            from_file: args.from_file.clone(),
            interval_seconds: args.interval_seconds,
            json: args.json,
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
            source: service_deployment_source_arg(args.source).to_owned(),
            timeout_seconds: args.timeout_seconds,
            write_state: args.write_state,
        }
    }
}

impl From<&ServiceReleasePlanArgs> for module::ServiceReleasePlanOptions {
    fn from(args: &ServiceReleasePlanArgs) -> Self {
        Self {
            environment_name: args.environment_name.clone(),
            fail_on: args.fail_on.clone(),
            json: args.json,
            manifest_reference: args.manifest_reference.clone(),
            output: args.output.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

const fn service_deployment_target_arg(target: ServiceDeploymentTargetArg) -> &'static str {
    match target {
        ServiceDeploymentTargetArg::Kubernetes => "kubernetes",
        ServiceDeploymentTargetArg::Operator => "operator",
    }
}

const fn service_deployment_source_arg(source: ServiceDeploymentSourceArg) -> &'static str {
    match source {
        ServiceDeploymentSourceArg::Kubernetes => "kubernetes",
        ServiceDeploymentSourceArg::Operator => "operator",
    }
}

impl From<&ServiceReleaseCheckArgs> for module::ServiceReleaseCheckOptions {
    fn from(args: &ServiceReleaseCheckArgs) -> Self {
        Self {
            environment_name: args.environment_name.clone(),
            fail_on: args.fail_on.clone(),
            json: args.json,
            plan_file: args.plan_file.clone(),
        }
    }
}

impl From<&ServiceReleaseApplyArgs> for module::ServiceReleaseApplyOptions {
    fn from(args: &ServiceReleaseApplyArgs) -> Self {
        Self {
            allow_incompatible: args.allow_incompatible,
            base_url: args.base_url.clone(),
            dry_run: args.dry_run,
            environment_name: args.environment_name.clone(),
            env_file: args.env_file.clone(),
            module_services_file: args.module_services_file.clone(),
            plan_file: args.plan_file.clone(),
            repo_root: args.repo_root.clone(),
        }
    }
}

impl From<&ServiceReleasePromoteArgs> for module::ServiceReleasePromoteOptions {
    fn from(args: &ServiceReleasePromoteArgs) -> Self {
        Self {
            fail_on: args.fail_on.clone(),
            from_environment: args.from_environment.clone(),
            json: args.json,
            output: args.output.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
            to_environment: args.to_environment.clone(),
        }
    }
}

impl From<&ServiceReleaseRollbackArgs> for module::ServiceReleaseRollbackPlanOptions {
    fn from(args: &ServiceReleaseRollbackArgs) -> Self {
        Self {
            environment_name: args.environment_name.clone(),
            json: args.json,
            output: args.output.clone(),
            release_id: args.release_id.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ServicePolicyCheckArgs> for module::ServiceReleaseCheckOptions {
    fn from(args: &ServicePolicyCheckArgs) -> Self {
        Self {
            environment_name: None,
            fail_on: args.fail_on.clone(),
            json: args.json,
            plan_file: args.plan_file.clone(),
        }
    }
}

impl From<&ModuleServiceListArgs> for module::ModuleServiceListOptions {
    fn from(args: &ModuleServiceListArgs) -> Self {
        Self {
            json: args.json,
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
        }
    }
}

impl From<&ModuleServiceExportArgs> for module::ModuleServiceExportOptions {
    fn from(args: &ModuleServiceExportArgs) -> Self {
        Self {
            format: args.format.clone(),
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
        }
    }
}

impl From<&ModuleServiceStatusArgs> for module::ModuleServiceStatusOptions {
    fn from(args: &ModuleServiceStatusArgs) -> Self {
        Self {
            json: args.json,
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ModuleServiceLogsArgs> for module::ModuleServiceLogsOptions {
    fn from(args: &ModuleServiceLogsArgs) -> Self {
        Self {
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
            tail: args.tail,
        }
    }
}

impl From<&ModuleServiceStartArgs> for module::ModuleServiceStartOptions {
    fn from(args: &ModuleServiceStartArgs) -> Self {
        Self {
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ModuleServiceStopArgs> for module::ModuleServiceStopOptions {
    fn from(args: &ModuleServiceStopArgs) -> Self {
        Self {
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            service_name: args.service_name.clone(),
        }
    }
}

impl From<&ConsoleOperatorBootstrapArgs> for console_operator::BootstrapOperatorOptions {
    fn from(args: &ConsoleOperatorBootstrapArgs) -> Self {
        Self {
            console_root: args.console_root.clone(),
            console_url: args.console_url.clone(),
            env_file: args.env_file.clone(),
            identifier: args.identifier.clone(),
            password_file: args.password_file.clone(),
            password_stdin: args.password_stdin,
            scopes: args.scopes.clone(),
            user_id: args.user_id.clone(),
        }
    }
}

impl From<&ConsoleChangeArgs> for console_installation::ChangeOptions {
    fn from(args: &ConsoleChangeArgs) -> Self {
        Self {
            manifest: args.manifest.clone(),
            root: args.root.clone(),
            env_file: args.env_file.clone(),
            output: args.output.clone(),
            apply: args.apply,
            approve_plan_digest: args.approve_plan_digest.clone(),
            approve_irreversible: args.approve_irreversible,
        }
    }
}

impl From<&ConsoleDoctorArgs> for console_installation::DoctorOptions {
    fn from(args: &ConsoleDoctorArgs) -> Self {
        Self {
            root: args.root.clone(),
            live_url: args.live_url.clone(),
            json: args.json,
        }
    }
}

impl From<&ConsoleBackupArgs> for console_installation::BackupOptions {
    fn from(args: &ConsoleBackupArgs) -> Self {
        Self {
            root: args.root.clone(),
            env_file: args.env_file.clone(),
            output: args.output.clone(),
            recipient: args.recipient.clone(),
            json: args.json,
        }
    }
}

impl From<&ConsoleRestoreArgs> for console_installation::RestoreOptions {
    fn from(args: &ConsoleRestoreArgs) -> Self {
        Self {
            root: args.root.clone(),
            recovery_set: args.recovery_set.clone(),
            current_env_file: args.current_env_file.clone(),
            recovery_env_file: args.recovery_env_file.clone(),
            output: args.output.clone(),
            apply: args.apply,
            approve_plan_digest: args.approve_plan_digest.clone(),
            identity_file: args.identity_file.clone(),
        }
    }
}

impl From<&ConsoleReconcileArgs> for console_installation::ReconcileOptions {
    fn from(args: &ConsoleReconcileArgs) -> Self {
        Self {
            root: args.root.clone(),
            env_file: args.env_file.clone(),
            evidence: args.evidence.clone(),
            output: args.output.clone(),
            apply: args.apply,
            approve_plan_digest: args.approve_plan_digest.clone(),
        }
    }
}

impl From<&ConsoleActivateArgs> for console_installation::ActivateOptions {
    fn from(args: &ConsoleActivateArgs) -> Self {
        Self {
            root: args.root.clone(),
            recovery_env_file: args.recovery_env_file.clone(),
            active_env_file: args.active_env_file.clone(),
            output: args.output.clone(),
            apply: args.apply,
            approve_plan_digest: args.approve_plan_digest.clone(),
            approve_authority_transfer: args.approve_authority_transfer,
        }
    }
}

impl From<&ConsoleRecoverActivationArgs> for console_installation::RecoverActivationOptions {
    fn from(args: &ConsoleRecoverActivationArgs) -> Self {
        Self {
            root: args.root.clone(),
            recovery_env_file: args.recovery_env_file.clone(),
            active_env_file: args.active_env_file.clone(),
            output: args.output.clone(),
            apply: args.apply,
            approve_plan_digest: args.approve_plan_digest.clone(),
            approve_authority_reset: args.approve_authority_reset,
        }
    }
}

impl From<&ModuleCreateArgs> for module::ModuleCreateOptions {
    fn from(args: &ModuleCreateArgs) -> Self {
        Self {
            capability: args.capability.clone(),
            dry_run: args.dry_run,
            icon: args.icon.clone(),
            label: args.label.clone(),
            module_id: args.module_id.clone(),
            repo_root: args.repo_root.clone(),
            route: args.route.clone(),
            surface_name: args.surface_name.clone(),
            with_console: args.with_console_ui,
        }
    }
}

fn looks_like_manifest_reference(reference: &str) -> bool {
    reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.ends_with(".json")
        || reference.contains("/manifest")
}

fn service_check_uses_manifest(args: &ServiceCheckArgs) -> bool {
    args.serve_command.is_some()
        || args.operation.is_some()
        || args.sample_input.is_some()
        || args
            .manifest_reference
            .as_deref()
            .is_some_and(looks_like_manifest_reference)
}

fn service_verify_uses_manifest(args: &ServiceCheckArgs) -> bool {
    args.manifest_reference.is_none() || service_check_uses_manifest(args)
}

async fn run_service_check_or_doctor(
    args: &ServiceCheckArgs,
    default_to_manifest: bool,
) -> anyhow::Result<()> {
    let uses_manifest = if default_to_manifest {
        service_verify_uses_manifest(args)
    } else {
        service_check_uses_manifest(args)
    };
    if uses_manifest {
        module::check_service_manifest_reference(
            args.manifest_reference
                .as_deref()
                .unwrap_or("./lenso.service.json"),
            module::ServiceManifestCheckOptions {
                cwd: args.cwd.clone(),
                env_file: args.env_file.clone(),
                json: args.json,
                manifest_url: args.manifest_url.clone(),
                operation: args.operation.clone(),
                ready_timeout_ms: args.ready_timeout_ms,
                ready_url: args.ready_url.clone(),
                repo_root: args.repo_root.clone(),
                sample_input: args.sample_input.clone(),
                serve_command: args.serve_command.clone(),
            },
        )
        .await?;
    } else {
        module::doctor_module(args.into()).await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve(args) => {
            host::serve(
                args.repo_root.as_deref(),
                args.skip_db,
                args.skip_migrate,
                args.separate_worker,
            )
            .await?;
        }
        Command::App { command } => match command {
            AppCommand::Create(args) => {
                launchpad::create_app(launchpad::AppCreateOptions {
                    blueprint: args.blueprint,
                    dir: args.dir,
                    force: args.force,
                })?;
            }
            AppCommand::List => {
                launchpad::list_blueprints();
            }
            AppCommand::Inspect(args) => {
                launchpad::inspect_blueprint(&args.blueprint)?;
            }
            AppCommand::Add(args) => {
                launchpad::add_app_addon(launchpad::AppAddOptions {
                    addon: args.addon,
                    observed_revision: args.observed_revision,
                })?;
            }
            AppCommand::Compose(args) => {
                launchpad::app_compose(launchpad::AppComposeOptions {
                    addons: Vec::new(),
                    apply: args.apply,
                    blueprint: args.blueprint,
                    dir: args.dir,
                    explain: false,
                    implementations: args.implementations,
                    observed_revision: args.observed_revision,
                    packs: args.packs,
                    repo_root: args.repo_root,
                    write_plan: false,
                })?;
            }
        },
        Command::Dev { command } => match command {
            DevCommand::Up(args) => {
                service::dev_service(service::ServiceDevOptions {
                    module_services_file: args.module_services_file,
                    no_workspace: args.no_workspace,
                    repo_root: args.repo_root,
                    separate_worker: args.separate_worker,
                    skip_db: args.skip_db,
                    skip_migrate: args.skip_migrate,
                    workspace_file: args.workspace_file,
                })
                .await?;
            }
            DevCommand::Status(args) => {
                launchpad::dev_status(launchpad::DevStatusOptions {
                    repo_root: args.repo_root,
                })?;
            }
            DevCommand::Doctor(args) => {
                launchpad::dev_doctor(launchpad::DevDoctorOptions {
                    live: args.live,
                    repo_root: args.repo_root,
                    write_state: args.write_state,
                })
                .await?;
            }
            DevCommand::Stop => {
                launchpad::dev_stop();
            }
        },
        Command::Agent { command } => match command {
            AgentCommand::Context(args) => {
                launchpad::agent_context(launchpad::AgentContextOptions {
                    for_capability: args.for_capability,
                    for_module: args.for_module,
                    from_app_plan: false,
                    output: args.output,
                    repo_root: args.repo_root,
                    task: None,
                })?;
            }
            AgentCommand::Task(args) => {
                launchpad::agent_context(launchpad::AgentContextOptions {
                    for_capability: args.for_capability,
                    for_module: args.for_module,
                    from_app_plan: false,
                    output: args.output,
                    repo_root: args.repo_root,
                    task: Some(args.task),
                })?;
            }
        },
        Command::Capability { command } => match command {
            CapabilityCommand::Init(args) => {
                capability::init(capability::InitOptions {
                    blueprints: args.for_blueprint,
                    dir: args.dir,
                    lang: args.lang,
                    name: args.name,
                })?;
            }
            CapabilityCommand::Check(args) => {
                capability::check(capability::CheckOptions {
                    json: args.json,
                    path: args.path,
                })?;
            }
            CapabilityCommand::Inspect(args) => {
                capability::inspect(capability::InspectOptions { path: args.path })?;
            }
            CapabilityCommand::Library { command } => match command {
                CapabilityLibraryCommand::Init(args) => {
                    capability::library_init(capability::LibraryInitOptions {
                        repo_root: args.repo_root,
                    })?;
                }
                CapabilityLibraryCommand::Add(args) => {
                    capability::library_add(capability::LibraryAddOptions {
                        path: args.path,
                        repo_root: args.repo_root,
                    })?;
                }
                CapabilityLibraryCommand::List(args) => {
                    capability::library_list(capability::LibraryListOptions {
                        json: args.json,
                        repo_root: args.repo_root,
                    })?;
                }
                CapabilityLibraryCommand::Check(args) => {
                    capability::library_check(capability::LibraryCheckOptions {
                        json: args.json,
                        repo_root: args.repo_root,
                    })?;
                }
            },
            CapabilityCommand::Fit(args) => {
                launchpad::capability_fit(launchpad::CapabilityFitOptions {
                    json: args.json,
                    pack: args.pack,
                    repo_root: args.repo_root,
                })?;
            }
        },
        Command::Host { command } => match command {
            HostCommand::Init { dir, name, force } => host::init(&dir, name.as_deref(), force)?,
        },
        Command::Console { command } => match command {
            ConsoleCommand::Install(args) => console_installation::install((&args).into())?,
            ConsoleCommand::Upgrade(args) => console_installation::upgrade((&args).into())?,
            ConsoleCommand::Backup(args) => console_installation::backup((&args).into())?,
            ConsoleCommand::Restore(args) => console_installation::restore((&args).into())?,
            ConsoleCommand::Recovery { command } => match command {
                ConsoleRecoveryCommand::Reconcile(args) => {
                    console_installation::reconcile((&args).into())?;
                }
                ConsoleRecoveryCommand::Activate(args) => {
                    console_installation::activate((&args).into())?;
                }
                ConsoleRecoveryCommand::RecoverActivation(args) => {
                    console_installation::recover_activation((&args).into())?;
                }
            },
            ConsoleCommand::Doctor(args) => console_installation::doctor((&args).into()).await?,
            ConsoleCommand::Operator { command } => match command {
                ConsoleOperatorCommand::Bootstrap(args) => {
                    console_operator::bootstrap_operator((&args).into()).await?;
                }
            },
            ConsoleCommand::Composition { command } => match command {
                ConsoleCompositionCommand::Plan(args) => {
                    console_composition::plan(console_composition::PlanOptions {
                        composition_file: args.composition,
                        env_file: args.env_file,
                        json: args.json,
                        output: args.output,
                    })
                    .await?;
                }
                ConsoleCompositionCommand::Apply(args) => {
                    console_composition::apply(console_composition::ApplyOptions {
                        approve_plan_digest: args.approve_plan_digest,
                        env_file: args.env_file,
                        plan_file: args.plan,
                    })
                    .await?;
                }
            },
            ConsoleCommand::Dev(args) => {
                console_dev::run_console_dev(console_dev::ConsoleDevOptions {
                    console_root: args.console_root,
                })?;
            }
        },
        Command::Ga { command } => match command {
            GaCommand::SupportCheck(args) => ga::support_check(
                &args.manifest,
                &args.components,
                &args.state_version,
                args.json,
            )?,
            GaCommand::ManifestMigrate(args) => ga::manifest_migrate(
                &args.manifest,
                &args.source,
                &args.target_format,
                &args.identity_pointers,
                args.target.as_deref(),
                args.dry_run,
                args.json,
            )?,
            GaCommand::ServiceUpgrade(args) => {
                ga::service_upgrade(&args.manifest, &args.input, args.json)?;
            }
            GaCommand::ContractRetire(args) => ga::contract_retire(
                &args.input,
                args.approval.as_deref(),
                args.output.as_deref(),
                args.json,
            )?,
            GaCommand::FailureEvaluate(args) => ga::failure_evaluate(&args.input, args.json)?,
        },
        Command::Operator { command } => match command {
            OperatorCommand::ExportCrd(args) => {
                operator::export_crd_bundle((&args).into())?;
            }
        },
        Command::System { command } => match command {
            SystemCommand::Dev(args) => {
                system_sandbox::dev_system(system_sandbox::SystemDevOptions {
                    cleanup: args.cleanup,
                    dry_run: args.dry_run,
                    json: args.json,
                    sandbox_file: args.sandbox_file,
                    scenario: args.scenario,
                    system_file: args.system_file,
                    adapter_child: args.adapter_child,
                })
                .await?;
            }
            SystemCommand::Check(args) => {
                system::plan_system(system::SystemPlanOptions {
                    check: true,
                    json: args.json,
                    system_file: args.system_file,
                })?;
            }
        },
        Command::Module { command } => match command {
            ModuleCommand::Create(args) => {
                module::create_module((&args).into()).await?;
            }
            ModuleCommand::Dev(args) => {
                if !args.console_ui {
                    anyhow::bail!("`lenso module dev` currently requires --console-ui");
                }
                console_dev::run_module_console_ui_dev(args.repo_root.as_deref())?;
            }
            ModuleCommand::Install(args) => {
                module::install_module(&args.manifest_reference, (&args).into()).await?;
            }
            ModuleCommand::Update(args) => {
                module::update_module(&args.module_name, (&args).into()).await?;
            }
            ModuleCommand::Remove(args) => {
                module::uninstall_module(&args.module_name, (&args).into()).await?;
            }
            ModuleCommand::Disable(args) => {
                module::uninstall_module(&args.module_name, (&args).into()).await?;
            }
            ModuleCommand::Doctor(args) => {
                module::doctor_module((&args).into()).await?;
            }
        },
        Command::Service { command } => match command {
            ServiceCommand::Create(args) => {
                service::create_service((&args).into())?;
            }
            ServiceCommand::Workspace { command } => match command {
                ServiceWorkspaceCommand::Init(args) => {
                    service::init_service_workspace(service::ServiceWorkspaceInitOptions {
                        force: args.force,
                        workspace_file: args.workspace_file,
                    })?;
                }
                ServiceWorkspaceCommand::Add(args) => {
                    service::add_service_workspace_entry(service::ServiceWorkspaceAddOptions {
                        command: args.command,
                        cwd: args.cwd,
                        lang: args.lang,
                        manifest: args.manifest,
                        modules: args.modules,
                        name: args.name,
                        ready_url: args.ready_url,
                        workspace_file: args.workspace_file,
                    })?;
                }
                ServiceWorkspaceCommand::List(args) => {
                    service::list_service_workspace(service::ServiceWorkspaceListOptions {
                        json: args.json,
                        workspace_file: args.workspace_file,
                    })?;
                }
                ServiceWorkspaceCommand::Check(args) => {
                    service::check_service_workspace(service::ServiceWorkspaceCheckOptions {
                        json: args.json,
                        service_name: args.service_name,
                        workspace_file: args.workspace_file,
                    })
                    .await?;
                }
                ServiceWorkspaceCommand::Export(args) => {
                    service::export_service_workspace(service::ServiceWorkspaceExportOptions {
                        output: args.output,
                        workspace_file: args.workspace_file,
                    })?;
                }
            },
            ServiceCommand::Env { command } => match command {
                ServiceEnvCommand::List(args) => {
                    module::list_service_environments((&args).into())?;
                }
                ServiceEnvCommand::Add(args) => {
                    module::add_service_environment((&args).into())?;
                }
                ServiceEnvCommand::Remove(args) => {
                    module::remove_service_environment((&args).into())?;
                }
                ServiceEnvCommand::Verify(args) => {
                    module::verify_service_environment((&args).into())?;
                }
            },
            ServiceCommand::Deploy { command } => match command {
                ServiceDeployCommand::Export(args) => {
                    module::export_service_deployment((&args).into())?;
                }
                ServiceDeployCommand::Status(args) => {
                    module::status_service_deployment((&args).into())?;
                }
                ServiceDeployCommand::Wait(args) => {
                    module::wait_service_deployment((&args).into())?;
                }
            },
            ServiceCommand::Dev(args) => {
                service::dev_service((&args).into()).await?;
            }
            ServiceCommand::Package(args) => {
                service::package_service((&args).into()).await?;
            }
            ServiceCommand::Install(args) => {
                let mut options: module::ServiceModuleInstallOptions = (&args.install).into();
                let mut manifest_reference = args.install.manifest_reference.clone();
                if let Some(resolved) = service::resolve_workspace_install_reference(
                    &manifest_reference,
                    args.install.repo_root.as_deref(),
                    args.workspace_file.as_deref(),
                )? {
                    manifest_reference = resolved.manifest_reference;
                    if options.base_url.is_none() {
                        options.base_url = resolved.base_url;
                    }
                }
                if options.base_url.is_none() {
                    options.base_url = service::infer_workspace_base_url_for_manifest(
                        &manifest_reference,
                        args.install.repo_root.as_deref(),
                        args.workspace_file.as_deref(),
                    )?;
                }
                module::install_module(&manifest_reference, options).await?;
            }
            ServiceCommand::Uninstall(args) => {
                module::uninstall_service_module(&args.module_name, (&args).into()).await?;
            }
            ServiceCommand::Diff(args) => {
                module::diff_service((&args).into()).await?;
            }
            ServiceCommand::UpgradePlan(args) => {
                module::diff_service((&args).into()).await?;
            }
            ServiceCommand::Upgrade(args) => {
                module::upgrade_service((&args).into()).await?;
            }
            ServiceCommand::Rollback(args) => {
                module::rollback_service((&args).into()).await?;
            }
            ServiceCommand::Release { command } => match command {
                ServiceReleaseCommand::Plan(args) => {
                    module::plan_service_release((&args).into()).await?;
                }
                ServiceReleaseCommand::Check(args) => {
                    module::check_service_release_plan((&args).into())?;
                }
                ServiceReleaseCommand::Apply(args) => {
                    module::apply_service_release_plan((&args).into()).await?;
                }
                ServiceReleaseCommand::Promote(args) => {
                    module::promote_service_release((&args).into()).await?;
                }
                ServiceReleaseCommand::Rollback(args) => {
                    module::plan_service_release_rollback((&args).into()).await?;
                }
            },
            ServiceCommand::Policy { command } => match command {
                ServicePolicyCommand::Check(args) => {
                    module::policy_check_service_release_plan((&args).into())?;
                }
            },
            ServiceCommand::Delivery { command } => match command {
                ServiceDeliveryCommand::Assemble(args) => {
                    delivery::assemble_release(&args.input, args.output.as_deref())?;
                }
                ServiceDeliveryCommand::Check(args) => {
                    delivery::check_release(&args.artifact, args.output.as_deref())?;
                }
                ServiceDeliveryCommand::Diff(args) => {
                    delivery::diff_releases(&args.from, &args.to, args.output.as_deref())?;
                }
                ServiceDeliveryCommand::Policy(args) => {
                    delivery::check_policy_evidence(&args.artifact, args.output.as_deref())?;
                }
                ServiceDeliveryCommand::CanIDeploy(args) => {
                    delivery::can_i_deploy(&args.artifact, args.output.as_deref())?;
                }
                ServiceDeliveryCommand::DeploymentPlan(args) => {
                    delivery::check_deployment_plan(&args.artifact, args.output.as_deref())?;
                }
                ServiceDeliveryCommand::OperatorExport(args) => {
                    delivery::export_operator_resource(
                        &args.deployment_plan,
                        args.previous.as_deref(),
                        args.output.as_deref(),
                    )?;
                }
                ServiceDeliveryCommand::PromotionApply(args) => {
                    delivery::authorize_promotion_apply(
                        &args.promotion_plan,
                        &args.approval,
                        &args.protected_evidence,
                        &args.environment_verification,
                        &args.source_observation,
                        &args.source_gateway_observation,
                        &args.target_observation,
                        &args.operator_export,
                        args.output.as_deref(),
                    )?;
                }
            },
            ServiceCommand::Doctor(args) => {
                module::doctor_module((&args).into()).await?;
            }
            ServiceCommand::Check(args) => {
                run_service_check_or_doctor(&args, false).await?;
            }
            ServiceCommand::Verify(args) => {
                run_service_check_or_doctor(&args, true).await?;
            }
            ServiceCommand::List(args) => {
                module::list_module_services((&args).into()).await?;
            }
            ServiceCommand::Export(args) => {
                module::export_module_services((&args).into()).await?;
            }
            ServiceCommand::Status(args) => {
                module::status_module_service((&args).into()).await?;
            }
            ServiceCommand::Logs(args) => {
                module::logs_module_service((&args).into()).await?;
            }
            ServiceCommand::Start(args) => {
                module::start_module_service((&args).into()).await?;
            }
            ServiceCommand::Stop(args) => {
                module::stop_module_service((&args).into()).await?;
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    fn subcommand_help(name: &str) -> String {
        let mut command = Cli::command();
        command
            .find_subcommand_mut(name)
            .unwrap_or_else(|| panic!("missing `{name}` command"))
            .render_long_help()
            .to_string()
    }

    #[test]
    fn public_app_help_exposes_compose_without_retired_lifecycle() {
        let help = subcommand_help("app");

        assert!(help.contains("  compose"));
        for hidden in ["  create", "  list", "  inspect", "  add"] {
            assert!(
                !help.contains(hidden),
                "app help still advertises `{}`:\n{help}",
                hidden.trim()
            );
        }
        for retired in [
            "  plan",
            "  upgrade",
            "  apply",
            "  next",
            "  explain",
            "  verify",
            "  diff",
            "  repair",
        ] {
            assert!(
                !help.contains(retired),
                "app help still exposes `{}`:\n{help}",
                retired.trim()
            );
        }
        assert!(!help.contains("Launchpad"));

        for retired in [
            "plan", "upgrade", "apply", "next", "explain", "verify", "diff", "repair",
        ] {
            assert!(Cli::try_parse_from(["lenso", "app", retired]).is_err());
        }
        assert!(Cli::try_parse_from(["lenso", "agent", "context", "--from-app-plan"]).is_err());
        assert!(
            Cli::try_parse_from([
                "lenso",
                "agent",
                "task",
                "inspect the app",
                "--from-app-plan",
            ])
            .is_err()
        );
    }

    #[test]
    fn app_compose_keeps_atomic_materialization_without_legacy_plan_flags() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("app")
            .expect("app command")
            .find_subcommand_mut("compose")
            .expect("app compose command")
            .render_long_help()
            .to_string();

        assert!(help.contains("--apply"));
        assert!(help.contains("Atomically materialize"));
        for retired in ["--addon", "--write-plan", "--explain", "Launchpad"] {
            assert!(
                !help.contains(retired),
                "app compose help still exposes `{retired}`:\n{help}"
            );
        }

        let cli = Cli::try_parse_from([
            "lenso",
            "app",
            "compose",
            "./support-desk",
            "--blueprint",
            "support-desk",
            "--pack",
            "./fixtures/support-desk",
            "--implementation",
            "support-api=linked",
            "--apply",
        ])
        .expect("public compose command");
        let Command::App {
            command: AppCommand::Compose(args),
        } = cli.command
        else {
            panic!("expected app compose");
        };
        assert!(args.apply);
        assert_eq!(args.packs.len(), 1);
        assert_eq!(args.implementations, vec!["support-api=linked"]);

        for retired in ["--addon", "--write-plan", "--explain"] {
            assert!(
                Cli::try_parse_from(["lenso", "app", "compose", "./support-desk", retired,])
                    .is_err()
            );
        }
    }

    #[test]
    fn public_system_help_exposes_local_run_and_validation_only() {
        let help = subcommand_help("system");

        assert!(help.contains("  dev"));
        assert!(help.contains("  check"));
        for retired in [
            "  init",
            "  add-service",
            "  add-module",
            "  plan",
            "  diff",
            "  apply",
            "  doctor",
            "  release",
            "  runbook",
            "  graph",
        ] {
            assert!(
                !help.contains(retired),
                "system help still exposes `{}`:\n{help}",
                retired.trim()
            );
        }

        for retired in [
            "init",
            "add-service",
            "add-module",
            "plan",
            "diff",
            "apply",
            "doctor",
            "release",
            "runbook",
            "graph",
        ] {
            assert!(Cli::try_parse_from(["lenso", "system", retired]).is_err());
        }

        let mut command = Cli::command();
        let dev_help = command
            .find_subcommand_mut("system")
            .expect("system command")
            .find_subcommand_mut("dev")
            .expect("system dev command")
            .render_long_help()
            .to_string();
        assert!(dev_help.contains("exact local launch preview"));
        assert!(!dev_help.contains("exact plan"));
    }

    #[test]
    fn public_dev_help_exposes_status_only() {
        let help = subcommand_help("dev");

        assert!(help.contains("  status"));
        for retired in ["  up", "  doctor", "  stop"] {
            assert!(
                !help.contains(retired),
                "dev help still exposes `{}`:\n{help}",
                retired.trim()
            );
        }
        assert!(!help.contains("Launchpad"));
    }

    #[test]
    fn parses_service_create_ts() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "create",
            "support-suite-provider",
            "--lang",
            "ts",
            "--port",
            "4110",
            "--workspace-file",
            "lenso.workspace.json",
        ]);

        let Command::Service {
            command: ServiceCommand::Create(args),
        } = cli.command
        else {
            panic!("expected service create");
        };

        assert_eq!(args.name, "support-suite-provider");
        assert_eq!(args.lang, ServiceLanguage::Ts);
        assert_eq!(args.port, 4110);
        assert_eq!(
            args.workspace_file.as_deref(),
            Some(std::path::Path::new("lenso.workspace.json"))
        );
    }

    #[test]
    fn parses_app_command_create_support_desk() {
        let cli = Cli::parse_from([
            "lenso",
            "app",
            "create",
            "support-desk",
            "--blueprint",
            "support-desk",
        ]);
        let Command::App {
            command: AppCommand::Create(args),
        } = cli.command
        else {
            panic!("expected app create");
        };

        assert_eq!(args.dir, std::path::PathBuf::from("support-desk"));
        assert_eq!(args.blueprint, "support-desk");
    }

    #[test]
    fn parses_app_list() {
        let cli = Cli::parse_from(["lenso", "app", "list"]);
        let Command::App {
            command: AppCommand::List,
        } = cli.command
        else {
            panic!("expected app list");
        };
    }

    #[test]
    fn parses_app_inspect() {
        let cli = Cli::parse_from(["lenso", "app", "inspect", "support-desk"]);
        let Command::App {
            command: AppCommand::Inspect(args),
        } = cli.command
        else {
            panic!("expected app inspect");
        };

        assert_eq!(args.blueprint, "support-desk");
    }

    #[test]
    fn parses_app_add() {
        let cli = Cli::parse_from([
            "lenso",
            "app",
            "add",
            "support-sla",
            "--observed-revision",
            "4",
        ]);
        let Command::App {
            command: AppCommand::Add(args),
        } = cli.command
        else {
            panic!("expected app add");
        };

        assert_eq!(args.addon, "support-sla");
        assert_eq!(args.observed_revision, Some(4));
    }

    #[test]
    fn parses_agent_task_for_module() {
        let cli = Cli::parse_from([
            "lenso",
            "agent",
            "task",
            "--for-module",
            "support-ticket",
            "add private notes",
        ]);
        let Command::Agent {
            command: AgentCommand::Task(args),
        } = cli.command
        else {
            panic!("expected agent task");
        };

        assert_eq!(args.for_module, Some("support-ticket".to_owned()));
    }

    #[test]
    fn parses_agent_task_for_capability() {
        let cli = Cli::parse_from([
            "lenso",
            "agent",
            "task",
            "--for-capability",
            "support-sla",
            "add enterprise escalation",
        ]);
        let Command::Agent {
            command: AgentCommand::Task(args),
        } = cli.command
        else {
            panic!("expected agent task");
        };

        assert_eq!(args.for_capability.as_deref(), Some("support-sla"));
    }

    #[test]
    fn parses_dev_command_status() {
        let cli = Cli::parse_from(["lenso", "dev", "status", "--repo-root", "support-desk"]);
        let Command::Dev {
            command: DevCommand::Status(args),
        } = cli.command
        else {
            panic!("expected dev status");
        };

        assert_eq!(
            args.repo_root.as_deref(),
            Some(std::path::Path::new("support-desk"))
        );
    }

    #[test]
    fn parses_dev_doctor() {
        let cli = Cli::parse_from(["lenso", "dev", "doctor", "--live", "--write-state"]);
        let Command::Dev {
            command: DevCommand::Doctor(args),
        } = cli.command
        else {
            panic!("expected dev doctor");
        };

        assert!(args.live);
        assert!(args.write_state);
    }

    #[test]
    fn parses_console_dev() {
        let cli = Cli::parse_from([
            "lenso",
            "console",
            "dev",
            "--console-root",
            "../lenso-console",
        ]);
        let Command::Console {
            command: ConsoleCommand::Dev(args),
        } = cli.command
        else {
            panic!("expected console dev");
        };

        assert_eq!(
            args.console_root.as_deref(),
            Some(std::path::Path::new("../lenso-console"))
        );
        assert!(Cli::try_parse_from(["lenso", "console", "package"]).is_err());
    }

    #[test]
    fn parses_console_operator_bootstrap_without_legacy_alias() {
        let cli = Cli::parse_from([
            "lenso",
            "console",
            "operator",
            "bootstrap",
            "--console-root",
            "../lenso-console",
            "--identifier",
            "admin@example.com",
            "--console-url",
            "http://127.0.0.1:3030",
            "--password-file",
            "./operator-password",
            "--scope",
            "runtime.stories.read",
        ]);
        let Command::Console {
            command:
                ConsoleCommand::Operator {
                    command: ConsoleOperatorCommand::Bootstrap(args),
                },
        } = cli.command
        else {
            panic!("expected Console operator bootstrap");
        };

        assert_eq!(
            args.console_root.as_deref(),
            Some(std::path::Path::new("../lenso-console"))
        );
        assert_eq!(args.identifier.as_deref(), Some("admin@example.com"));
        assert_eq!(
            args.password_file.as_deref(),
            Some(std::path::Path::new("./operator-password"))
        );
        assert_eq!(args.console_url.as_deref(), Some("http://127.0.0.1:3030"));
        assert_eq!(args.scopes, ["runtime.stories.read"]);
        assert!(
            Cli::try_parse_from([
                "lenso",
                "console",
                "bootstrap-admin",
                "--identifier",
                "admin@example.com",
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["lenso", "console", "update"]).is_err());
        assert!(Cli::try_parse_from(["lenso", "console", "recovery"]).is_err());
    }

    #[test]
    fn parses_console_composition_plan_and_apply() {
        let plan = Cli::parse_from([
            "lenso",
            "console",
            "composition",
            "plan",
            "--composition",
            "composition.json",
            "--env-file",
            "console.env",
            "--output",
            "plan.json",
        ]);
        let Command::Console {
            command:
                ConsoleCommand::Composition {
                    command: ConsoleCompositionCommand::Plan(args),
                },
        } = plan.command
        else {
            panic!("expected Console composition plan");
        };
        assert_eq!(args.composition, std::path::Path::new("composition.json"));
        assert_eq!(
            args.output.as_deref(),
            Some(std::path::Path::new("plan.json"))
        );

        let apply = Cli::parse_from([
            "lenso",
            "console",
            "composition",
            "apply",
            "--plan",
            "plan.json",
            "--env-file",
            "console.env",
            "--approve-plan-digest",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]);
        let Command::Console {
            command:
                ConsoleCommand::Composition {
                    command: ConsoleCompositionCommand::Apply(args),
                },
        } = apply.command
        else {
            panic!("expected Console composition apply");
        };
        assert_eq!(args.plan, std::path::Path::new("plan.json"));
    }

    #[test]
    fn parses_console_installation_authority_commands() {
        let install = Cli::parse_from([
            "lenso",
            "console",
            "install",
            "--manifest",
            "release.json",
            "--root",
            "/srv/lenso-console",
            "--output",
            "plan.json",
        ]);
        let Command::Console {
            command: ConsoleCommand::Install(args),
        } = install.command
        else {
            panic!("expected Console install");
        };
        assert_eq!(args.manifest, std::path::Path::new("release.json"));
        assert_eq!(args.root, std::path::Path::new("/srv/lenso-console"));
        assert_eq!(
            args.output.as_deref(),
            Some(std::path::Path::new("plan.json"))
        );
        assert!(!args.apply);

        let upgrade = Cli::parse_from([
            "lenso",
            "console",
            "upgrade",
            "--manifest",
            "release.json",
            "--env-file",
            "console.env",
            "--apply",
            "--approve-plan-digest",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--approve-irreversible",
        ]);
        let Command::Console {
            command: ConsoleCommand::Upgrade(args),
        } = upgrade.command
        else {
            panic!("expected Console upgrade");
        };
        assert!(args.apply);
        assert!(args.approve_irreversible);
        assert_eq!(
            args.env_file.as_deref(),
            Some(std::path::Path::new("console.env"))
        );

        let doctor = Cli::parse_from([
            "lenso",
            "console",
            "doctor",
            "--root",
            "/srv/lenso-console",
            "--live-url",
            "https://console.example.com",
            "--json",
        ]);
        let Command::Console {
            command: ConsoleCommand::Doctor(args),
        } = doctor.command
        else {
            panic!("expected Console doctor");
        };
        assert_eq!(
            args.live_url.as_deref(),
            Some("https://console.example.com")
        );
        assert!(args.json);

        let restore = Cli::parse_from([
            "lenso",
            "console",
            "restore",
            "--root",
            "/srv/lenso-console",
            "--recovery-set",
            "recovery-set",
            "--current-env-file",
            "current.env",
            "--recovery-env-file",
            "recovery.env",
            "--apply",
            "--approve-plan-digest",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--identity-file",
            "age-key.txt",
        ]);
        let Command::Console {
            command: ConsoleCommand::Restore(args),
        } = restore.command
        else {
            panic!("expected Console restore");
        };
        assert!(args.apply);
        assert_eq!(args.recovery_set, std::path::Path::new("recovery-set"));
        assert_eq!(
            args.identity_file.as_deref(),
            Some(std::path::Path::new("age-key.txt"))
        );

        let backup = Cli::parse_from([
            "lenso",
            "console",
            "backup",
            "--root",
            "/srv/lenso-console",
            "--env-file",
            "console.env",
            "--output",
            "recovery-set",
            "--recipient",
            "age1example",
            "--json",
        ]);
        let Command::Console {
            command: ConsoleCommand::Backup(args),
        } = backup.command
        else {
            panic!("expected Console backup");
        };
        assert_eq!(args.output, std::path::Path::new("recovery-set"));
        assert_eq!(args.recipient, "age1example");
        assert!(args.json);

        assert!(
            Cli::try_parse_from([
                "lenso",
                "console",
                "install",
                "--manifest",
                "release.json",
                "--approve-plan-digest",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_module_dev_console_ui() {
        let cli = Cli::parse_from([
            "lenso",
            "module",
            "dev",
            "--console-ui",
            "--repo-root",
            "./module-repo",
        ]);
        let Command::Module {
            command: ModuleCommand::Dev(args),
        } = cli.command
        else {
            panic!("expected module dev");
        };

        assert!(args.console_ui);
        assert_eq!(
            args.repo_root.as_deref(),
            Some(std::path::Path::new("./module-repo"))
        );
        assert!(Cli::try_parse_from(["lenso", "module", "dev", "--console"]).is_err());
    }

    #[test]
    fn parses_agent_command_context() {
        let cli = Cli::parse_from(["lenso", "agent", "context", "--output", "AGENT_CONTEXT.md"]);
        let Command::Agent {
            command: AgentCommand::Context(args),
        } = cli.command
        else {
            panic!("expected agent context");
        };

        assert_eq!(
            args.output.as_deref(),
            Some(std::path::Path::new("AGENT_CONTEXT.md"))
        );
    }

    #[test]
    fn parses_capability_init_ts() {
        let cli = Cli::parse_from([
            "lenso",
            "capability",
            "init",
            "support-sla",
            "--dir",
            "./capabilities/support-sla",
            "--lang",
            "ts",
            "--for-blueprint",
            "support-desk",
        ]);
        let Command::Capability { command } = cli.command else {
            panic!("expected capability command");
        };
        let CapabilityCommand::Init(args) = command else {
            panic!("expected capability init");
        };

        assert_eq!(args.name, "support-sla");
        assert_eq!(args.lang, "ts");
        assert_eq!(args.for_blueprint, vec!["support-desk"]);
    }

    #[test]
    fn parses_capability_check_json() {
        let cli = Cli::parse_from([
            "lenso",
            "capability",
            "check",
            "./capabilities/support-sla",
            "--json",
        ]);
        let Command::Capability { command } = cli.command else {
            panic!("expected capability command");
        };
        let CapabilityCommand::Check(args) = command else {
            panic!("expected capability check");
        };

        assert_eq!(
            args.path,
            std::path::PathBuf::from("./capabilities/support-sla")
        );
        assert!(args.json);
    }

    #[test]
    fn parses_capability_library_add() {
        let cli = Cli::parse_from([
            "lenso",
            "capability",
            "library",
            "add",
            "./capabilities/support-sla",
            "--repo-root",
            "./acme-support",
        ]);
        let Command::Capability { command } = cli.command else {
            panic!("expected capability command");
        };
        let CapabilityCommand::Library {
            command: CapabilityLibraryCommand::Add(args),
        } = command
        else {
            panic!("expected capability library add");
        };

        assert_eq!(
            args.path,
            std::path::PathBuf::from("./capabilities/support-sla")
        );
        assert_eq!(
            args.repo_root.as_deref(),
            Some(std::path::Path::new("./acme-support"))
        );
    }

    #[test]
    fn parses_capability_library_list_json() {
        let cli = Cli::parse_from(["lenso", "capability", "library", "list", "--json"]);
        let Command::Capability { command } = cli.command else {
            panic!("expected capability command");
        };
        let CapabilityCommand::Library {
            command: CapabilityLibraryCommand::List(args),
        } = command
        else {
            panic!("expected capability library list");
        };

        assert!(args.json);
    }

    #[test]
    fn parses_capability_fit_json() {
        let cli = Cli::parse_from([
            "lenso",
            "capability",
            "fit",
            "support-sla",
            "--repo-root",
            ".",
            "--json",
        ]);
        let Command::Capability { command } = cli.command else {
            panic!("expected capability command");
        };
        let CapabilityCommand::Fit(args) = command else {
            panic!("expected capability fit");
        };

        assert_eq!(args.pack, std::path::PathBuf::from("support-sla"));
        assert!(args.json);
    }

    #[test]
    fn parses_service_create_rust() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "create",
            "rust-audit-service",
            "--lang",
            "rust",
        ]);

        let Command::Service {
            command: ServiceCommand::Create(args),
        } = cli.command
        else {
            panic!("expected service create");
        };

        assert_eq!(args.name, "rust-audit-service");
        assert_eq!(args.lang, ServiceLanguage::Rust);
    }

    #[test]
    fn parses_service_dev() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "dev",
            "--skip-db",
            "--workspace-file",
            "services.json",
        ]);
        let Command::Service {
            command: ServiceCommand::Dev(args),
        } = cli.command
        else {
            panic!("expected service dev");
        };

        assert!(args.skip_db);
        assert_eq!(
            args.workspace_file.as_deref(),
            Some(std::path::Path::new("services.json"))
        );
    }

    #[test]
    fn parses_service_workspace_add() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "workspace",
            "add",
            "support-suite-provider",
            "--cwd",
            "services/support-suite-provider",
            "--lang",
            "ts",
            "--command",
            "pnpm start",
            "--ready-url",
            "http://127.0.0.1:4110/lenso/service/v1/status",
            "--module",
            "support-ticket",
        ]);

        let Command::Service {
            command:
                ServiceCommand::Workspace {
                    command: ServiceWorkspaceCommand::Add(args),
                },
        } = cli.command
        else {
            panic!("expected service workspace add");
        };

        assert_eq!(args.name, "support-suite-provider");
        assert_eq!(args.lang, ServiceLanguage::Ts);
        assert_eq!(args.modules, ["support-ticket"]);
    }

    #[test]
    fn parses_service_workspace_check() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "workspace",
            "check",
            "support-suite-provider",
            "--workspace-file",
            ".lenso/services.json",
            "--json",
        ]);

        let Command::Service {
            command:
                ServiceCommand::Workspace {
                    command: ServiceWorkspaceCommand::Check(args),
                },
        } = cli.command
        else {
            panic!("expected service workspace check");
        };

        assert_eq!(args.service_name.as_deref(), Some("support-suite-provider"));
        assert_eq!(
            args.workspace_file.as_deref(),
            Some(std::path::Path::new(".lenso/services.json"))
        );
        assert!(args.json);
    }

    #[test]
    fn parses_service_workspace_export() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "workspace",
            "export",
            "--workspace-file",
            "lenso.workspace.json",
            "--output",
            ".lenso/module-services.json",
        ]);

        let Command::Service {
            command:
                ServiceCommand::Workspace {
                    command: ServiceWorkspaceCommand::Export(args),
                },
        } = cli.command
        else {
            panic!("expected service workspace export");
        };

        assert_eq!(
            args.workspace_file.as_deref(),
            Some(std::path::Path::new("lenso.workspace.json"))
        );
        assert_eq!(
            args.output.as_deref(),
            Some(std::path::Path::new(".lenso/module-services.json"))
        );
    }

    #[test]
    fn parses_service_env_add() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "env",
            "add",
            "staging",
            "--service",
            "support-suite-provider",
            "--target",
            "kubernetes",
            "--namespace",
            "lenso-staging",
            "--image",
            "ghcr.io/acme/support-suite-provider:0.4.0",
            "--public-base-url",
            "https://support-staging.example.com",
            "--replicas",
            "2",
            "--port",
            "4110",
        ]);

        let Command::Service {
            command:
                ServiceCommand::Env {
                    command: ServiceEnvCommand::Add(args),
                },
        } = cli.command
        else {
            panic!("expected service env add");
        };

        assert_eq!(args.name, "staging");
        assert_eq!(args.service_name, "support-suite-provider");
        assert_eq!(args.target, ServiceDeploymentTargetArg::Kubernetes);
        assert_eq!(args.namespace.as_deref(), Some("lenso-staging"));
        assert_eq!(args.replicas, Some(2));
        assert_eq!(args.port, Some(4110));
    }

    #[test]
    fn parses_service_deploy_commands() {
        let export = Cli::parse_from([
            "lenso",
            "service",
            "deploy",
            "export",
            "support-suite-provider",
            "--env",
            "staging",
            "--target",
            "kubernetes",
            "--output-dir",
            "dist/kubernetes/staging",
            "--image",
            "ghcr.io/acme/support-suite-provider:0.4.0",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Deploy {
                    command: ServiceDeployCommand::Export(export_args),
                },
        } = export.command
        else {
            panic!("expected service deploy export");
        };
        assert_eq!(export_args.service_name, "support-suite-provider");
        assert_eq!(export_args.environment_name, "staging");
        assert_eq!(export_args.target, ServiceDeploymentTargetArg::Kubernetes);

        let status = Cli::parse_from([
            "lenso",
            "service",
            "deploy",
            "status",
            "support-suite-provider",
            "--env",
            "staging",
            "--from-file",
            "deployment.json",
            "--write-state",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Deploy {
                    command: ServiceDeployCommand::Status(status_args),
                },
        } = status.command
        else {
            panic!("expected service deploy status");
        };
        assert_eq!(status_args.environment_name, "staging");
        assert!(status_args.write_state);

        let wait = Cli::parse_from([
            "lenso",
            "service",
            "deploy",
            "wait",
            "support-suite-provider",
            "--env",
            "staging",
            "--source",
            "kubernetes",
            "--timeout-seconds",
            "30",
            "--interval-seconds",
            "2",
            "--write-state",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Deploy {
                    command: ServiceDeployCommand::Wait(wait_args),
                },
        } = wait.command
        else {
            panic!("expected service deploy wait");
        };
        assert_eq!(wait_args.environment_name, "staging");
        assert_eq!(wait_args.timeout_seconds, 30);
        assert_eq!(wait_args.interval_seconds, 2);
        assert!(wait_args.write_state);
    }

    #[test]
    fn parses_operator_export_crd() {
        let cli = Cli::parse_from([
            "lenso",
            "operator",
            "export-crd",
            "--output",
            "dist/lenso-operator/crds",
            "--namespace",
            "lenso-system",
        ]);

        let Command::Operator {
            command: OperatorCommand::ExportCrd(args),
        } = cli.command
        else {
            panic!("expected operator export-crd");
        };

        assert_eq!(
            args.output,
            std::path::PathBuf::from("dist/lenso-operator/crds")
        );
        assert_eq!(args.namespace, "lenso-system");
    }

    #[test]
    fn parses_service_deploy_operator_target_and_source() {
        let export = Cli::parse_from([
            "lenso",
            "service",
            "deploy",
            "export",
            "support-suite-provider",
            "--env",
            "staging",
            "--target",
            "operator",
            "--output-dir",
            "dist/operator/staging",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Deploy {
                    command: ServiceDeployCommand::Export(export_args),
                },
        } = export.command
        else {
            panic!("expected service deploy export");
        };
        assert_eq!(export_args.target, ServiceDeploymentTargetArg::Operator);

        let status = Cli::parse_from([
            "lenso",
            "service",
            "deploy",
            "status",
            "support-suite-provider",
            "--env",
            "staging",
            "--source",
            "operator",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Deploy {
                    command: ServiceDeployCommand::Status(status_args),
                },
        } = status.command
        else {
            panic!("expected service deploy status");
        };
        assert_eq!(status_args.source, ServiceDeploymentSourceArg::Operator);
    }

    #[test]
    fn parses_service_install_workspace_file() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "install",
            "./services/support-suite-provider/lenso.service.json",
            "--workspace-file",
            ".lenso/services.json",
        ]);

        let Command::Service {
            command: ServiceCommand::Install(args),
        } = cli.command
        else {
            panic!("expected service install");
        };

        assert_eq!(
            args.workspace_file.as_deref(),
            Some(std::path::Path::new(".lenso/services.json"))
        );
    }

    #[test]
    fn parses_module_disable_and_rejects_removed_aliases() {
        let cli = Cli::parse_from(["lenso", "module", "disable", "support-ticket"]);
        let Command::Module {
            command: ModuleCommand::Disable(disable_args),
        } = cli.command
        else {
            panic!("expected module disable");
        };
        assert_eq!(disable_args.module_name, "support-ticket");
        assert!(Cli::try_parse_from(["lenso", "module", "enable", "support-ticket"]).is_err());
        assert!(Cli::try_parse_from(["lenso", "module", "add", "support-ticket"]).is_err());
        assert!(Cli::try_parse_from(["lenso", "module", "uninstall", "support-ticket"]).is_err());
    }

    #[test]
    fn parses_service_package() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "package",
            "../services/support-suite-provider",
            "--manifest",
            "service.json",
            "--output-dir",
            "../dist/services",
            "--check",
            "--json",
        ]);
        let Command::Service {
            command: ServiceCommand::Package(args),
        } = cli.command
        else {
            panic!("expected service package");
        };

        assert_eq!(
            args.service_dir.as_path(),
            std::path::Path::new("../services/support-suite-provider")
        );
        assert_eq!(args.manifest, "service.json");
        assert_eq!(
            args.output_dir.as_path(),
            std::path::Path::new("../dist/services")
        );
        assert!(args.check);
        assert!(args.json);
    }

    #[test]
    fn parses_service_check_manifest_reference() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "check",
            "./lenso.service.json",
            "--json",
            "--serve-command",
            "pnpm start",
        ]);
        let Command::Service {
            command: ServiceCommand::Check(args),
        } = cli.command
        else {
            panic!("expected service check");
        };

        assert_eq!(
            args.manifest_reference.as_deref(),
            Some("./lenso.service.json")
        );
        assert!(args.json);
        assert_eq!(args.serve_command.as_deref(), Some("pnpm start"));
    }

    #[test]
    fn parses_service_verify_manifest_reference() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "verify",
            "./lenso.service.json",
            "--json",
            "--serve-command",
            "pnpm start",
        ]);
        let Command::Service {
            command: ServiceCommand::Verify(args),
        } = cli.command
        else {
            panic!("expected service verify");
        };

        assert_eq!(
            args.manifest_reference.as_deref(),
            Some("./lenso.service.json")
        );
        assert!(args.json);
        assert_eq!(args.serve_command.as_deref(), Some("pnpm start"));
        assert!(service_verify_uses_manifest(&args));
    }

    #[test]
    fn parses_service_check_operation_filter_and_sample_input() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "check",
            "./lenso.service.json",
            "--operation",
            "support-ticket/http/GET:/tickets",
            "--sample-input",
            "fixtures/probe.json",
        ]);
        let Command::Service {
            command: ServiceCommand::Check(args),
        } = cli.command
        else {
            panic!("expected service check");
        };

        assert_eq!(
            args.operation.as_deref(),
            Some("support-ticket/http/GET:/tickets")
        );
        assert_eq!(
            args.sample_input.as_deref(),
            Some(std::path::Path::new("fixtures/probe.json"))
        );
    }

    #[test]
    fn service_check_operation_options_use_manifest_check_mode() {
        let cli = Cli::parse_from(["lenso", "service", "check", "--operation", "missing"]);
        let Command::Service {
            command: ServiceCommand::Check(args),
        } = cli.command
        else {
            panic!("expected service check");
        };

        assert!(service_check_uses_manifest(&args));
        assert_eq!(args.manifest_reference.as_deref(), None);

        let cli = Cli::parse_from([
            "lenso",
            "service",
            "check",
            "--sample-input",
            "fixtures/probe.json",
        ]);
        let Command::Service {
            command: ServiceCommand::Check(args),
        } = cli.command
        else {
            panic!("expected service check");
        };

        assert!(service_check_uses_manifest(&args));
        assert_eq!(args.manifest_reference.as_deref(), None);
    }

    #[test]
    fn service_verify_defaults_to_manifest_but_accepts_provider_name() {
        let cli = Cli::parse_from(["lenso", "service", "verify"]);
        let Command::Service {
            command: ServiceCommand::Verify(args),
        } = cli.command
        else {
            panic!("expected service verify");
        };
        assert!(service_verify_uses_manifest(&args));

        let cli = Cli::parse_from(["lenso", "service", "verify", "support-ticket"]);
        let Command::Service {
            command: ServiceCommand::Verify(args),
        } = cli.command
        else {
            panic!("expected service verify");
        };
        assert!(!service_verify_uses_manifest(&args));
    }

    #[test]
    fn parses_service_delivery_lifecycle_commands() {
        let diff = Cli::parse_from([
            "lenso",
            "service",
            "diff",
            "support-suite-provider",
            "./lenso.service.json",
        ]);
        let Command::Service {
            command: ServiceCommand::Diff(diff_args),
        } = diff.command
        else {
            panic!("expected service diff");
        };
        assert_eq!(diff_args.service_name, "support-suite-provider");

        let upgrade_plan = Cli::parse_from([
            "lenso",
            "service",
            "upgrade-plan",
            "support-suite-provider",
            "./lenso.service.json",
            "--json",
        ]);
        let Command::Service {
            command: ServiceCommand::UpgradePlan(upgrade_plan_args),
        } = upgrade_plan.command
        else {
            panic!("expected service upgrade-plan");
        };
        assert_eq!(upgrade_plan_args.service_name, "support-suite-provider");
        assert!(upgrade_plan_args.json);

        let upgrade = Cli::parse_from([
            "lenso",
            "service",
            "upgrade",
            "support-suite-provider",
            "./lenso.service.json",
            "--dry-run",
        ]);
        let Command::Service {
            command: ServiceCommand::Upgrade(upgrade_args),
        } = upgrade.command
        else {
            panic!("expected service upgrade");
        };
        assert!(upgrade_args.dry_run);

        let rollback = Cli::parse_from([
            "lenso",
            "service",
            "rollback",
            "support-suite-provider",
            "--dry-run",
        ]);
        let Command::Service {
            command: ServiceCommand::Rollback(rollback_args),
        } = rollback.command
        else {
            panic!("expected service rollback");
        };
        assert!(rollback_args.dry_run);
    }

    #[test]
    fn parses_service_release_and_policy_commands() {
        let plan = Cli::parse_from([
            "lenso",
            "service",
            "release",
            "plan",
            "support-suite-provider",
            "./lenso.service-package.json",
            "--output",
            ".lenso/releases/support.plan.json",
            "--env",
            "staging",
            "--fail-on",
            "breaking",
            "--json",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Release {
                    command: ServiceReleaseCommand::Plan(plan_args),
                },
        } = plan.command
        else {
            panic!("expected service release plan");
        };
        assert_eq!(plan_args.service_name, "support-suite-provider");
        assert_eq!(plan_args.environment_name.as_deref(), Some("staging"));
        assert_eq!(plan_args.fail_on.as_deref(), Some("breaking"));
        assert!(plan_args.json);

        let check = Cli::parse_from([
            "lenso",
            "service",
            "release",
            "check",
            ".lenso/releases/support.plan.json",
            "--fail-on",
            "needs_attention",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Release {
                    command: ServiceReleaseCommand::Check(check_args),
                },
        } = check.command
        else {
            panic!("expected service release check");
        };
        assert_eq!(check_args.fail_on.as_deref(), Some("needs_attention"));

        let apply = Cli::parse_from([
            "lenso",
            "service",
            "release",
            "apply",
            ".lenso/releases/support.plan.json",
            "--env",
            "staging",
            "--dry-run",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Release {
                    command: ServiceReleaseCommand::Apply(apply_args),
                },
        } = apply.command
        else {
            panic!("expected service release apply");
        };
        assert!(apply_args.dry_run);
        assert_eq!(apply_args.environment_name.as_deref(), Some("staging"));

        let promote = Cli::parse_from([
            "lenso",
            "service",
            "release",
            "promote",
            "support-suite-provider",
            "--from",
            "staging",
            "--to",
            "prod",
            "--output",
            ".lenso/releases/support.prod.plan.json",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Release {
                    command: ServiceReleaseCommand::Promote(promote_args),
                },
        } = promote.command
        else {
            panic!("expected service release promote");
        };
        assert_eq!(promote_args.from_environment, "staging");
        assert_eq!(promote_args.to_environment, "prod");

        let rollback = Cli::parse_from([
            "lenso",
            "service",
            "release",
            "rollback",
            "support-suite-provider",
            "--env",
            "prod",
            "--to",
            "rel_1",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Release {
                    command: ServiceReleaseCommand::Rollback(rollback_args),
                },
        } = rollback.command
        else {
            panic!("expected service release rollback");
        };
        assert_eq!(rollback_args.environment_name, "prod");
        assert_eq!(rollback_args.release_id.as_deref(), Some("rel_1"));

        let policy = Cli::parse_from([
            "lenso",
            "service",
            "policy",
            "check",
            ".lenso/releases/support.plan.json",
            "--json",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Policy {
                    command: ServicePolicyCommand::Check(policy_args),
                },
        } = policy.command
        else {
            panic!("expected service policy check");
        };
        assert!(policy_args.json);
    }

    #[test]
    fn parses_autonomous_service_delivery_commands() {
        let assemble = Cli::parse_from([
            "lenso",
            "service",
            "delivery",
            "assemble",
            "release-input.json",
            "--output",
            "release.json",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Delivery {
                    command: ServiceDeliveryCommand::Assemble(args),
                },
        } = assemble.command
        else {
            panic!("expected service delivery assemble");
        };
        assert_eq!(args.input, std::path::PathBuf::from("release-input.json"));
        assert_eq!(args.output, Some(std::path::PathBuf::from("release.json")));

        let export = Cli::parse_from([
            "lenso",
            "service",
            "delivery",
            "operator-export",
            "production.deployment-plan.json",
            "--previous",
            "previous-export.json",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Delivery {
                    command: ServiceDeliveryCommand::OperatorExport(args),
                },
        } = export.command
        else {
            panic!("expected service delivery operator export");
        };
        assert_eq!(
            args.deployment_plan,
            std::path::PathBuf::from("production.deployment-plan.json")
        );
        assert_eq!(
            args.previous,
            Some(std::path::PathBuf::from("previous-export.json"))
        );

        let promotion_apply = Cli::parse_from([
            "lenso",
            "service",
            "delivery",
            "promotion-apply",
            "promotion-plan.json",
            "approval.json",
            "protected-evidence.json",
            "environment-verification.json",
            "source-observation.json",
            "source-gateway-observation.json",
            "target-observation.json",
            "operator-export.json",
            "--output",
            "authorization.json",
        ]);
        let Command::Service {
            command:
                ServiceCommand::Delivery {
                    command: ServiceDeliveryCommand::PromotionApply(args),
                },
        } = promotion_apply.command
        else {
            panic!("expected service delivery promotion apply");
        };
        assert_eq!(
            args.promotion_plan,
            std::path::PathBuf::from("promotion-plan.json")
        );
        assert_eq!(
            args.source_observation,
            std::path::PathBuf::from("source-observation.json")
        );
        assert_eq!(
            args.source_gateway_observation,
            std::path::PathBuf::from("source-gateway-observation.json")
        );
        assert_eq!(
            args.target_observation,
            std::path::PathBuf::from("target-observation.json")
        );
        assert_eq!(
            args.output,
            Some(std::path::PathBuf::from("authorization.json"))
        );
    }

    #[test]
    fn parses_service_logs() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "logs",
            "support-ticket",
            "api",
            "--tail",
            "100",
        ]);
        let Command::Service {
            command: ServiceCommand::Logs(args),
        } = cli.command
        else {
            panic!("expected service logs");
        };

        assert_eq!(args.module_name, "support-ticket");
        assert_eq!(args.service_name, "api");
        assert_eq!(args.tail, 100);
    }

    #[test]
    fn parses_system_dev_dry_run_and_cleanup_surfaces() {
        let cli = Cli::parse_from([
            "lenso",
            "system",
            "dev",
            "--system-file",
            "system.json",
            "--sandbox-file",
            "sandbox.json",
            "--scenario",
            "deadline-timeout",
            "--dry-run",
            "--json",
        ]);
        let Command::System {
            command: SystemCommand::Dev(args),
        } = cli.command
        else {
            panic!("expected system dev");
        };
        assert_eq!(
            args.system_file.as_deref(),
            Some(std::path::Path::new("system.json"))
        );
        assert_eq!(
            args.sandbox_file.as_deref(),
            Some(std::path::Path::new("sandbox.json"))
        );
        assert_eq!(args.scenario.as_deref(), Some("deadline-timeout"));
        assert!(args.dry_run);
        assert!(args.json);
        assert!(!args.cleanup);

        let cleanup = Cli::parse_from(["lenso", "system", "dev", "--cleanup"]);
        let Command::System {
            command: SystemCommand::Dev(args),
        } = cleanup.command
        else {
            panic!("expected system dev cleanup");
        };
        assert!(args.cleanup);
    }
}
