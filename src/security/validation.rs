use crate::config::SecurityConfig;
use crate::error::{DeviceOpsError, Result};
use crate::models::{Command, JobDocument};
use std::path::Path;

// ============================================================================
// Job Document Validation
// ============================================================================

pub fn validate_job_document(document: &JobDocument) -> Result<()> {
    // Validate version
    if document.version != "1.0" {
        return Err(DeviceOpsError::InvalidJobDocument(format!(
            "Unsupported job document version: {}",
            document.version
        )));
    }

    // Validate steps exist
    if document.steps.is_empty() {
        return Err(DeviceOpsError::InvalidJobDocument(
            "Job document has no steps".to_string(),
        ));
    }

    // Validate all steps and final step
    let all_steps: Vec<&crate::models::JobStep> = document
        .steps
        .iter()
        .chain(document.final_step.as_ref().map(|s| s.as_ref()))
        .collect();

    for step in all_steps {
        // Validate action type
        if step.action.action_type != "runCommand" {
            return Err(DeviceOpsError::InvalidJobDocument(format!(
                "Unsupported action type: {}. Only 'runCommand' is supported",
                step.action.action_type
            )));
        }

        // Validate command length
        if step.action.input.command.len() > 4096 {
            return Err(DeviceOpsError::InvalidJobDocument(
                "Command too long (max 4096 characters)".to_string(),
            ));
        }

        // Validate command is not empty
        if step.action.input.command.trim().is_empty() {
            return Err(DeviceOpsError::InvalidJobDocument(
                "Command cannot be empty".to_string(),
            ));
        }

        // Validate timeout is reasonable
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
    command_allowlist: Vec<String>,
    path_allowlist: Vec<String>,
}

impl SecurityValidator {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            command_allowlist: config.command_allowlist,
            path_allowlist: config.path_allowlist,
        }
    }

    pub fn validate(&self, command: &Command) -> Result<()> {
        // Check for path traversal
        if self.has_path_traversal(&command.script_path) {
            return Err(DeviceOpsError::SecurityError(format!(
                "Path traversal detected: {}",
                command.script_path
            )));
        }

        // Check if command is in allowlist
        if !self.command_allowlist.is_empty() && !self.is_command_allowed(&command.script_path) {
            return Err(DeviceOpsError::SecurityError(format!(
                "Command not in allowlist: {}",
                command.script_path
            )));
        }

        // Check if path is in allowed paths
        if !self.path_allowlist.is_empty() && !self.is_path_allowed(&command.script_path) {
            return Err(DeviceOpsError::SecurityError(format!(
                "Path not in allowlist: {}",
                command.script_path
            )));
        }

        Ok(())
    }

    fn is_command_allowed(&self, script_path: &str) -> bool {
        self.command_allowlist
            .iter()
            .any(|allowed| script_path == allowed)
    }

    fn is_path_allowed(&self, script_path: &str) -> bool {
        let path = Path::new(script_path);
        self.path_allowlist
            .iter()
            .any(|allowed_path| path.starts_with(allowed_path))
    }

    fn has_path_traversal(&self, path: &str) -> bool {
        // Check for common path traversal patterns
        if path.contains("..") || path.contains("~") {
            return true;
        }

        // Check for encoded path traversal attempts
        let lower = path.to_lowercase();
        if lower.contains("%2e%2e") || lower.contains("%2f") || lower.contains("%5c") {
            return true;
        }

        // Reject relative paths - only allow absolute paths
        if !path.starts_with('/') {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobAction, JobInput, JobStep};

    // ========================================================================
    // Job Document Validation Tests
    // ========================================================================

    #[test]
    fn test_validate_valid_document() {
        let doc = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "Test".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "/opt/test.sh".to_string(),
                        args: None,
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: None,
            include_std_out: None,
        };

        assert!(validate_job_document(&doc).is_ok());
    }

    #[test]
    fn test_validate_invalid_version() {
        let doc = JobDocument {
            version: "2.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "Test".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "/opt/test.sh".to_string(),
                        args: None,
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: None,
            include_std_out: None,
        };

        assert!(validate_job_document(&doc).is_err());
    }

    #[test]
    fn test_validate_invalid_action_type() {
        let doc = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "Test".to_string(),
                    action_type: "invalidAction".to_string(),
                    input: JobInput {
                        command: "/opt/test.sh".to_string(),
                        args: None,
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: None,
            include_std_out: None,
        };

        assert!(validate_job_document(&doc).is_err());
    }

    #[test]
    fn test_validate_empty_command() {
        let doc = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "Test".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "   ".to_string(),
                        args: None,
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: None,
            include_std_out: None,
        };

        assert!(validate_job_document(&doc).is_err());
    }

    // ========================================================================
    // Security Validation Tests
    // ========================================================================

    #[test]
    fn test_path_traversal_detection() {
        let config = SecurityConfig {
            enabled: true,
            command_allowlist: vec![],
            path_allowlist: vec![],
        };
        let validator = SecurityValidator::new(config);

        // Test basic path traversal
        let command = Command {
            script_path: "../etc/passwd".to_string(),
            args: vec![],
            run_as_user: None,
        };
        assert!(validator.validate(&command).is_err());

        // Test encoded path traversal
        let command2 = Command {
            script_path: "/opt/%2e%2e/etc/passwd".to_string(),
            args: vec![],
            run_as_user: None,
        };
        assert!(validator.validate(&command2).is_err());

        // Test relative path
        let command3 = Command {
            script_path: "relative/path.sh".to_string(),
            args: vec![],
            run_as_user: None,
        };
        assert!(validator.validate(&command3).is_err());
    }

    #[test]
    fn test_command_allowlist() {
        let config = SecurityConfig {
            enabled: true,
            command_allowlist: vec!["/opt/device-scripts/test.sh".to_string()],
            path_allowlist: vec![],
        };
        let validator = SecurityValidator::new(config);

        let allowed_command = Command {
            script_path: "/opt/device-scripts/test.sh".to_string(),
            args: vec![],
            run_as_user: None,
        };

        assert!(validator.validate(&allowed_command).is_ok());

        let disallowed_command = Command {
            script_path: "/tmp/malicious.sh".to_string(),
            args: vec![],
            run_as_user: None,
        };

        assert!(validator.validate(&disallowed_command).is_err());
    }
}
