use std::process::Command;
use tracing::{info, error};
use crate::llm::ToolDefinition;

// -----------------------------------------------------------------------------
// VCS trait – version control abstraction
// -----------------------------------------------------------------------------

pub trait Vcs {
    /// Commit all current changes with the given message.
    fn commit_all(&self, message: &str) -> Result<(), anyhow::Error>;

    /// Revert to the last committed state.
    fn revert(&self) -> Result<(), anyhow::Error>;

    /// Get a summary of current changes.
    #[allow(dead_code)]
    fn status(&self) -> Result<String, anyhow::Error>;
}

// -----------------------------------------------------------------------------
// GitVcs – concrete implementation
// -----------------------------------------------------------------------------

pub struct GitVcs;

impl Vcs for GitVcs {
    fn commit_all(&self, message: &str) -> Result<(), anyhow::Error> {
        let add_output = Command::new("git")
            .arg("add")
            .arg("-A")
            .output()?;

        if !add_output.status.success() {
            anyhow::bail!(
                "git add -A failed: {}",
                String::from_utf8_lossy(&add_output.stderr)
            );
        }

        let commit_output = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(message)
            .output()?;

        if !commit_output.status.success() {
            anyhow::bail!(
                "git commit failed: {}",
                String::from_utf8_lossy(&commit_output.stderr)
            );
        }

        info!("Successfully created commit: {}", message);
        Ok(())
    }

    fn revert(&self) -> Result<(), anyhow::Error> {
        let output = Command::new("git")
            .arg("reset")
            .arg("--hard")
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git reset --hard failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        info!("Hard reset to HEAD.");
        Ok(())
    }

    fn status(&self) -> Result<String, anyhow::Error> {
        let output = Command::new("git")
            .arg("status")
            .arg("--short")
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git status --short failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// -----------------------------------------------------------------------------
// Backward-compatible free functions
// -----------------------------------------------------------------------------

pub fn git_commit_all() {
    let vcs = GitVcs;
    if let Err(e) = vcs.commit_all("Auto-commit: Tests passed after agent execution.") {
        error!("Auto-commit failed: {}", e);
    }
}

pub fn git_reset_hard() {
    let vcs = GitVcs;
    if let Err(e) = vcs.revert() {
        error!("Git reset failed: {}", e);
    }
}

// -----------------------------------------------------------------------------
// Tool definitions for the LLM
// -----------------------------------------------------------------------------

pub fn bash_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "bash".to_string(),
        description: "Execute a bash command and return its output. Use this to run shell commands, read files, write files, and interact with the system.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                }
            },
            "required": ["command"]
        }),
    }
}
