mod routes;

use lenso::host::http::{LinkedHttpContribution, ModuleHttpMethod, ModuleHttpRoute};
use lenso::host::prelude::*;

pub const MODULE_NAME: &str = "app";
pub const APP_STATUS_READ_CAPABILITY: &str = "app.status.read";
pub const APP_ITEMS_READ_CAPABILITY: &str = "app.items.read";
pub const APP_ITEMS_WRITE_CAPABILITY: &str = "app.items.write";

const APP_MIGRATIONS: &[Migration] = &[Migration {
    name: "app/0001_create_app_schema",
    sql: include_str!("migrations/0001_create_app_schema.sql"),
}];

/// Project-owned linked module skeleton.
///
/// Rename this module or add more modules beside it as your backend grows.
pub fn linked_module() -> HostLinkedModule {
    HostLinkedModule::manifest_only(MODULE_NAME, manifest, APP_MIGRATIONS)
        .with_http_binding(http_binding)
}

pub fn http_routes() -> Vec<ModuleHttpRoute> {
    vec![
        ModuleHttpRoute {
            method: ModuleHttpMethod::Get,
            path: "/v1/app/status".to_owned(),
            capability: Some(APP_STATUS_READ_CAPABILITY.to_owned()),
            display_name: Some("App Status".to_owned()),
            story_title: Some("App Status".to_owned()),
        },
        ModuleHttpRoute {
            method: ModuleHttpMethod::Get,
            path: "/v1/app/items".to_owned(),
            capability: Some(APP_ITEMS_READ_CAPABILITY.to_owned()),
            display_name: Some("List App Items".to_owned()),
            story_title: Some("App Items".to_owned()),
        },
        ModuleHttpRoute {
            method: ModuleHttpMethod::Get,
            path: "/v1/app/items/{id}".to_owned(),
            capability: Some(APP_ITEMS_READ_CAPABILITY.to_owned()),
            display_name: Some("Get App Item".to_owned()),
            story_title: Some("App Items".to_owned()),
        },
        ModuleHttpRoute {
            method: ModuleHttpMethod::Patch,
            path: "/v1/app/items/{id}".to_owned(),
            capability: Some(APP_ITEMS_WRITE_CAPABILITY.to_owned()),
            display_name: Some("Update App Item".to_owned()),
            story_title: Some("App Items".to_owned()),
        },
        ModuleHttpRoute {
            method: ModuleHttpMethod::Delete,
            path: "/v1/app/items/{id}".to_owned(),
            capability: Some(APP_ITEMS_WRITE_CAPABILITY.to_owned()),
            display_name: Some("Delete App Item".to_owned()),
            story_title: Some("App Items".to_owned()),
        },
        ModuleHttpRoute {
            method: ModuleHttpMethod::Post,
            path: "/v1/app/items".to_owned(),
            capability: Some(APP_ITEMS_WRITE_CAPABILITY.to_owned()),
            display_name: Some("Create App Item".to_owned()),
            story_title: Some("App Items".to_owned()),
        },
    ]
}

fn manifest() -> ModuleManifest {
    ModuleManifest::builder(MODULE_NAME)
        .capabilities(vec![
            APP_STATUS_READ_CAPABILITY.to_owned(),
            APP_ITEMS_READ_CAPABILITY.to_owned(),
            APP_ITEMS_WRITE_CAPABILITY.to_owned(),
        ])
        .http_routes(http_routes())
        .build()
}

fn http_binding() -> LinkedBinding {
    LinkedBinding::builder()
        .http(LinkedHttpContribution {
            public_prefixes: &["/v1/app/"],
            merge: routes::merge_http,
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_module_exposes_starter_metadata() {
        let module = linked_module();
        let manifest = (module.manifest)();

        assert_eq!(module.module_name, MODULE_NAME);
        assert_eq!(manifest.name, MODULE_NAME);
        assert_eq!(
            manifest.capabilities,
            vec![
                APP_STATUS_READ_CAPABILITY,
                APP_ITEMS_READ_CAPABILITY,
                APP_ITEMS_WRITE_CAPABILITY
            ]
        );
        assert_eq!(manifest.http_routes, http_routes());
        assert!(module.http_binding.is_some());
        assert!(module
            .migrations
            .iter()
            .any(|migration| migration.name == "app/0001_create_app_schema"));
    }
}
