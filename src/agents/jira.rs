use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, JsonObject, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use serde_json::Value;

const DEFAULT_MCP_URL: &str = "https://mcp.atlassian.com/v1/mcp/authv2";
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const ALLOWED_TOOLS: [&str; 6] = [
    "atlassianUserInfo",
    "getAccessibleAtlassianResources",
    "getJiraIssue",
    "searchJiraIssuesUsingJql",
    "searchAtlassian",
    "fetchAtlassian",
];

pub(super) struct Jira {
    service: RunningService<RoleClient, ()>,
    tools: Vec<Tool>,
    allowed_tool_names: HashSet<String>,
}

impl Jira {
    pub(super) async fn connect() -> Result<Self> {
        let program = if cfg!(windows) { "npx.cmd" } else { "npx" };
        let mcp_url =
            std::env::var("FLOGGING_JIRA_MCP_URL").unwrap_or_else(|_| DEFAULT_MCP_URL.to_owned());
        let mut command = tokio::process::Command::new(program);
        command.arg("-y").arg("mcp-remote@latest").arg(mcp_url);
        command.kill_on_drop(true);

        let (transport, _) = TokioChildProcess::builder(command)
            .stderr(Stdio::null())
            .spawn()
            .context("could not start the Atlassian MCP client; npx must be installed")?;
        let service = ()
            .serve(transport)
            .await
            .context("could not connect to the Atlassian MCP server")?;
        let tools = service
            .list_all_tools()
            .await
            .context("could not list Atlassian MCP tools")?
            .into_iter()
            .filter(|tool| ALLOWED_TOOLS.contains(&tool.name.as_ref()))
            .collect::<Vec<_>>();

        if tools.is_empty() {
            return Err(anyhow!(
                "the Atlassian MCP server did not expose any supported read-only Jira tools"
            ));
        }

        let allowed_tool_names = tools.iter().map(|tool| tool.name.to_string()).collect();

        Ok(Self {
            service,
            tools,
            allowed_tool_names,
        })
    }

    pub(super) fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub(super) async fn call(&self, name: String, arguments: Value) -> Result<String> {
        if !self.allowed_tool_names.contains(&name) {
            return Err(anyhow!("the agent requested unsupported Jira tool {name}"));
        }

        let arguments: JsonObject = arguments
            .as_object()
            .cloned()
            .context("Jira tool arguments must be a JSON object")?;
        let result = tokio::time::timeout(
            TOOL_CALL_TIMEOUT,
            self.service
                .call_tool(CallToolRequestParams::new(name).with_arguments(arguments)),
        )
        .await
        .context("Jira MCP tool call timed out")?
        .context("Jira MCP tool call failed")?;

        serde_json::to_string(&result).context("could not serialize the Jira MCP result")
    }

    pub(super) async fn shutdown(self) -> Result<()> {
        self.service
            .cancel()
            .await
            .map(|_| ())
            .context("could not stop the Atlassian MCP client")
    }
}
