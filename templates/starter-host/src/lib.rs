mod modules;

use lenso_host::prelude::*;

/// Host-owned module composition for this application.
///
/// Add project modules here with `HostBuilder::linked_module(...)`.
pub fn host_composition() -> HostComposition {
    HostBuilder::new()
        .linked_module(modules::app::linked_module())
        .build()
}
