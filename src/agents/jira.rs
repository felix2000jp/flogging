use std::collections::HashSet;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, JsonObject, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

const DEFAULT_MCP_URL: &str = "https://mcp.atlassian.com/v1/mcp/authv2";
const MCP_REMOTE_PACKAGE: &str = "mcp-remote@0.8.2";
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
        command.arg("-y").arg(MCP_REMOTE_PACKAGE).arg(mcp_url);
        command.kill_on_drop(true);

        tracing::info!(package = MCP_REMOTE_PACKAGE, "connecting to Jira MCP");

        let (transport, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .context("could not start the Atlassian MCP client; npx must be installed")?;
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();

                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            tracing::debug!(message = %line, "Jira MCP process output");
                        }
                        Ok(None) => break,
                        Err(error) => {
                            tracing::warn!(%error, "could not read Jira MCP process output");
                            break;
                        }
                    }
                }
            });
        }
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

        tracing::info!(supported_tool_count = tools.len(), "connected to Jira MCP");

        Ok(Self {
            service,
            tools,
            allowed_tool_names,
        })
    }

    pub(super) fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub(super) async fn current_user(&self) -> Result<Value> {
        let result = self
            .call_result("atlassianUserInfo".to_owned(), json!({}))
            .await
            .context("could not identify the authenticated Jira user")?;

        authenticated_user(result)
    }

    pub(super) async fn call(&self, name: String, arguments: Value) -> Result<String> {
        let result = self.call_result(name, arguments).await?;

        serde_json::to_string(&result).context("could not serialize the Jira MCP result")
    }

    async fn call_result(&self, name: String, arguments: Value) -> Result<CallToolResult> {
        if !self.allowed_tool_names.contains(&name) {
            return Err(anyhow!("the agent requested unsupported Jira tool {name}"));
        }

        let arguments: JsonObject = arguments
            .as_object()
            .cloned()
            .context("Jira tool arguments must be a JSON object")?;
        let started_at = Instant::now();
        tracing::debug!(tool = %name, "calling Jira MCP tool");
        let result = tokio::time::timeout(
            TOOL_CALL_TIMEOUT,
            self.service
                .call_tool(CallToolRequestParams::new(name.clone()).with_arguments(arguments)),
        )
        .await
        .with_context(|| format!("Jira MCP tool {name} timed out"))?
        .with_context(|| format!("Jira MCP tool {name} failed"));

        if result.is_ok() {
            tracing::debug!(
                tool = %name,
                elapsed_ms = started_at.elapsed().as_millis(),
                "Jira MCP tool completed"
            );
        }

        result
    }

    pub(super) async fn shutdown(self) -> Result<()> {
        tracing::debug!("stopping Jira MCP client");
        self.service
            .cancel()
            .await
            .map(|_| ())
            .context("could not stop the Atlassian MCP client")
    }
}

fn authenticated_user(result: CallToolResult) -> Result<Value> {
    if result.is_error == Some(true) {
        return Err(anyhow!("Atlassian rejected the authenticated-user request"));
    }

    if let Some(structured_content) = result.structured_content {
        return Ok(structured_content);
    }

    let text = result
        .content
        .iter()
        .filter_map(|content| content.as_text())
        .map(|content| content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() {
        return Err(anyhow!(
            "Atlassian returned no authenticated-user information"
        ));
    }

    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

#[cfg(test)]
mod tests {
    use rmcp::model::{CallToolResult, ContentBlock};
    use serde_json::json;

    use super::authenticated_user;

    #[test]
    fn extracts_structured_authenticated_user_information() {
        let mut result = CallToolResult::success(vec![]);
        result.structured_content = Some(json!({ "account_id": "user-123" }));

        assert_eq!(
            authenticated_user(result).unwrap(),
            json!({ "account_id": "user-123" })
        );
    }

    #[test]
    fn extracts_json_authenticated_user_information_from_text() {
        let result =
            CallToolResult::success(vec![ContentBlock::text(r#"{"account_id":"user-123"}"#)]);

        assert_eq!(
            authenticated_user(result).unwrap(),
            json!({ "account_id": "user-123" })
        );
    }

    #[test]
    fn rejects_an_authenticated_user_tool_error() {
        let result = CallToolResult::error(vec![ContentBlock::text("not authenticated")]);

        assert!(authenticated_user(result).is_err());
    }
}
