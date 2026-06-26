mod host;
mod module;

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
    /// Create a linked or remote module scaffold.
    Create(ModuleCreateArgs),
    /// Install a remote source or enable a linked module.
    Install(RemoteModuleInstallArgs),
    /// Add a configured remote module source.
    Add(RemoteModuleInstallArgs),
    /// Reapply an installed module from its install receipt.
    Update(ModuleUpdateArgs),
    /// Remove a remote source or disable a linked module.
    Uninstall(RemoteModuleUninstallArgs),
    /// Diagnose installed remote module services.
    Doctor(ModuleDoctorArgs),
    /// Manage a local module catalog.
    Catalog {
        #[command(subcommand)]
        command: ModuleCatalogCommand,
    },
    /// Install remote modules.
    Marketplace {
        #[command(subcommand)]
        command: ModuleMarketplaceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ModuleCatalogCommand {
    /// Add a remote module manifest to the local catalog.
    Add(ModuleCatalogAddArgs),
}

#[derive(Debug, Subcommand)]
enum ModuleMarketplaceCommand {
    /// Install a remote module from its manifest.
    Install(RemoteModuleInstallArgs),
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

    /// Directory for standalone remote packages.
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

    /// Create a standalone remote module package.
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
            module_name: args.module_name.clone(),
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
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
    }

    Ok(())
}
