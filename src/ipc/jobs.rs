use crate::config::Config;
use crate::error::Result;
use crate::executor::CommandExecutor;
use crate::ipc::IpcClient;
use crate::models::{Job, JobOrError, JobStatus};
use crate::security::{validate_job_document, SecurityValidator};

pub struct JobHandler {
    ipc_client: IpcClient,
    executor: CommandExecutor,
}

impl JobHandler {
    pub fn new(ipc_client: IpcClient, config: Config) -> Self {
        let security = if config.security.allowlist.is_empty() {
            None
        } else {
            Some(SecurityValidator::new(config.security.clone()))
        };

        let executor = CommandExecutor::new(config.execution, security);

        Self {
            ipc_client,
            executor,
        }
    }

    /// Main event loop.
    ///
    /// Two-phase startup eliminates duplicate job delivery:
    /// 1. Query `$next/get` to pick up any job queued while offline
    /// 2. Subscribe to `notify-next` for steady-state job delivery
    ///
    /// By completing the query phase before subscribing to notify-next,
    /// there is no overlap window where both mechanisms deliver the same job.
    pub async fn run(&mut self) -> Result<()> {
        tracing::info!("Job handler starting");

        // Phase 1: Pick up any pending job from while we were offline
        tracing::info!("Querying for pending jobs");
        match self.ipc_client.query_pending_job().await {
            Ok(Some(job_or_error)) => {
                self.process_job_or_error(job_or_error).await;
            }
            Ok(None) => {
                tracing::info!("No pending jobs found");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to query pending jobs on startup");
            }
        }

        // Phase 2: Subscribe to notify-next for new jobs going forward
        let mut job_stream = self.ipc_client.subscribe_to_notify_next().await?;

        tracing::info!("Listening for job notifications");

        while let Some(job_or_error) = job_stream.recv().await {
            self.process_job_or_error(job_or_error).await;
        }

        tracing::warn!("Job channel closed, exiting job handler");
        Ok(())
    }

    async fn process_job_or_error(&mut self, job_or_error: JobOrError) {
        match job_or_error {
            JobOrError::Valid(job) => {
                if let Err(e) = self.handle_job(job).await {
                    tracing::error!(error = %e, "Failed to handle job");
                }
            }
            JobOrError::ParseError { job_id, error } => {
                if let Err(e) = self.handle_parse_error(&job_id, &error).await {
                    tracing::error!(error = %e, "Failed to handle parse error");
                }
            }
        }
    }

    async fn handle_parse_error(&self, job_id: &str, error: &str) -> Result<()> {
        tracing::error!(job_id = %job_id, error = %error, "Marking malformed job as FAILED");

        let status = JobStatus::failed(
            format!("Job document parsing failed: {}", error),
        );

        self.ipc_client.update_job_status(job_id, status).await
    }

    async fn handle_job(&mut self, job: Job) -> Result<()> {
        tracing::info!(job_id = %job.job_id, "Processing job");

        if let Err(e) = validate_job_document(&job.document) {
            tracing::error!(job_id = %job.job_id, error = %e, "Invalid job document");
            let status = JobStatus::failed(e.to_string());
            return self.ipc_client.update_job_status(&job.job_id, status).await;
        }

        // Send IN_PROGRESS to start the IoT Jobs inProgressTimeoutInMinutes timer.
        // This is critical — without it, a crashed device leaves the job stuck in QUEUED forever.
        if let Err(e) = self.ipc_client.update_job_status(&job.job_id, JobStatus::in_progress()).await {
            tracing::error!(job_id = %job.job_id, error = %e, "Failed to send IN_PROGRESS");
            // Continue anyway — execution is more important than the status update
        }

        let result = self.executor.execute(&job.document).await;
        let include_stdout = job.document.include_std_out.unwrap_or(false);

        let status = match result {
            Ok(execution_result) => {
                if execution_result.overall_success {
                    tracing::info!(
                        job_id = %job.job_id,
                        steps_executed = execution_result.outputs.len(),
                        "Job succeeded"
                    );
                } else {
                    tracing::error!(
                        job_id = %job.job_id,
                        failed_step = ?execution_result.failed_step,
                        "Job failed"
                    );
                }
                JobStatus::from_result(&execution_result, include_stdout)
            }
            Err(e) => {
                tracing::error!(job_id = %job.job_id, error = %e, "Job execution error");
                JobStatus::failed(e.to_string())
            }
        };

        self.ipc_client.update_job_status(&job.job_id, status).await
    }
}
