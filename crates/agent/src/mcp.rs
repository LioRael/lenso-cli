//! Optional stdio MCP transport. The official SDK owns framing and lifecycle.

use std::{borrow::Cow, sync::Arc};

use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};

use crate::Agent;

struct Server(Arc<Agent>);

impl ServerHandler for Server {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2025_11_25])
    }

    fn get_info(&self) -> ServerConfig {
        let mut config = ServerConfig::default();
        config.protocol_version = ProtocolVersion::V_2025_11_25;
        config.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();
        config.server_info = Implementation::new("lenso-agent", env!("CARGO_PKG_VERSION"));
        config.instructions = Some("Only locally selected aliases are exposed. Questions require explicit answers; discovery never grants permission. Report resources last for this server process only.".into());
        config
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "No pagination cursor is available",
                None,
            ));
        }
        let tools = self
            .0
            .tools()
            .into_iter()
            .map(|tool| serde_json::to_value(tool).and_then(serde_json::from_value))
            .collect::<Result<Vec<Tool>, _>>()
            .map_err(|_| ErrorData::internal_error("Tool description unavailable", None))?;
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !self.0.tools().iter().any(|tool| tool.name == request.name) {
            return Err(ErrorData::invalid_params("Tool is not selected", None));
        }
        let result = self
            .0
            .call(
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
                context.ct,
            )
            .await;
        let (value, failed) = match result {
            Ok(value) => {
                let failed = value.exit_code != 0 && value.exit_code != 2;
                (json!(value), failed)
            }
            Err(error) => (
                json!({"exit_code":error.exit_code(),"result":error.diagnostic()}),
                true,
            ),
        };
        Ok(if failed {
            CallToolResult::structured_error(value)
        } else {
            CallToolResult::structured(value)
        }
        .into())
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "No pagination cursor is available",
                None,
            ));
        }
        let resources = self
            .0
            .reports()
            .await
            .into_iter()
            .map(|uri| {
                let mut resource = Resource::new(uri, "CLI execution report");
                resource.mime_type = Some("application/json".into());
                resource
            })
            .collect();
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let value =
            self.0.report(&request.uri).await.map_err(|_| {
                ErrorData::invalid_params("Unknown or evicted execution report", None)
            })?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(value.to_string(), request.uri)
                .with_mime_type("application/json"),
        ])
        .into())
    }
}

/// Serve one local stdio client; EOF or an interrupt cancels active CLI execution.
///
/// # Errors
/// Returns a public transport diagnostic without raw protocol or SDK errors.
pub async fn serve_stdio(agent: Arc<Agent>) -> Result<(), lenso_cli::Error> {
    let running = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        Server(agent.clone()).serve(rmcp::transport::stdio()),
    )
    .await
    .map_err(|_| {
        crate::process::failure(
            lenso_cli::DiagnosticCode::OperationFailed,
            "mcp_initialization_timeout",
        )
    })?
    .map_err(|_| {
        crate::process::failure(
            lenso_cli::DiagnosticCode::OperationFailed,
            "mcp_initialization_failed",
        )
    })?;
    tokio::select! {
        _ = running.waiting() => {},
        _ = crate::shutdown_signal() => {},
    }
    agent.shutdown().await;
    Ok(())
}
