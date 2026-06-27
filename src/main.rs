mod host;
mod module;
mod service;

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
    /// Manage Runtime Console assets, access, and packages.
    Console {
        #[command(subcommand)]
        command: ConsoleCommand,
    },
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
struct ConsoleBootstrapAdminArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to read for `DATABASE_URL`.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Auth user id, such as `usr_abc`.
    #[arg(long)]
    user_id: Option<String>,

    /// Password-auth identifier, such as an email address.
    #[arg(long)]
    identifier: Option<String>,

    /// Extra scope to grant. console.admin is always included.
    #[arg(long = "scope")]
    scopes: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum ConsoleCommand {
    /// Refresh the hosted Runtime Console assets in a host project.
    Update(ConsoleUpdateArgs),
    /// Grant Runtime Console admin scopes to an auth user.
    BootstrapAdmin(ConsoleBootstrapAdminArgs),
    /// Manage Runtime Console package registration.
    Package {
        #[command(subcommand)]
        command: ConsolePackageCommand,
    },
}

#[derive(Debug, Args, Clone)]
struct ConsoleUpdateArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Install from a local artifact directory or .tar.gz instead of downloading.
    #[arg(long = "artifact")]
    source: Option<std::path::PathBuf>,

    /// Runtime Console GitHub release version to download.
    #[arg(long = "console-version", default_value = "latest")]
    console_version: String,
}

#[derive(Debug, Subcommand)]
enum ModuleCommand {
    /// Create a linked module or service scaffold.
    Create(ModuleCreateArgs),
    /// Install a remote source or enable a linked module.
    Install(RemoteModuleInstallArgs),
    /// Add a configured service source.
    Add(RemoteModuleInstallArgs),
    /// Reapply an installed module from its install receipt.
    Update(ModuleUpdateArgs),
    /// Remove a remote source or disable a linked module.
    Uninstall(RemoteModuleUninstallArgs),
    /// Diagnose installed services.
    Doctor(ModuleDoctorArgs),
    /// Inspect and manage declared service processes.
    Service {
        #[command(subcommand)]
        command: ModuleServiceCommand,
    },
    /// Manage a local module catalog.
    Catalog {
        #[command(subcommand)]
        command: ModuleCatalogCommand,
    },
    /// Install services.
    Marketplace {
        #[command(subcommand)]
        command: ModuleMarketplaceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ModuleCatalogCommand {
    /// Add a service manifest to the local catalog.
    Add(ModuleCatalogAddArgs),
}

#[derive(Debug, Subcommand)]
enum ModuleMarketplaceCommand {
    /// Install a service from its manifest.
    Install(RemoteModuleInstallArgs),
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Create a service provider scaffold.
    Create(ServiceCreateArgs),
    /// Start service providers, then run the generated host.
    Dev(ServiceDevArgs),
    /// Install a service manifest.
    Install(RemoteModuleInstallArgs),
    /// Remove a service provider and its provided modules.
    Uninstall(RemoteModuleUninstallArgs),
    /// Show changes between installed and candidate service manifests.
    Diff(ServiceDiffArgs),
    /// Upgrade an installed service from a candidate manifest.
    Upgrade(ServiceUpgradeArgs),
    /// Roll back a service to the previous installed manifest snapshot.
    Rollback(ServiceRollbackArgs),
    /// Diagnose installed services and their provided modules.
    Doctor(ModuleDoctorArgs),
    /// Check a service manifest or configured service state.
    Check(ServiceCheckArgs),
    /// List declared services.
    List(ModuleServiceListArgs),
    /// Export a deployment fragment for declared services.
    Export(ModuleServiceExportArgs),
    /// Show one service with local state.
    Status(ModuleServiceStatusArgs),
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

    /// Print files without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args, Clone)]
struct ServiceDevArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

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

    /// Ready wait timeout in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    ready_timeout_ms: u64,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to read when checking installed services.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Remote module services file.
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

    /// Remote service base URL for local manifest files.
    #[arg(long)]
    base_url: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print changes without writing them.
    #[arg(long)]
    dry_run: bool,

    /// Allow upgrade when compatibility metadata does not match this host.
    #[arg(long)]
    allow_incompatible: bool,
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

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print changes without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Subcommand)]
enum ModuleServiceCommand {
    /// List declared services.
    List(ModuleServiceListArgs),
    /// Export a deployment fragment for declared services.
    Export(ModuleServiceExportArgs),
    /// Show one service with local state.
    Status(ModuleServiceStatusArgs),
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

    /// Remote module services file.
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

    /// Remote module services file.
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

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
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

    /// Remote module services file.
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

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct RemoteModuleInstallArgs {
    /// Module reference: remote manifest URL/path, or linked module name.
    manifest_reference: String,

    /// Legacy loading-source override when the reference is not a descriptor.
    #[arg(long, default_value = "remote")]
    source: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Remote module base URL.
    #[arg(long)]
    base_url: Option<String>,

    /// Install descriptor profile to apply.
    #[arg(long = "profile", alias = "with", value_delimiter = ',')]
    install_profiles: Vec<String>,

    /// Skip Runtime Console extension registration.
    #[arg(long = "no-console-extension", alias = "no-console-plan", action = clap::ArgAction::SetFalse, default_value_t = true)]
    console_plan: bool,

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
struct RemoteModuleUninstallArgs {
    /// Module name.
    module_name: String,

    /// Loading source: remote or linked.
    #[arg(long)]
    source: Option<String>,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Environment file to update.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,

    /// Remote module services file.
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

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Remote module base URL override.
    #[arg(long)]
    base_url: Option<String>,

    /// Install descriptor profile to apply.
    #[arg(long = "profile", alias = "with", value_delimiter = ',')]
    install_profiles: Vec<String>,

    /// Skip Runtime Console extension registration.
    #[arg(long = "no-console-extension", alias = "no-console-plan", action = clap::ArgAction::SetFalse, default_value_t = true)]
    console_plan: bool,

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

    /// Remote module services file.
    #[arg(long)]
    module_services_file: Option<std::path::PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ModuleCatalogAddArgs {
    /// Remote module manifest URL, file URL, or local JSON path.
    manifest_reference: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Module catalog file to update.
    #[arg(long)]
    catalog_file: Option<std::path::PathBuf>,

    /// Remote module base URL.
    #[arg(long)]
    base_url: Option<String>,

    /// Catalog summary text.
    #[arg(long)]
    summary: Option<String>,

    /// Print catalog changes without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Subcommand)]
enum ConsolePackageCommand {
    /// Create a Runtime Console package scaffold.
    Create(ConsolePackageCreateArgs),
    /// Apply a console package install plan.
    ApplyPlan(ConsolePackageApplyPlanArgs),
}

#[derive(Debug, Args, Clone)]
struct ModuleCreateArgs {
    /// Module id, such as billing or support.
    module_id: String,

    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Directory for standalone service packages.
    #[arg(long)]
    output_dir: Option<std::path::PathBuf>,

    /// Runtime Console app root.
    #[arg(long)]
    runtime_console_root: Option<std::path::PathBuf>,

    /// Console surface area.
    #[arg(long)]
    area: Option<String>,

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

    /// Console package install source.
    #[arg(long)]
    source: Option<String>,

    /// Create a standalone service package.
    #[arg(long)]
    remote: bool,

    /// Create a matching Runtime Console package.
    #[arg(long)]
    with_console: bool,

    /// Console package slug.
    #[arg(long)]
    package_slug: Option<String>,

    /// Console package npm scope.
    #[arg(long)]
    package_scope: Option<String>,

    /// Full console package name.
    #[arg(long)]
    package_name: Option<String>,

    /// Console surface name.
    #[arg(long)]
    surface_name: Option<String>,

    /// Remote package root directory.
    #[arg(long)]
    package_root: Option<String>,

    /// Print files without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsolePackageCreateArgs {
    /// Module id, such as billing or support.
    module_id: String,

    /// Runtime Console app root.
    #[arg(long)]
    runtime_console_root: Option<std::path::PathBuf>,

    /// Console surface area.
    #[arg(long)]
    area: Option<String>,

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

    /// Console package install source.
    #[arg(long)]
    source: Option<String>,

    /// Console package slug.
    #[arg(long)]
    package_slug: Option<String>,

    /// Console package npm scope.
    #[arg(long)]
    package_scope: Option<String>,

    /// Full console package name.
    #[arg(long)]
    package_name: Option<String>,

    /// Console surface name.
    #[arg(long)]
    surface_name: Option<String>,

    /// Print files without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args, Clone)]
struct ConsolePackageApplyPlanArgs {
    /// Lenso host repository root.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Runtime Console app root.
    #[arg(long)]
    runtime_console_root: Option<std::path::PathBuf>,

    /// Console package install plan file.
    #[arg(long)]
    install_plan_file: Option<std::path::PathBuf>,

    /// Dependency version to write when the package is not already declared.
    #[arg(long)]
    dependency_version: Option<String>,

    /// Print install plan changes without writing them.
    #[arg(long)]
    dry_run: bool,
}

impl From<&RemoteModuleInstallArgs> for module::RemoteModuleInstallOptions {
    fn from(args: &RemoteModuleInstallArgs) -> Self {
        Self {
            allow_incompatible: args.allow_incompatible,
            base_url: args.base_url.clone(),
            console_plan: args.console_plan,
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            install_profiles: args.install_profiles.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            run_install_commands: args.run_install_commands,
            source: args.source.clone(),
        }
    }
}

impl From<&RemoteModuleUninstallArgs> for module::RemoteModuleUninstallOptions {
    fn from(args: &RemoteModuleUninstallArgs) -> Self {
        Self {
            dry_run: args.dry_run,
            env_file: args.env_file.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            source: args.source.clone(),
        }
    }
}

impl From<&ModuleUpdateArgs> for module::ModuleUpdateOptions {
    fn from(args: &ModuleUpdateArgs) -> Self {
        Self {
            allow_incompatible: args.allow_incompatible,
            base_url: args.base_url.clone(),
            console_plan: args.console_plan,
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

impl From<&ConsoleBootstrapAdminArgs> for host::BootstrapAdminOptions {
    fn from(args: &ConsoleBootstrapAdminArgs) -> Self {
        Self {
            env_file: args.env_file.clone(),
            identifier: args.identifier.clone(),
            repo_root: args.repo_root.clone(),
            scopes: args.scopes.clone(),
            user_id: args.user_id.clone(),
        }
    }
}

impl From<&ModuleCreateArgs> for module::ModuleCreateOptions {
    fn from(args: &ModuleCreateArgs) -> Self {
        Self {
            area: args.area.clone(),
            capability: args.capability.clone(),
            dry_run: args.dry_run,
            icon: args.icon.clone(),
            label: args.label.clone(),
            module_id: args.module_id.clone(),
            output_dir: args.output_dir.clone(),
            package_name: args.package_name.clone(),
            package_root: args.package_root.clone(),
            package_scope: args.package_scope.clone(),
            package_slug: args.package_slug.clone(),
            remote: args.remote,
            repo_root: args.repo_root.clone(),
            route: args.route.clone(),
            runtime_console_root: args.runtime_console_root.clone(),
            source: args.source.clone(),
            surface_name: args.surface_name.clone(),
            with_console: args.with_console,
        }
    }
}

impl From<&ConsolePackageCreateArgs> for module::ConsolePackageCreateOptions {
    fn from(args: &ConsolePackageCreateArgs) -> Self {
        Self {
            area: args.area.clone(),
            capability: args.capability.clone(),
            dry_run: args.dry_run,
            icon: args.icon.clone(),
            label: args.label.clone(),
            module_id: args.module_id.clone(),
            package_name: args.package_name.clone(),
            package_scope: args.package_scope.clone(),
            package_slug: args.package_slug.clone(),
            route: args.route.clone(),
            runtime_console_root: args.runtime_console_root.clone(),
            source: args.source.clone(),
            surface_name: args.surface_name.clone(),
        }
    }
}

impl From<&ModuleCatalogAddArgs> for module::ModuleCatalogAddOptions {
    fn from(args: &ModuleCatalogAddArgs) -> Self {
        Self {
            base_url: args.base_url.clone(),
            catalog_file: args.catalog_file.clone(),
            dry_run: args.dry_run,
            repo_root: args.repo_root.clone(),
            summary: args.summary.clone(),
        }
    }
}

impl From<&ConsolePackageApplyPlanArgs> for module::ConsolePackageApplyPlanOptions {
    fn from(args: &ConsolePackageApplyPlanArgs) -> Self {
        Self {
            dependency_version: args.dependency_version.clone(),
            dry_run: args.dry_run,
            install_plan_file: args.install_plan_file.clone(),
            log_next_steps: true,
            repo_root: args.repo_root.clone(),
            runtime_console_root: args.runtime_console_root.clone(),
        }
    }
}

fn looks_like_manifest_reference(reference: &str) -> bool {
    reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.ends_with(".json")
        || reference.contains("/manifest")
}

fn warn_module_install_manifest_reference(reference: &str) {
    if looks_like_manifest_reference(reference) {
        eprintln!(
            "warning: `lenso module install <manifest>` is deprecated for service manifests; use `lenso service install <manifest>` or `lenso module install <module-name>`."
        );
    }
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
        Command::Host { command } => match command {
            HostCommand::Init { dir, name, force } => host::init(&dir, name.as_deref(), force)?,
        },
        Command::Console { command } => match command {
            ConsoleCommand::Update(args) => {
                host::update_console(host::UpdateConsoleOptions {
                    repo_root: args.repo_root,
                    source: args.source,
                    version: args.console_version,
                })
                .await?;
            }
            ConsoleCommand::BootstrapAdmin(args) => {
                host::bootstrap_admin((&args).into()).await?;
            }
            ConsoleCommand::Package { command } => match command {
                ConsolePackageCommand::Create(args) => {
                    module::create_console_package((&args).into()).await?;
                }
                ConsolePackageCommand::ApplyPlan(args) => {
                    module::apply_console_package_install_plan((&args).into()).await?;
                }
            },
        },
        Command::Module { command } => match command {
            ModuleCommand::Create(args) => {
                module::create_module((&args).into()).await?;
            }
            ModuleCommand::Install(args) => {
                warn_module_install_manifest_reference(&args.manifest_reference);
                module::install_module(&args.manifest_reference, (&args).into()).await?;
            }
            ModuleCommand::Add(args) => {
                module::install_module(&args.manifest_reference, (&args).into()).await?;
            }
            ModuleCommand::Update(args) => {
                module::update_module(&args.module_name, (&args).into()).await?;
            }
            ModuleCommand::Uninstall(args) => {
                module::uninstall_module(&args.module_name, (&args).into()).await?;
            }
            ModuleCommand::Doctor(args) => {
                module::doctor_module((&args).into()).await?;
            }
            ModuleCommand::Service { command } => match command {
                ModuleServiceCommand::List(args) => {
                    module::list_module_services((&args).into()).await?;
                }
                ModuleServiceCommand::Export(args) => {
                    module::export_module_services((&args).into()).await?;
                }
                ModuleServiceCommand::Status(args) => {
                    module::status_module_service((&args).into()).await?;
                }
                ModuleServiceCommand::Start(args) => {
                    module::start_module_service((&args).into()).await?;
                }
                ModuleServiceCommand::Stop(args) => {
                    module::stop_module_service((&args).into()).await?;
                }
            },
            ModuleCommand::Catalog { command } => match command {
                ModuleCatalogCommand::Add(args) => {
                    module::add_module_catalog_entry(&args.manifest_reference, (&args).into())
                        .await?;
                }
            },
            ModuleCommand::Marketplace { command } => match command {
                ModuleMarketplaceCommand::Install(args) => {
                    module::install_module(&args.manifest_reference, (&args).into()).await?;
                }
            },
        },
        Command::Service { command } => match command {
            ServiceCommand::Create(args) => {
                service::create_service((&args).into())?;
            }
            ServiceCommand::Dev(args) => {
                service::dev_service((&args).into()).await?;
            }
            ServiceCommand::Install(args) => {
                module::install_module(&args.manifest_reference, (&args).into()).await?;
            }
            ServiceCommand::Uninstall(args) => {
                module::uninstall_remote_module(&args.module_name, (&args).into()).await?;
            }
            ServiceCommand::Diff(args) => {
                module::diff_service((&args).into()).await?;
            }
            ServiceCommand::Upgrade(args) => {
                module::upgrade_service((&args).into()).await?;
            }
            ServiceCommand::Rollback(args) => {
                module::rollback_service((&args).into()).await?;
            }
            ServiceCommand::Doctor(args) => {
                module::doctor_module((&args).into()).await?;
            }
            ServiceCommand::Check(args) => {
                let checks_manifest = args.serve_command.is_some()
                    || args
                        .manifest_reference
                        .as_deref()
                        .is_some_and(looks_like_manifest_reference);
                if checks_manifest {
                    module::check_service_manifest_reference(
                        args.manifest_reference
                            .as_deref()
                            .unwrap_or("./lenso.service.json"),
                        module::ServiceManifestCheckOptions {
                            cwd: args.cwd.clone(),
                            json: args.json,
                            manifest_url: args.manifest_url.clone(),
                            ready_timeout_ms: args.ready_timeout_ms,
                            ready_url: args.ready_url.clone(),
                            serve_command: args.serve_command.clone(),
                        },
                    )
                    .await?;
                } else {
                    module::doctor_module((&args).into()).await?;
                }
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

    #[test]
    fn parses_service_create_ts() {
        let cli = Cli::parse_from([
            "lenso",
            "service",
            "create",
            "support-suite-provider",
            "--lang",
            "ts",
        ]);

        let Command::Service {
            command: ServiceCommand::Create(args),
        } = cli.command
        else {
            panic!("expected service create");
        };

        assert_eq!(args.name, "support-suite-provider");
        assert_eq!(args.lang, ServiceLanguage::Ts);
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
        let cli = Cli::parse_from(["lenso", "service", "dev", "--skip-db"]);
        let Command::Service {
            command: ServiceCommand::Dev(args),
        } = cli.command
        else {
            panic!("expected service dev");
        };

        assert!(args.skip_db);
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
}
