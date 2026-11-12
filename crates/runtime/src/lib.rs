//! Prepare declared dependencies and discover commands without executing their modules.

mod cache;
mod discovery;
mod github;
mod host;
mod http;
mod manifest;
mod migration;
mod package;
mod process;
mod project;
mod publish;
mod registry;
mod resources;
mod source;
mod wasm;

pub use cache::Cache;
pub use host::{Evidence, path_allowed, project_path};
pub use http::Credential;
pub use manifest::{Extension, HandlerSpec, load_project, load_project_with_options};
pub use migration::migrate_project;
pub use process::ProcessSpec;
pub use project::{
    Installation, LoadMode, LoadOptions, Project, ProjectCli, prepare_project, read_project,
    register_management, register_management_with_options,
};
pub use wasm::WasmSpec;

pub use github::GithubConfig;
pub use package::package_extension;
pub use resources::{ResourcePin, verify_resource_pins};
