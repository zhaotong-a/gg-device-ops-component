use serde::{Deserialize, Serialize};

/// IoT Jobs notification wrapper
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobNotification {
    pub timestamp: Option<i64>,
    pub execution: Option<JobExecution>,
}

/// Job execution details from IoT Jobs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobExecution {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub status: String,
    #[serde(rename = "queuedAt")]
    pub queued_at: Option<i64>,
    #[serde(rename = "jobDocument")]
    pub job_document: JobDocument,
}

/// Internal job representation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Job {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub document: JobDocument,
}

/// Job or parse error - used to handle malformed job notifications
#[derive(Debug, Clone)]
pub enum JobOrError {
    Valid(Job),
    ParseError { job_id: String, error: String },
}

impl From<JobNotification> for Option<Job> {
    fn from(notification: JobNotification) -> Self {
        notification.execution.map(|exec| Job {
            job_id: exec.job_id,
            document: exec.job_document,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobDocument {
    pub version: String,
    pub steps: Vec<JobStep>,
    #[serde(rename = "finalStep", default)]
    pub final_step: Option<Box<JobStep>>,
    #[serde(rename = "includeStdOut", default)]
    pub include_std_out: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobStep {
    pub action: JobAction,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobAction {
    pub name: String,
    #[serde(rename = "type")]
    pub action_type: String,
    pub input: JobInput,
    #[serde(rename = "runAsUser", default)]
    pub run_as_user: Option<String>,
    #[serde(rename = "ignoreStepFailure", default)]
    pub ignore_step_failure: Option<bool>,
    #[serde(rename = "allowStdErr", default)]
    pub allow_std_err: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobInput {
    pub command: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub execution_time_ms: u64,
    pub stderr_line_count: usize,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub script_path: String,
    pub args: Vec<String>,
    pub run_as_user: Option<String>,
}

/// Aggregated result from executing all steps
#[derive(Debug, Clone)]
pub struct JobExecutionResult {
    pub outputs: Vec<StepOutput>,
    pub overall_success: bool,
    pub failed_step: Option<String>,
}

/// Output from a single step execution
#[derive(Debug, Clone)]
pub struct StepOutput {
    pub step_name: String,
    pub output: ExecutionOutput,
    pub ignored_failure: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_job_document() {
        let json = r#"{
            "version": "1.0",
            "steps": [{
                "action": {
                    "name": "Test",
                    "type": "runCommand",
                    "input": {
                        "command": "/opt/test.sh"
                    }
                }
            }]
        }"#;

        let doc: JobDocument = serde_json::from_str(json).unwrap();
        assert_eq!(doc.version, "1.0");
        assert_eq!(doc.steps.len(), 1);
        assert_eq!(doc.steps[0].action.input.command, "/opt/test.sh");
    }
}

// ============================================================================
// Job Status & Formatting
// ============================================================================

/// Format job execution result into IoT Jobs statusDetails
/// AWS IoT Jobs requires all values in statusDetails to be strings, not nested objects
/// AWS IoT Jobs has a limit of 10 key-value pairs in statusDetails
pub fn format_status_details(
    result: &JobExecutionResult,
    include_stdout: bool,
) -> serde_json::Value {
    let mut details = serde_json::Map::new();

    // Summary fields (always included)
    details.insert(
        "steps_executed".to_string(),
        serde_json::Value::String(result.outputs.len().to_string()),
    );
    details.insert(
        "overall_success".to_string(),
        serde_json::Value::String(result.overall_success.to_string()),
    );

    if let Some(failed_step) = &result.failed_step {
        details.insert(
            "failed_step".to_string(),
            serde_json::Value::String(failed_step.clone()),
        );
    }

    // For multi-step jobs, create compact JSON strings to stay under 10 field limit
    if result.outputs.len() > 1 {
        // Compact format: JSON array of step summaries
        let step_summaries: Vec<serde_json::Value> = result
            .outputs
            .iter()
            .map(|step| {
                let mut summary = serde_json::Map::new();
                summary.insert(
                    "name".to_string(),
                    serde_json::Value::String(step.step_name.clone()),
                );
                summary.insert(
                    "exit_code".to_string(),
                    serde_json::Value::Number(step.output.exit_code.into()),
                );
                summary.insert(
                    "time_ms".to_string(),
                    serde_json::Value::Number(step.output.execution_time_ms.into()),
                );

                if include_stdout && !step.output.stdout.is_empty() {
                    summary.insert(
                        "stdout".to_string(),
                        serde_json::Value::String(step.output.stdout.clone()),
                    );
                }

                if !step.output.stderr.is_empty() {
                    summary.insert(
                        "stderr".to_string(),
                        serde_json::Value::String(step.output.stderr.clone()),
                    );
                }

                if step.ignored_failure {
                    summary.insert("ignored_failure".to_string(), serde_json::Value::Bool(true));
                }

                serde_json::Value::Object(summary)
            })
            .collect();

        details.insert(
            "steps".to_string(),
            serde_json::Value::String(serde_json::to_string(&step_summaries).unwrap_or_default()),
        );
    } else {
        // Single step: use individual fields for easier reading
        if let Some(step_output) = result.outputs.first() {
            details.insert(
                "step_name".to_string(),
                serde_json::Value::String(step_output.step_name.clone()),
            );
            details.insert(
                "exit_code".to_string(),
                serde_json::Value::String(step_output.output.exit_code.to_string()),
            );
            details.insert(
                "execution_time_ms".to_string(),
                serde_json::Value::String(step_output.output.execution_time_ms.to_string()),
            );

            if include_stdout && !step_output.output.stdout.is_empty() {
                details.insert(
                    "stdout".to_string(),
                    serde_json::Value::String(step_output.output.stdout.clone()),
                );
            }

            if !step_output.output.stderr.is_empty() {
                details.insert(
                    "stderr".to_string(),
                    serde_json::Value::String(step_output.output.stderr.clone()),
                );
            }

            if step_output.ignored_failure {
                details.insert(
                    "ignored_failure".to_string(),
                    serde_json::Value::String("true".to_string()),
                );
            }
        }
    }

    serde_json::Value::Object(details)
}

/// Job status for IoT Jobs updates
#[derive(Debug, Clone)]
pub struct JobStatus {
    status: JobStatusType,
    status_details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum JobStatusType {
    InProgress,
    Succeeded,
    Failed,
}

impl JobStatus {
    /// Create a succeeded status from execution result
    pub fn from_success(result: &JobExecutionResult, include_stdout: bool) -> Self {
        Self {
            status: JobStatusType::Succeeded,
            status_details: format_status_details(result, include_stdout),
        }
    }

    /// Create a failed status from execution result
    pub fn from_failure(result: &JobExecutionResult, include_stdout: bool) -> Self {
        Self {
            status: JobStatusType::Failed,
            status_details: format_status_details(result, include_stdout),
        }
    }

    /// Create a simple failed status for validation errors
    pub fn failed(reason: String, stdout: Option<String>, stderr: Option<String>) -> Self {
        let mut details = serde_json::json!({
            "reason": reason,
        });

        if let Some(stdout) = stdout {
            details["stdout"] = serde_json::Value::String(stdout);
        }

        if let Some(stderr) = stderr {
            details["stderr"] = serde_json::Value::String(stderr);
        }

        Self {
            status: JobStatusType::Failed,
            status_details: details,
        }
    }

    /// Convert to JSON for IoT Jobs API
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status,
            "statusDetails": self.status_details,
        })
    }
}
