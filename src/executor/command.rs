use crate::config::ExecutionConfig;
use crate::error::{DeviceOpsError, Result};
use crate::models::{Command, ExecutionOutput, JobDocument, JobExecutionResult, StepOutput};
use crate::security::SecurityValidator;
use async_trait::async_trait;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

const MAX_OUTPUT_LINES: usize = 1000;
const MAX_OUTPUT_BYTES: usize = 32 * 1024; // 32KB limit for IoT Jobs statusDetails

/// Trait for running commands - allows mocking in tests
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &Command) -> Result<ExecutionOutput>;
}

/// Real command runner that executes commands on the system
pub struct SystemCommandRunner;

#[async_trait]
impl CommandRunner for SystemCommandRunner {
    async fn run(&self, command: &Command) -> Result<ExecutionOutput> {
        tracing::info!(
            script = %command.script_path,
            args = ?command.args,
            run_as_user = ?command.run_as_user,
            "Executing command"
        );

        let mut cmd = if let Some(user) = &command.run_as_user {
            // Build: sudo -u $user -n command args...
            let mut sudo_cmd = TokioCommand::new("sudo");
            sudo_cmd.arg("-u").arg(user).arg("-n");
            sudo_cmd.arg(&command.script_path);
            sudo_cmd.args(&command.args);
            sudo_cmd
        } else {
            let mut cmd = TokioCommand::new(&command.script_path);
            cmd.args(&command.args);
            cmd
        };

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Spawn the process so we can kill it on timeout
        let child = cmd.spawn().map_err(|e| {
            DeviceOpsError::ExecutionError(format!("Failed to spawn command: {}", e))
        })?;

        let output = child.wait_with_output().await.map_err(|e| {
            DeviceOpsError::ExecutionError(format!("Failed to execute command: {}", e))
        })?;

        let (stdout, stdout_truncated) = Self::limit_output(&output.stdout);
        let (stderr, stderr_truncated) = Self::limit_output(&output.stderr);
        let stderr_line_count = stderr.lines().count();
        let exit_code = output.status.code().unwrap_or(-1);

        tracing::info!(
            exit_code = exit_code,
            stdout_len = stdout.len(),
            stderr_len = stderr.len(),
            stderr_lines = stderr_line_count,
            stdout_truncated = stdout_truncated,
            stderr_truncated = stderr_truncated,
            "Command execution completed"
        );

        Ok(ExecutionOutput {
            stdout,
            stderr,
            exit_code,
            execution_time_ms: 0, // Will be set by caller
            stderr_line_count,
            stdout_truncated,
            stderr_truncated,
        })
    }
}

impl SystemCommandRunner {
    /// Limit output to MAX_OUTPUT_LINES and MAX_OUTPUT_BYTES
    fn limit_output(bytes: &[u8]) -> (String, bool) {
        let full_output = String::from_utf8_lossy(bytes);
        let lines: Vec<&str> = full_output.lines().collect();

        let mut truncated = false;
        let mut result = String::new();

        // Limit by line count
        let lines_to_take = if lines.len() > MAX_OUTPUT_LINES {
            truncated = true;
            MAX_OUTPUT_LINES
        } else {
            lines.len()
        };

        for (idx, line) in lines.iter().take(lines_to_take).enumerate() {
            if idx > 0 {
                result.push('\n');
            }
            result.push_str(line);

            // Check if we're approaching byte limit
            if result.len() > MAX_OUTPUT_BYTES - 100 {
                truncated = true;
                break;
            }
        }

        if truncated {
            result.push_str("\n[Output truncated: exceeded limit]");
        }

        // Final truncation to ensure we don't exceed byte limit
        if result.len() > MAX_OUTPUT_BYTES {
            result.truncate(MAX_OUTPUT_BYTES - 50);
            result.push_str("\n[Output truncated: size limit]");
        }

        (result, truncated)
    }
}

pub struct CommandExecutor<R: CommandRunner = SystemCommandRunner> {
    config: ExecutionConfig,
    security: Option<SecurityValidator>,
    runner: R,
}

impl CommandExecutor<SystemCommandRunner> {
    pub fn new(config: ExecutionConfig, security: Option<SecurityValidator>) -> Self {
        Self {
            config,
            security,
            runner: SystemCommandRunner,
        }
    }
}

impl<R: CommandRunner> CommandExecutor<R> {
    /// Create executor with custom runner (for testing)
    #[cfg(test)]
    pub fn new_with_runner(
        config: ExecutionConfig,
        security: Option<SecurityValidator>,
        runner: R,
    ) -> Self {
        Self {
            config,
            security,
            runner,
        }
    }

    /// Execute all steps in the job document sequentially
    pub async fn execute(&self, job_document: &JobDocument) -> Result<JobExecutionResult> {
        let mut outputs = Vec::new();
        let mut overall_success = true;
        let mut failed_step = None;

        // Execute all steps in sequence
        for (idx, step) in job_document.steps.iter().enumerate() {
            tracing::info!(
                step_number = idx + 1,
                step_name = %step.action.name,
                "Executing step"
            );

            match self.execute_step(&step.action).await {
                Ok(output) => {
                    let step_failed = !self.evaluate_step_success(&output, &step.action);
                    let ignore_failure = step.action.ignore_step_failure.unwrap_or(false);

                    if step_failed && !ignore_failure {
                        tracing::error!(
                            step_name = %step.action.name,
                            exit_code = output.exit_code,
                            stderr_lines = output.stderr_line_count,
                            "Step failed"
                        );
                        overall_success = false;
                        failed_step = Some(step.action.name.clone());

                        outputs.push(StepOutput {
                            step_name: step.action.name.clone(),
                            output,
                            ignored_failure: false,
                        });
                        break;
                    }

                    if step_failed && ignore_failure {
                        tracing::warn!(
                            step_name = %step.action.name,
                            "Step failed but ignoreStepFailure=true, continuing"
                        );
                    }

                    outputs.push(StepOutput {
                        step_name: step.action.name.clone(),
                        output,
                        ignored_failure: step_failed && ignore_failure,
                    });
                }
                Err(e) => {
                    let ignore_failure = step.action.ignore_step_failure.unwrap_or(false);

                    if !ignore_failure {
                        tracing::error!(
                            step_name = %step.action.name,
                            error = %e,
                            "Step execution failed"
                        );
                        overall_success = false;
                        failed_step = Some(step.action.name.clone());
                        break;
                    }

                    tracing::warn!(
                        step_name = %step.action.name,
                        error = %e,
                        "Step execution failed but ignoreStepFailure=true, continuing"
                    );
                }
            }
        }

        // Execute final step if all steps succeeded
        if overall_success {
            if let Some(final_step) = &job_document.final_step {
                tracing::info!(
                    step_name = %final_step.action.name,
                    "Executing final step"
                );

                match self.execute_step(&final_step.action).await {
                    Ok(output) => {
                        let step_failed = !self.evaluate_step_success(&output, &final_step.action);

                        if step_failed {
                            tracing::error!(
                                step_name = %final_step.action.name,
                                "Final step failed"
                            );
                            overall_success = false;
                            failed_step = Some(final_step.action.name.clone());
                        }

                        outputs.push(StepOutput {
                            step_name: final_step.action.name.clone(),
                            output,
                            ignored_failure: false,
                        });
                    }
                    Err(e) => {
                        tracing::error!(
                            step_name = %final_step.action.name,
                            error = %e,
                            "Final step execution failed"
                        );
                        overall_success = false;
                        failed_step = Some(final_step.action.name.clone());
                    }
                }
            }
        }

        Ok(JobExecutionResult {
            outputs,
            overall_success,
            failed_step,
        })
    }

    /// Execute a single step
    async fn execute_step(&self, action: &crate::models::JobAction) -> Result<ExecutionOutput> {
        let command = self.build_command(action)?;

        // Security validation (if enabled)
        if let Some(validator) = &self.security {
            validator.validate(&command)?;
        }

        // Execute with timeout
        let timeout_duration =
            Duration::from_secs(action.input.timeout.unwrap_or(self.config.default_timeout));

        let start = std::time::Instant::now();

        let output = match timeout(timeout_duration, self.runner.run(&command)).await {
            Ok(result) => result?,
            Err(_) => {
                tracing::error!(
                    timeout_secs = timeout_duration.as_secs(),
                    "Command execution timed out"
                );
                return Err(DeviceOpsError::TimeoutError(timeout_duration.as_secs()));
            }
        };

        let execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(ExecutionOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
            execution_time_ms,
            stderr_line_count: output.stderr_line_count,
            stdout_truncated: output.stdout_truncated,
            stderr_truncated: output.stderr_truncated,
        })
    }

    /// Build command with sudo support if runAsUser is specified
    fn build_command(&self, action: &crate::models::JobAction) -> Result<Command> {
        let run_as_user = if let Some(user) = &action.run_as_user {
            if self.verify_sudo_and_user(user)? {
                Some(user.clone())
            } else {
                tracing::warn!(
                    user = %user,
                    "sudo or user not found, running as current user"
                );
                None
            }
        } else {
            None
        };

        Ok(Command {
            script_path: action.input.command.clone(),
            args: action.input.args.clone().unwrap_or_default(),
            run_as_user,
        })
    }

    /// Verify that sudo and the specified user exist
    fn verify_sudo_and_user(&self, user: &str) -> Result<bool> {
        // Check if sudo exists
        let sudo_check = std::process::Command::new("which")
            .arg("sudo")
            .output()
            .map_err(|e| {
                DeviceOpsError::ExecutionError(format!("Failed to check for sudo: {}", e))
            })?;

        if !sudo_check.status.success() {
            tracing::warn!("sudo command not found");
            return Ok(false);
        }

        // Check if user exists
        let user_check = std::process::Command::new("id")
            .arg(user)
            .output()
            .map_err(|e| {
                DeviceOpsError::ExecutionError(format!("Failed to check for user: {}", e))
            })?;

        if !user_check.status.success() {
            tracing::warn!(user = %user, "User does not exist");
            return Ok(false);
        }

        // Verify passwordless sudo is configured by testing with -n flag
        let sudo_test = std::process::Command::new("sudo")
            .arg("-n")
            .arg("-u")
            .arg(user)
            .arg("true")
            .output()
            .map_err(|e| {
                DeviceOpsError::ExecutionError(format!("Failed to test sudo access: {}", e))
            })?;

        if !sudo_test.status.success() {
            tracing::warn!(
                user = %user,
                "Passwordless sudo not configured for user"
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// Evaluate if a step succeeded based on exit code and stderr
    fn evaluate_step_success(
        &self,
        output: &ExecutionOutput,
        action: &crate::models::JobAction,
    ) -> bool {
        // Check exit code
        if output.exit_code != 0 {
            return false;
        }

        // Check stderr line count against allowStdErr
        let allowed_stderr = action.allow_std_err.unwrap_or(0);
        if output.stderr_line_count > allowed_stderr as usize {
            tracing::warn!(
                stderr_lines = output.stderr_line_count,
                allowed = allowed_stderr,
                "Step produced more stderr lines than allowed"
            );
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobAction, JobInput, JobStep};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Mock command runner for unit tests
    struct MockCommandRunner {
        responses: Arc<Mutex<VecDeque<Result<ExecutionOutput>>>>,
    }

    impl MockCommandRunner {
        fn new(responses: Vec<Result<ExecutionOutput>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into())),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for MockCommandRunner {
        async fn run(&self, _command: &Command) -> Result<ExecutionOutput> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(DeviceOpsError::ExecutionError(
                        "No more mock responses".to_string(),
                    ))
                })
        }
    }

    // ========================================================================
    // UNIT TESTS - Test logic without running real commands
    // ========================================================================

    #[tokio::test]
    async fn test_single_step_execution_logic() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![Ok(ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: 0,
            execution_time_ms: 0,
            stderr_line_count: 0,
            stdout_truncated: false,
            stderr_truncated: false,
        })]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "Test".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "echo".to_string(),
                        args: Some(vec!["hello".to_string()]),
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

        let result = executor.execute(&document).await.unwrap();
        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.outputs[0].output.stdout, "hello");
        assert_eq!(result.outputs[0].output.exit_code, 0);
    }

    #[tokio::test]
    async fn test_multi_step_execution_logic() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![
            Ok(ExecutionOutput {
                stdout: "step1".to_string(),
                stderr: String::new(),
                exit_code: 0,
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
            Ok(ExecutionOutput {
                stdout: "step2".to_string(),
                stderr: String::new(),
                exit_code: 0,
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
        ]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![
                JobStep {
                    action: JobAction {
                        name: "Step1".to_string(),
                        action_type: "runCommand".to_string(),
                        input: JobInput {
                            command: "echo".to_string(),
                            args: Some(vec!["step1".to_string()]),
                            timeout: None,
                        },
                        run_as_user: None,
                        ignore_step_failure: None,
                        allow_std_err: None,
                    },
                },
                JobStep {
                    action: JobAction {
                        name: "Step2".to_string(),
                        action_type: "runCommand".to_string(),
                        input: JobInput {
                            command: "echo".to_string(),
                            args: Some(vec!["step2".to_string()]),
                            timeout: None,
                        },
                        run_as_user: None,
                        ignore_step_failure: None,
                        allow_std_err: None,
                    },
                },
            ],
            final_step: None,
            include_std_out: None,
        };

        let result = executor.execute(&document).await.unwrap();
        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert_eq!(result.outputs[0].output.stdout, "step1");
        assert_eq!(result.outputs[1].output.stdout, "step2");
    }

    #[tokio::test]
    async fn test_ignore_step_failure_logic() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![
            Ok(ExecutionOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1, // Failed
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
            Ok(ExecutionOutput {
                stdout: "success".to_string(),
                stderr: String::new(),
                exit_code: 0,
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
        ]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![
                JobStep {
                    action: JobAction {
                        name: "FailingStep".to_string(),
                        action_type: "runCommand".to_string(),
                        input: JobInput {
                            command: "false".to_string(),
                            args: None,
                            timeout: None,
                        },
                        run_as_user: None,
                        ignore_step_failure: Some(true),
                        allow_std_err: None,
                    },
                },
                JobStep {
                    action: JobAction {
                        name: "SuccessStep".to_string(),
                        action_type: "runCommand".to_string(),
                        input: JobInput {
                            command: "echo".to_string(),
                            args: Some(vec!["success".to_string()]),
                            timeout: None,
                        },
                        run_as_user: None,
                        ignore_step_failure: None,
                        allow_std_err: None,
                    },
                },
            ],
            final_step: None,
            include_std_out: None,
        };

        let result = executor.execute(&document).await.unwrap();
        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert!(result.outputs[0].ignored_failure);
        assert_eq!(result.outputs[1].output.stdout, "success");
    }

    #[tokio::test]
    async fn test_final_step_execution_logic() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![
            Ok(ExecutionOutput {
                stdout: "main".to_string(),
                stderr: String::new(),
                exit_code: 0,
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
            Ok(ExecutionOutput {
                stdout: "final".to_string(),
                stderr: String::new(),
                exit_code: 0,
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
        ]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "MainStep".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "echo".to_string(),
                        args: Some(vec!["main".to_string()]),
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: Some(Box::new(JobStep {
                action: JobAction {
                    name: "FinalStep".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "echo".to_string(),
                        args: Some(vec!["final".to_string()]),
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            })),
            include_std_out: None,
        };

        let result = executor.execute(&document).await.unwrap();
        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert_eq!(result.outputs[0].step_name, "MainStep");
        assert_eq!(result.outputs[1].step_name, "FinalStep");
    }

    #[tokio::test]
    async fn test_allow_std_err_logic() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![Ok(ExecutionOutput {
            stdout: String::new(),
            stderr: "error\n".to_string(),
            exit_code: 0,
            execution_time_ms: 0,
            stderr_line_count: 1,
            stdout_truncated: false,
            stderr_truncated: false,
        })]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "StderrStep".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "sh".to_string(),
                        args: Some(vec!["-c".to_string(), "echo error >&2".to_string()]),
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: Some(1), // Allow 1 line of stderr
                },
            }],
            final_step: None,
            include_std_out: None,
        };

        let result = executor.execute(&document).await.unwrap();
        assert!(result.overall_success);
        assert_eq!(result.outputs[0].output.stderr_line_count, 1);
    }

    #[tokio::test]
    async fn test_step_failure_stops_execution() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![
            Ok(ExecutionOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1, // Failed
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
            // Second step should not be called
        ]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![
                JobStep {
                    action: JobAction {
                        name: "FailingStep".to_string(),
                        action_type: "runCommand".to_string(),
                        input: JobInput {
                            command: "false".to_string(),
                            args: None,
                            timeout: None,
                        },
                        run_as_user: None,
                        ignore_step_failure: None,
                        allow_std_err: None,
                    },
                },
                JobStep {
                    action: JobAction {
                        name: "ShouldNotRun".to_string(),
                        action_type: "runCommand".to_string(),
                        input: JobInput {
                            command: "echo".to_string(),
                            args: Some(vec!["should not run".to_string()]),
                            timeout: None,
                        },
                        run_as_user: None,
                        ignore_step_failure: None,
                        allow_std_err: None,
                    },
                },
            ],
            final_step: None,
            include_std_out: None,
        };

        let result = executor.execute(&document).await.unwrap();
        assert!(!result.overall_success);
        assert_eq!(result.outputs.len(), 1); // Only first step executed
        assert_eq!(result.failed_step, Some("FailingStep".to_string()));
    }

    #[tokio::test]
    async fn test_final_step_not_run_on_failure() {
        let config = ExecutionConfig {
            default_timeout: 300,
        };

        let mock = MockCommandRunner::new(vec![
            Ok(ExecutionOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1, // Failed
                execution_time_ms: 0,
                stderr_line_count: 0,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
            // Final step should not be called
        ]);

        let executor = CommandExecutor::new_with_runner(config, None, mock);

        let document = JobDocument {
            version: "1.0".to_string(),
            steps: vec![JobStep {
                action: JobAction {
                    name: "FailingStep".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "false".to_string(),
                        args: None,
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            }],
            final_step: Some(Box::new(JobStep {
                action: JobAction {
                    name: "FinalStep".to_string(),
                    action_type: "runCommand".to_string(),
                    input: JobInput {
                        command: "echo".to_string(),
                        args: Some(vec!["cleanup".to_string()]),
                        timeout: None,
                    },
                    run_as_user: None,
                    ignore_step_failure: None,
                    allow_std_err: None,
                },
            })),
            include_std_out: None,
        };

        let result = executor.execute(&document).await.unwrap();
        assert!(!result.overall_success);
        assert_eq!(result.outputs.len(), 1); // Only failing step, no final step
    }
}
