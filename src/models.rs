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

impl ExecutionOutput {
    /// Create a synthetic error output for steps that failed before producing real output
    pub fn from_error(error: &impl std::fmt::Display) -> Self {
        Self {
            stdout: String::new(),
            stderr: error.to_string(),
            exit_code: -1,
            execution_time_ms: 0,
            stderr_line_count: 1,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }
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

impl StepOutput {
    fn to_summary_json(&self, include_stdout: bool) -> serde_json::Value {
        let mut m = serde_json::json!({
            "name": self.step_name,
            "exit_code": self.output.exit_code,
            "time_ms": self.output.execution_time_ms,
        });
        if include_stdout && !self.output.stdout.is_empty() {
            m["stdout"] = serde_json::Value::String(self.output.stdout.clone());
        }
        if !self.output.stderr.is_empty() {
            m["stderr"] = serde_json::Value::String(self.output.stderr.clone());
        }
        if self.ignored_failure {
            m["ignored_failure"] = serde_json::Value::Bool(true);
        }
        m
    }
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
/// AWS IoT Jobs statusDetails constraints:
/// - Max 10 key-value pairs
/// - Max 1,024 characters per value
/// - Max 128 characters per key
/// - All values must be strings
/// Truncate a string to fit within the AWS IoT Jobs 1,024 char value limit.
fn truncate_to_limit(s: &str) -> String {
    if s.len() <= 1024 {
        s.to_string()
    } else {
        let mut truncated = s[..1020].to_string();
        truncated.push_str("...");
        truncated
    }
}

pub fn format_status_details(
    result: &JobExecutionResult,
    include_stdout: bool,
) -> serde_json::Value {
    let mut details = serde_json::Map::new();

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

    if result.outputs.len() > 1 {
        let summaries: Vec<serde_json::Value> = result
            .outputs
            .iter()
            .map(|s| s.to_summary_json(include_stdout))
            .collect();
        let mut steps_json = serde_json::to_string(&summaries).unwrap_or_default();
        if steps_json.len() > 1024 {
            // Re-serialize without stdout to fit within limit
            let summaries_no_stdout: Vec<serde_json::Value> = result
                .outputs
                .iter()
                .map(|s| s.to_summary_json(false))
                .collect();
            steps_json = serde_json::to_string(&summaries_no_stdout).unwrap_or_default();
            if steps_json.len() > 1024 {
                steps_json.truncate(1020);
                steps_json.push_str("...]");
            }
        }
        details.insert(
            "steps".to_string(),
            serde_json::Value::String(steps_json),
        );
    } else if let Some(step) = result.outputs.first() {
        details.insert("step_name".into(), step.step_name.clone().into());
        details.insert("exit_code".into(), step.output.exit_code.to_string().into());
        details.insert("execution_time_ms".into(), step.output.execution_time_ms.to_string().into());
        if include_stdout && !step.output.stdout.is_empty() {
            details.insert("stdout".into(), truncate_to_limit(&step.output.stdout).into());
        }
        if !step.output.stderr.is_empty() {
            details.insert("stderr".into(), truncate_to_limit(&step.output.stderr).into());
        }
        if step.ignored_failure {
            details.insert("ignored_failure".into(), "true".to_string().into());
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
    /// Create an IN_PROGRESS status to start the IoT Jobs timeout timer.
    pub fn in_progress() -> Self {
        Self {
            status: JobStatusType::InProgress,
            status_details: serde_json::json!({}),
        }
    }

    /// Create status from execution result
    pub fn from_result(result: &JobExecutionResult, include_stdout: bool) -> Self {
        let status = if result.overall_success {
            JobStatusType::Succeeded
        } else {
            JobStatusType::Failed
        };
        Self {
            status,
            status_details: format_status_details(result, include_stdout),
        }
    }

    /// Create a simple failed status for validation/parse errors
    pub fn failed(reason: String) -> Self {
        Self {
            status: JobStatusType::Failed,
            status_details: serde_json::json!({
                "reason": reason,
            }),
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
