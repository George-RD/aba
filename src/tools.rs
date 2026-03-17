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
// JjVcs – Jujutsu implementation
// -----------------------------------------------------------------------------

pub struct JjVcs;

impl Vcs for JjVcs {
    fn commit_all(&self, message: &str) -> Result<(), anyhow::Error> {
        // In JJ, the working copy IS a commit. We just need to describe it
        // and create a new empty working-copy change on top.
        let describe = Command::new("jj")
            .args(["describe", "-m", message])
            .output()?;

        if !describe.status.success() {
            anyhow::bail!(
                "jj describe failed: {}",
                String::from_utf8_lossy(&describe.stderr)
            );
        }

        // Create a new empty change on top (equivalent to "finishing" the current commit)
        let new = Command::new("jj").arg("new").output()?;

        if !new.status.success() {
            anyhow::bail!(
                "jj new failed: {}",
                String::from_utf8_lossy(&new.stderr)
            );
        }

        info!("JJ commit: {message}");
        Ok(())
    }

    fn revert(&self) -> Result<(), anyhow::Error> {
        // Restore the working copy to match the parent (undo all changes)
        let output = Command::new("jj")
            .args(["restore", "--from", "@-"])
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "jj restore failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        info!("JJ restored working copy to parent.");
        Ok(())
    }

    fn status(&self) -> Result<String, anyhow::Error> {
        let output = Command::new("jj")
            .args(["status"])
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "jj status failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// -----------------------------------------------------------------------------
// VCS detection and free functions
// -----------------------------------------------------------------------------

/// Detect which VCS is available (prefer JJ if colocated repo exists).
fn detect_vcs() -> Box<dyn Vcs> {
    if Command::new("jj").arg("root").output().is_ok_and(|o| o.status.success()) {
        Box::new(JjVcs)
    } else {
        Box::new(GitVcs)
    }
}

pub fn vcs_commit_all() {
    let vcs = detect_vcs();
    if let Err(e) = vcs.commit_all("Auto-commit: Tests passed after agent execution.") {
        error!("Auto-commit failed: {e}");
    }
}

pub fn vcs_revert() {
    let vcs = detect_vcs();
    if let Err(e) = vcs.revert() {
        error!("VCS revert failed: {e}");
    }
}

// Backward-compatible aliases
pub fn git_commit_all() {
    vcs_commit_all();
}

pub fn git_reset_hard() {
    vcs_revert();
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
