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
const MAX_OUTPUT_BYTES: usize = 1024; // AWS IoT Jobs statusDetails value limit: 1,024 chars per field

/// Trait for running commands - allows mocking in tests
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &Command, timeout_secs: u64) -> Result<ExecutionOutput>;
}

/// Real command runner that executes commands on the system
pub struct SystemCommandRunner;

#[async_trait]
impl CommandRunner for SystemCommandRunner {
    async fn run(&self, command: &Command, timeout_secs: u64) -> Result<ExecutionOutput> {
        tracing::info!(
            command = %command.script_path,
            args = ?command.args,
            run_as_user = ?command.run_as_user,
            timeout_secs = timeout_secs,
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
        let mut child = cmd.spawn().map_err(|e| {
            DeviceOpsError::ExecutionError(format!("Failed to spawn command: {}", e))
        })?;

        let timeout_duration = Duration::from_secs(timeout_secs);
        let start = std::time::Instant::now();

        let output = match timeout(timeout_duration, child.wait_with_output()).await {
            Ok(result) => result.map_err(|e| {
                DeviceOpsError::ExecutionError(format!("Failed to execute command: {}", e))
            })?,
            Err(_) => {
                // Timeout — kill the child process to avoid orphans
                tracing::error!(
                    timeout_secs = timeout_secs,
                    "Command execution timed out, killing process"
                );
                if let Err(e) = child.kill().await {
                    tracing::warn!(error = %e, "Failed to kill timed-out process");
                }
                return Err(DeviceOpsError::TimeoutError(timeout_secs));
            }
        };

        let execution_time_ms = start.elapsed().as_millis() as u64;

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
            execution_time_ms = execution_time_ms,
            "Command execution completed"
        );

        Ok(ExecutionOutput {
            stdout,
            stderr,
            exit_code,
            execution_time_ms,
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

            let ignore_failure = step.action.ignore_step_failure.unwrap_or(false);

            let output = match self.execute_step(&step.action).await {
                Ok(output) => output,
                Err(e) => ExecutionOutput::from_error(&e),
            };

            let step_failed = !self.evaluate_step_success(&output, &step.action);

            if step_failed && ignore_failure {
                tracing::warn!(
                    step_name = %step.action.name,
                    "Step failed but ignoreStepFailure=true, continuing"
                );
            } else if step_failed {
                tracing::error!(
                    step_name = %step.action.name,
                    exit_code = output.exit_code,
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

            outputs.push(StepOutput {
                step_name: step.action.name.clone(),
                output,
                ignored_failure: step_failed,
            });
        }

        // Execute final step always (like try/finally — for cleanup/summary)
        if let Some(final_step) = &job_document.final_step {
            tracing::info!(
                step_name = %final_step.action.name,
                overall_success = overall_success,
                "Executing final step"
            );

            let output = match self.execute_step(&final_step.action).await {
                Ok(output) => output,
                Err(e) => {
                    tracing::error!(
                        step_name = %final_step.action.name,
                        error = %e,
                        "Final step execution failed"
                    );
                    ExecutionOutput::from_error(&e)
                }
            };

            let step_failed = !self.evaluate_step_success(&output, &final_step.action);
            if step_failed {
                overall_success = false;
                failed_step = Some(final_step.action.name.clone());
            }

            outputs.push(StepOutput {
                step_name: final_step.action.name.clone(),
                output,
                ignored_failure: false,
            });
        }

        Ok(JobExecutionResult {
            outputs,
            overall_success,
            failed_step,
        })
    }

    /// Execute a single step
    async fn execute_step(&self, action: &crate::models::JobAction) -> Result<ExecutionOutput> {
        let command = self.build_command(action).await?;

        // Security validation (if enabled)
        if let Some(validator) = &self.security {
            validator.validate(&command)?;
        }

        let timeout_secs = action.input.timeout.unwrap_or(self.config.default_timeout);

        self.runner.run(&command, timeout_secs).await
    }

    /// Build command with sudo support if runAsUser is specified
    async fn build_command(&self, action: &crate::models::JobAction) -> Result<Command> {
        let run_as_user = if let Some(user) = &action.run_as_user {
            if self.verify_sudo_and_user(user).await? {
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

    /// Verify that sudo and the specified user exist (async-safe)
    async fn verify_sudo_and_user(&self, user: &str) -> Result<bool> {
        // Check if sudo exists
        let sudo_check = TokioCommand::new("which")
            .arg("sudo")
            .output()
            .await
            .map_err(|e| {
                DeviceOpsError::ExecutionError(format!("Failed to check for sudo: {}", e))
            })?;

        if !sudo_check.status.success() {
            tracing::warn!("sudo command not found");
            return Ok(false);
        }

        // Check if user exists
        let user_check = TokioCommand::new("id")
            .arg(user)
            .output()
            .await
            .map_err(|e| {
                DeviceOpsError::ExecutionError(format!("Failed to check for user: {}", e))
            })?;

        if !user_check.status.success() {
            tracing::warn!(user = %user, "User does not exist");
            return Ok(false);
        }

        // Verify passwordless sudo is configured by testing with -n flag
        let sudo_test = TokioCommand::new("sudo")
            .arg("-n")
            .arg("-u")
            .arg(user)
            .arg("true")
            .output()
            .await
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
        async fn run(&self, _command: &Command, _timeout_secs: u64) -> Result<ExecutionOutput> {
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
    // Test Helpers
    // ========================================================================

    fn ok_output(stdout: &str, exit_code: i32) -> Result<ExecutionOutput> {
        Ok(ExecutionOutput {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code,
            execution_time_ms: 0,
            stderr_line_count: 0,
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    fn ok_output_with_stderr(stderr: &str, stderr_lines: usize) -> Result<ExecutionOutput> {
        Ok(ExecutionOutput {
            stdout: String::new(),
            stderr: stderr.to_string(),
            exit_code: 0,
            execution_time_ms: 0,
            stderr_line_count: stderr_lines,
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    fn step(name: &str) -> JobStep {
        JobStep {
            action: JobAction {
                name: name.to_string(),
                action_type: "runCommand".to_string(),
                input: JobInput {
                    command: "test".to_string(),
                    args: None,
                    timeout: None,
                },
                run_as_user: None,
                ignore_step_failure: None,
                allow_std_err: None,
            },
        }
    }

    fn step_with_ignore(name: &str) -> JobStep {
        let mut s = step(name);
        s.action.ignore_step_failure = Some(true);
        s
    }

    fn step_with_allow_stderr(name: &str, allowed: i32) -> JobStep {
        let mut s = step(name);
        s.action.allow_std_err = Some(allowed);
        s
    }

    fn doc(steps: Vec<JobStep>) -> JobDocument {
        JobDocument {
            version: "1.0".to_string(),
            steps,
            final_step: None,
            include_std_out: None,
        }
    }

    fn doc_with_final(steps: Vec<JobStep>, final_step: JobStep) -> JobDocument {
        JobDocument {
            version: "1.0".to_string(),
            steps,
            final_step: Some(Box::new(final_step)),
            include_std_out: None,
        }
    }

    fn make_executor(responses: Vec<Result<ExecutionOutput>>) -> CommandExecutor<MockCommandRunner> {
        let config = ExecutionConfig { default_timeout: 300 };
        let mock = MockCommandRunner::new(responses);
        CommandExecutor::new_with_runner(config, None, mock)
    }

    // ========================================================================
    // UNIT TESTS
    // ========================================================================

    #[tokio::test]
    async fn test_single_step_execution_logic() {
        let executor = make_executor(vec![ok_output("hello", 0)]);
        let result = executor.execute(&doc(vec![step("Test")])).await.unwrap();

        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.outputs[0].output.stdout, "hello");
    }

    #[tokio::test]
    async fn test_multi_step_execution_logic() {
        let executor = make_executor(vec![ok_output("step1", 0), ok_output("step2", 0)]);
        let result = executor.execute(&doc(vec![step("Step1"), step("Step2")])).await.unwrap();

        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert_eq!(result.outputs[0].output.stdout, "step1");
        assert_eq!(result.outputs[1].output.stdout, "step2");
    }

    #[tokio::test]
    async fn test_ignore_step_failure_logic() {
        let executor = make_executor(vec![ok_output("", 1), ok_output("success", 0)]);
        let result = executor
            .execute(&doc(vec![step_with_ignore("FailingStep"), step("SuccessStep")]))
            .await
            .unwrap();

        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert!(result.outputs[0].ignored_failure);
        assert_eq!(result.outputs[1].output.stdout, "success");
    }

    #[tokio::test]
    async fn test_final_step_execution_logic() {
        let executor = make_executor(vec![ok_output("main", 0), ok_output("final", 0)]);
        let result = executor
            .execute(&doc_with_final(vec![step("MainStep")], step("FinalStep")))
            .await
            .unwrap();

        assert!(result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert_eq!(result.outputs[0].step_name, "MainStep");
        assert_eq!(result.outputs[1].step_name, "FinalStep");
    }

    #[tokio::test]
    async fn test_allow_std_err_logic() {
        let executor = make_executor(vec![ok_output_with_stderr("error\n", 1)]);
        let result = executor
            .execute(&doc(vec![step_with_allow_stderr("StderrStep", 1)]))
            .await
            .unwrap();

        assert!(result.overall_success);
        assert_eq!(result.outputs[0].output.stderr_line_count, 1);
    }

    #[tokio::test]
    async fn test_step_failure_stops_execution() {
        let executor = make_executor(vec![ok_output("", 1)]);
        let result = executor
            .execute(&doc(vec![step("FailingStep"), step("ShouldNotRun")]))
            .await
            .unwrap();

        assert!(!result.overall_success);
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.failed_step, Some("FailingStep".to_string()));
    }

    #[tokio::test]
    async fn test_final_step_runs_even_on_failure() {
        let executor = make_executor(vec![ok_output("", 1), ok_output("cleanup done", 0)]);
        let result = executor
            .execute(&doc_with_final(vec![step("FailingStep")], step("FinalStep")))
            .await
            .unwrap();

        assert!(!result.overall_success);
        assert_eq!(result.outputs.len(), 2);
        assert_eq!(result.outputs[0].step_name, "FailingStep");
        assert_eq!(result.outputs[1].step_name, "FinalStep");
        assert_eq!(result.outputs[1].output.stdout, "cleanup done");
    }
}
