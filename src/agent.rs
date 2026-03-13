use crate::llm::{LlmClient, LlmRequest, Message, ToolCall};
use crate::tools::{bash_tool_definition, git_commit_all, git_reset_hard};
use tracing::{info, warn, error};

const SYSTEM_PROMPT: &str = "You are a self-improving coding agent. Execute tools to accomplish your task. Do not summarize or explain — just act.";
const MAX_TOOL_TURNS: usize = 50;

pub struct AgentCore {
    llm: Box<dyn LlmClient>,
}

impl AgentCore {
    pub fn new(llm: Box<dyn LlmClient>) -> Self {
        Self { llm }
    }

    pub async fn run_cycle(&mut self, initial_prompt: String) -> Result<(), anyhow::Error> {
        let tools = vec![bash_tool_definition()];

        // Build initial conversation
        let mut messages: Vec<Message> = vec![Message {
            role: "user".into(),
            content: initial_prompt,
            tool_call_id: None,
            tool_calls: None,
        }];

        // Inner loop: call LLM → execute tools → feed results back → repeat
        for turn in 0..MAX_TOOL_TURNS {
            info!("--- Turn {} ---", turn + 1);

            let req = LlmRequest {
                system_prompt: SYSTEM_PROMPT.into(),
                messages: messages.clone(),
                max_tokens: 16384,
                temperature: 0.1,
                tools: tools.clone(),
            };

            let resp = match self.llm.complete(req).await {
                Ok(r) => r,
                Err(e) => {
                    error!("LLM call failed: {:?}", e);
                    return Err(e.into());
                }
            };

            // Log any text the LLM returned
            if let Some(ref text) = resp.text {
                info!("LLM: {}", text);
            }

            // If no tool calls, the LLM is done — move to post-tools hook
            let tool_calls = match resp.tool_calls {
                Some(tc) if !tc.is_empty() => tc,
                _ => {
                    info!("LLM finished (no tool calls). Running post-tools hook.");
                    break;
                }
            };

            // Add assistant message with tool calls to conversation
            messages.push(Message {
                role: "assistant".into(),
                content: resp.text.unwrap_or_default(),
                tool_call_id: None,
                tool_calls: Some(tool_calls.clone()),
            });

            // Execute each tool call and collect results
            for tool_call in &tool_calls {
                let result = execute_tool(tool_call);

                // Add tool result to conversation
                messages.push(Message {
                    role: "tool".into(),
                    content: result,
                    tool_call_id: Some(tool_call.id.clone()),
                    tool_calls: None,
                });
            }
        }

        // Post-tools hook: run cargo test, commit or revert
        run_post_tools_hook();

        Ok(())
    }
}

fn execute_tool(tool_call: &ToolCall) -> String {
    info!("Executing tool: {} with args: {}", tool_call.name, tool_call.arguments);

    if tool_call.name == "bash" {
        if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool_call.arguments) {
            if let Some(cmd) = args.get("command").and_then(|c| c.as_str()) {
                info!("Running: {}", cmd);
                return match std::process::Command::new("bash").arg("-c").arg(cmd).output() {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        let exit_code = out.status.code().unwrap_or(-1);
                        if !stdout.is_empty() {
                            info!("stdout: {}", &stdout[..stdout.len().min(500)]);
                        }
                        if !stderr.is_empty() {
                            warn!("stderr: {}", &stderr[..stderr.len().min(500)]);
                        }
                        format!("exit_code: {}\nstdout:\n{}\nstderr:\n{}", exit_code, stdout, stderr)
                    }
                    Err(e) => format!("Failed to execute command: {}", e),
                };
            }
        }
        "Invalid bash tool arguments".to_string()
    } else {
        format!("Unknown tool: {}", tool_call.name)
    }
}

fn run_post_tools_hook() {
    info!("Running post-tools hook: cargo test");
    let output = std::process::Command::new("cargo")
        .arg("test")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            info!("Tests passed! Auto-committing.");
            git_commit_all();
        }
        Ok(out) => {
            warn!("Tests failed. Reverting changes.");
            warn!("stderr: {}", String::from_utf8_lossy(&out.stderr));
            git_reset_hard();
        }
        Err(e) => {
            error!("Failed to run cargo test: {}", e);
        }
    }
}
