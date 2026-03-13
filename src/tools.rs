use std::process::Command;
use tracing::{info, warn, error};

pub fn git_commit_all() {
    let output = Command::new("git")
        .arg("add")
        .arg("-A")
        .output();
        
    match output {
        Ok(out) if out.status.success() => {
            let commit = Command::new("git")
                .arg("commit")
                .arg("-m")
                .arg("Auto-commit: Tests passed after agent execution.")
                .output();
                
            match commit {
                Ok(c_out) if c_out.status.success() => info!("Successfully created auto-commit."),
                Ok(c_out) => warn!("Git commit issue: {}", String::from_utf8_lossy(&c_out.stderr)),
                Err(e) => error!("Failed to execute git commit: {}", e),
            }
        }
        Ok(out) => warn!("Git add failed: {}", String::from_utf8_lossy(&out.stderr)),
        Err(e) => error!("Failed to execute git add: {}", e),
    }
}

pub fn git_reset_hard() {
    let output = Command::new("git")
        .arg("reset")
        .arg("--hard")
        .output();
        
    match output {
        Ok(out) if out.status.success() => info!("Hard reset to HEAD after failure."),
        Ok(out) => warn!("Git reset failed: {}", String::from_utf8_lossy(&out.stderr)),
        Err(e) => error!("Failed to execute git reset: {}", e),
    }
}
