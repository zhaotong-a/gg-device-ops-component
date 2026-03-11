use crate::config::SecurityConfig;
use crate::error::{DeviceOpsError, Result};
use crate::models::{Command, JobDocument};
use std::path::Path;

// ============================================================================
// Job Document Validation
// ============================================================================

pub fn validate_job_document(document: &JobDocument) -> Result<()> {
    if document.version != "1.0" {
        return Err(DeviceOpsError::InvalidJobDocument(format!(
            "Unsupported job document version: {}",
            document.version
        )));
    }

    if document.steps.is_empty() {
        return Err(DeviceOpsError::InvalidJobDocument(
            "Job document has no steps".to_string(),
        ));
    }

    let all_steps: Vec<&crate::models::JobStep> = document
        .steps
        .iter()
        .chain(document.final_step.as_ref().map(|s| s.as_ref()))
        .collect();

    for step in all_steps {
        if step.action.action_type != "runCommand" {
            return Err(DeviceOpsError::InvalidJobDocument(format!(
                "Unsupported action type: {}. Only 'runCommand' is supported",
                step.action.action_type
            )));
        }

        if step.action.input.command.len() > 4096 {
            return Err(DeviceOpsError::InvalidJobDocument(
                "Command too long (max 4096 characters)".to_string(),
            ));
        }

        if step.action.input.command.trim().is_empty() {
            return Err(DeviceOpsError::InvalidJobDocument(
                "Command cannot be empty".to_string(),
            ));
        }

        if let Some(timeout) = step.action.input.timeout {
            if timeout == 0 || timeout > 86400 {
                return Err(DeviceOpsError::InvalidJobDocument(
                    "Timeout must be between 1 and 86400 seconds (24 hours)".to_string(),
                ));
            }
        }
    }

    Ok(())
}

// ============================================================================
// Security Validation (Command Allowlist & Path Traversal)
// ============================================================================

pub struct SecurityValidator {
    /// Entries ending in `/` are directory prefixes; everything else is exact match.
    allowlist: Vec<String>,
}

impl SecurityValidator {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            allowlist: config.allowlist,
        }
    }

    pub fn validate(&self, command: &Command) -> Result<()> {
        let is_absolute = command.script_path.starts_with('/');

        if is_absolute {
            if self.has_path_traversal(&command.script_path) {
                return Err(DeviceOpsError::SecurityError(format!(
                    "Path traversal detected: {}",
                    command.script_path
                )));
            }

            let resolved_path = std::fs::canonicalize(&command.script_path)
                .map_err(|e| {
                    DeviceOpsError::SecurityError(format!(
                        "Cannot resolve path '{}': {} (file may not exist or is inaccessible)",
                        command.script_path, e
                    ))
                })?
                .to_string_lossy()
                .to_string();

            if !self.is_allowed(&resolved_path) {
                return Err(DeviceOpsError::SecurityError(format!(
                    "Command not in allowlist: {} (resolved: {})",
                    command.script_path, resolved_path
                )));
            }
        } else {
            if !self.is_allowed(&command.script_path) {
                return Err(DeviceOpsError::SecurityError(format!(
                    "Command not in allowlist: {}",
                    command.script_path
                )));
            }
        }

        Ok(())
    }

    /// Entries ending in `/` are directory prefixes; others are exact match.
    fn is_allowed(&self, command: &str) -> bool {
        let path = Path::new(command);
        self.allowlist.iter().any(|entry| {
            if entry.ends_with('/') {
                path.starts_with(entry)
            } else {
                command == entry
            }
        })
    }

    fn has_path_traversal(&self, path: &str) -> bool {
        if path.contains("..") {
            return true;
        }
        let lower = path.to_lowercase();
        lower.contains("%2e%2e") || lower.contains("%2f") || lower.contains("%5c")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobAction, JobInput, JobStep};

    fn make_doc(version: &str, action_type: &str, command: &str) -> JobDocument {
        JobDocument {
            version: version.to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "Test".to_string(),
                    action_type: action_type.to_string(),
                    input: JobInput { command: command.to_string(), args: None, timeout: None },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: None,
            include_std_out: None,
        }
    }

    fn cmd(path: &str) -> Command {
        Command { script_path: path.to_string(), args: vec![], run_as_user: None }
    }

    fn make_validator(entries: Vec<&str>) -> SecurityValidator {
        SecurityValidator::new(SecurityConfig {
            allowlist: entries.into_iter().map(String::from).collect(),
        })
    }

    // Job document validation
    #[test]
    fn test_validate_valid_document() {
        assert!(validate_job_document(&make_doc("1.0", "runCommand", "/opt/test.sh")).is_ok());
    }

    #[test]
    fn test_validate_invalid_version() {
        assert!(validate_job_document(&make_doc("2.0", "runCommand", "/opt/test.sh")).is_err());
    }

    #[test]
    fn test_validate_invalid_action_type() {
        assert!(validate_job_document(&make_doc("1.0", "invalidAction", "/opt/test.sh")).is_err());
    }

    #[test]
    fn test_validate_empty_command() {
        assert!(validate_job_document(&make_doc("1.0", "runCommand", "   ")).is_err());
    }

    // Security validation
    #[test]
    fn test_path_traversal_detection() {
        let v = make_validator(vec!["/opt/device-scripts/"]);
        assert!(v.has_path_traversal("/opt/../etc/passwd"));
        assert!(v.has_path_traversal("/opt/%2e%2e/etc/passwd"));
        assert!(!v.has_path_traversal("/opt/device-scripts/test.sh"));
        assert!(!v.has_path_traversal("/opt/scripts/backup~1.sh"));
    }

    #[test]
    fn test_exact_match_allowlist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let script = tmpdir.path().join("test.sh");
        std::fs::write(&script, "#!/bin/bash\necho hello").unwrap();
        let real_path = script.to_string_lossy().to_string();

        let v = make_validator(vec![&real_path]);
        assert!(v.validate(&cmd(&real_path)).is_ok());
        assert!(v.validate(&cmd("/tmp/does-not-exist-12345.sh")).is_err());
    }

    #[test]
    fn test_directory_prefix_allowlist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let script = tmpdir.path().join("script.sh");
        std::fs::write(&script, "#!/bin/bash\necho hello").unwrap();
        let real_path = script.to_string_lossy().to_string();
        let dir_entry = format!("{}/", tmpdir.path().to_string_lossy());

        let v = make_validator(vec![&dir_entry]);
        assert!(v.validate(&cmd(&real_path)).is_ok());
    }

    #[test]
    fn test_bare_command_allowlist() {
        let v = make_validator(vec!["hostname", "ifconfig"]);
        assert!(v.validate(&cmd("hostname")).is_ok());
        assert!(v.validate(&cmd("rm")).is_err());
    }

    #[test]
    fn test_empty_allowlist_denies_all() {
        let v = make_validator(vec![]);
        assert!(v.validate(&cmd("hostname")).is_err());
    }
}
