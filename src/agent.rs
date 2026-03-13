use crate::llm::{LlmClient, LlmRequest, ToolCall};
use crate::tools::{git_commit_all, git_reset_hard};
use tracing::{info, warn, error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    WaitingForUserInput,
    CallingLlm,
    ExecutingTools(Vec<ToolCall>),
    PostToolsHook,
    ShuttingDown,
    Error(String),
}

pub struct AgentCore {
    llm: Box<dyn LlmClient>,
    state: AgentState,
}

impl AgentCore {
    pub fn new(llm: Box<dyn LlmClient>) -> Self {
        Self {
            llm,
            state: AgentState::WaitingForUserInput,
        }
    }

    pub fn state(&self) -> &AgentState {
        &self.state
    }

    pub async fn run_cycle(&mut self, initial_prompt: String) -> Result<(), anyhow::Error> {
        loop {
            match &self.state {
                AgentState::WaitingForUserInput => {
                    info!("Agent received user input, transitioning to CallingLlm.");
                    self.state = AgentState::CallingLlm;
                }
                AgentState::CallingLlm => {
                    info!("Calling LLM...");
                    let req = LlmRequest {
                        system_prompt: "You are a self-improving coding agent. Do not summarize or compact your context. Execute tools.".into(),
                        user_prompt: initial_prompt.clone(),
                        max_tokens: 4096,
                        temperature: 0.1,
                    };
                    match self.llm.complete(req).await {
                        Ok(resp) => {
                            if let Some(tool_calls) = resp.tool_calls {
                                self.state = AgentState::ExecutingTools(tool_calls);
                            } else {
                                self.state = AgentState::ShuttingDown;
                            }
                        }
                        Err(e) => {
                            error!("LLM Call Failed: {:?}", e);
                            self.state = AgentState::Error(e.to_string());
                        }
                    }
                }
                AgentState::ExecutingTools(tools) => {
                    let tools_to_run = tools.clone();
                    for tool in tools_to_run {
                        info!("Executing Tool: {} with args: {}", tool.name, tool.arguments);
                        // Very simple MVP weaver: execute bash directly
                        if tool.name == "bash" {
                            if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool.arguments) {
                                if let Some(cmd) = args.get("command").and_then(|c| c.as_str()) {
                                    info!("Running bash command: {}", cmd);
                                    let output = std::process::Command::new("bash")
                                        .arg("-c")
                                        .arg(cmd)
                                        .output();
                                    match output {
                                        Ok(out) => info!("Command stdout: {}", String::from_utf8_lossy(&out.stdout)),
                                        Err(e) => error!("Command failed: {}", e),
                                    }
                                }
                            }
                        }
                    }
                    self.state = AgentState::PostToolsHook;
                }
                AgentState::PostToolsHook => {
                    info!("Running post-tools hook to verify and possibly auto-commit.");
                    // Check if tools succeeded, e.g., run `cargo test`
                    let output = std::process::Command::new("cargo")
                        .arg("test")
                        .output();
                    
                    if let Ok(out) = output {
                        if out.status.success() {
                            info!("Tests passed! Triggering auto-commit.");
                            git_commit_all();
                        } else {
                            warn!("Tests failed. Need to retry.");
                            git_reset_hard();
                        }
                    }
                    
                    self.state = AgentState::ShuttingDown;
                }
                AgentState::ShuttingDown => {
                    info!("Agent loop finished normally.");
                    break;
                }
                AgentState::Error(msg) => {
                    warn!("Agent entered error state: {}", msg);
                    break;
                }
            }
        }
        Ok(())
    }
}
