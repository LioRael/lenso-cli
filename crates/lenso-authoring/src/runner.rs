use std::time::Duration;

use lenso_kernel::{ExecutionAdapterCatalog, RuntimeDriver, TerminalOutcome};

use crate::{AuthoringError, ResolvedProject};

pub async fn run_project<D: RuntimeDriver>(
    resolved: &ResolvedProject,
    driver: D,
    adapters: ExecutionAdapterCatalog,
    shutdown_timeout: Duration,
) -> Result<TerminalOutcome, AuthoringError> {
    let available = adapters.execution_classes();
    for instance in resolved.plan().module_instances() {
        if !available.contains(instance.execution_class()) {
            return Err(AuthoringError::UnavailableExecutionClass {
                instance: instance.instance_key().to_owned(),
                execution_class: instance.execution_class().to_string(),
            });
        }
    }
    lenso_runner::run(resolved.plan().clone(), driver, adapters, shutdown_timeout)
        .await
        .map_err(|source| AuthoringError::Runner { source })
}
