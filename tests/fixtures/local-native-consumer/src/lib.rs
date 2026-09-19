use lenso_capability_agent_tool_provider::{self as tools, ExecuteRequest};
#[lenso::plugin(consumer, lifecycle)]
#[derive(Clone, Debug)]
struct Proof {
    tools: lenso::Port<tools::ToolProviderClient>,
}
impl lenso::Lifecycle for Proof {
    async fn activate(&self, _context: lenso::ActivateContext) -> Result<(), lenso::RuntimeFailure> {
        let response = self.tools.execute(ExecuteRequest {
            name: "local.starter".into(),
            arguments_json: r#"{"text":"mixed host works"}"#.try_into().unwrap(),
        }).await.map_err(|e| lenso::RuntimeFailure::InvalidResolvedPlan { detail: format!("{e:?}") })?;
        assert!(response.content.contains("MIXED HOST WORKS"), "{response:?}");
        eprintln!("NATIVE_TO_BUN_OK {}", response.content);
        Ok(())
    }
    async fn deactivate(&self, _context: lenso::DeactivateContext) -> Result<(), lenso::RuntimeFailure> {
        eprintln!("NATIVE_DEACTIVATED");
        Ok(())
    }
}
