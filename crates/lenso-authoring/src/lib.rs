//! Package-manager inspection, Plan resolution, and Runner orchestration for Lenso.

mod app_edit;
mod authoring_project;
mod canonical;
mod definition;
mod package_manager;
mod recipe;
mod resolution;
mod runner;
mod validation;
mod workflow;

pub use app_edit::*;
pub use authoring_project::*;
pub use definition::*;
pub use lenso_app_plan::authoring::*;
pub use recipe::*;
pub use resolution::*;
pub use runner::*;
pub use workflow::*;

pub(crate) use canonical::{
    canonical_json_bytes, canonical_json_string, canonical_pretty_json, sort_json_value,
};
