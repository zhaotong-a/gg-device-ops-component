use crate::config::Config;
use crate::error::Result;
use crate::executor::CommandExecutor;
use crate::ipc::IpcClient;
use crate::models::{Job, JobOrError, JobStatus};
use crate::security::{validate_job_document, SecurityValidator};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct JobHandler {
    ipc_client: IpcClient,
    executor: CommandExecutor,
    processed_jobs: Arc<Mutex<VecDeque<String>>>,
}

impl JobHandler {
    pub fn new(ipc_client: IpcClient, config: Config) -> Self {
        let security = if config.security.enabled {
            Some(SecurityValidator::new(config.security.clone()))
        } else {
            None
        };

        let executor = CommandExecutor::new(config.execution, security);

        Self {
            ipc_client,
            executor,
            processed_jobs: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
        }
    }

    /// Check if job was already processed and mark it as processed if not.
    /// Returns true if this is a new job that should be handled.
    fn mark_job_processed(&self, job_id: &str) -> bool {
        let mut processed = self.processed_jobs.lock().unwrap();

        // Check if already processed
        if processed.contains(&job_id.to_string()) {
            return false;
        }

        // Mark as processed
        processed.push_back(job_id.to_string());

        // Keep only the last 100 job IDs (FIFO eviction)
        if processed.len() > 100 {
            processed.pop_front();
        }

        true
    }

    pub async fn run(&mut self) -> Result<()> {
        tracing::info!("Job handler starting");

        // Request any pending jobs on startup
        if let Err(e) = self.ipc_client.request_next_job().await {
            tracing::warn!(error = %e, "Failed to request pending jobs on startup, will retry on next event");
        }

        // Subscribe to job notifications and reconnection signals
        let (mut job_stream, mut reconnect_stream) = self.ipc_client.subscribe_to_jobs().await?;

        tracing::info!("Listening for job notifications and reconnection signals");

        // Process jobs and reconnection signals as they arrive
        loop {
            tokio::select! {
                Some(job_or_error) = job_stream.recv() => {
                    match job_or_error {
                        JobOrError::Valid(job) => {
                            if let Err(e) = self.handle_job(job).await {
                                tracing::error!(error = %e, "Failed to handle job");
                            }
                        }
                        JobOrError::ParseError { job_id, error } => {
                            if self.mark_job_processed(&job_id) {
                                if let Err(e) = self.handle_parse_error(&job_id, &error).await {
                                    tracing::error!(error = %e, "Failed to handle parse error");
                                }
                            } else {
                                tracing::debug!(job_id = %job_id, "Parse error already processed, skipping duplicate");
                            }
                        }
                    }
                }
                Some(()) = reconnect_stream.recv() => {
                    tracing::info!("Handling reconnection event - querying pending jobs");
                    if let Err(e) = self.ipc_client.request_next_job().await {
                        tracing::error!(error = %e, "Failed to query jobs after reconnection");
                    }
                }
                else => {
                    tracing::warn!("All channels closed, exiting job handler");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_parse_error(&self, job_id: &str, error: &str) -> Result<()> {
        tracing::error!(job_id = %job_id, error = %error, "Marking malformed job as FAILED");

        let status = JobStatus::failed(
            format!("Job document parsing failed: {}", error),
            None,
            None,
        );

        self.ipc_client.update_job_status(job_id, status).await?;

        // Request next job
        self.ipc_client.request_next_job().await?;

        Ok(())
    }

    async fn handle_job(&self, job: Job) -> Result<()> {
        // Check if we've already processed this job
        if !self.mark_job_processed(&job.job_id) {
            tracing::debug!(job_id = %job.job_id, "Job already processed, skipping duplicate");
            return Ok(());
        }

        tracing::info!(job_id = %job.job_id, "Received job");

        // Validate job document
        if let Err(e) = validate_job_document(&job.document) {
            tracing::error!(job_id = %job.job_id, error = %e, "Invalid job document");
            let status = JobStatus::failed(e.to_string(), None, None);
            self.ipc_client
                .update_job_status(&job.job_id, status)
                .await?;
            self.ipc_client.request_next_job().await?;
            return Ok(());
        }

        // Execute all steps in the job document
        // AWS rejects IN_PROGRESS with empty statusDetails, so we skip it
        let result = self.executor.execute(&job.document).await;

        // Determine whether to include stdout based on job document
        let include_stdout = job.document.include_std_out.unwrap_or(false);

        // Update final status using new JobExecutionResult
        let status = match result {
            Ok(execution_result) => {
                if execution_result.overall_success {
                    tracing::info!(
                        job_id = %job.job_id,
                        steps_executed = execution_result.outputs.len(),
                        "Job succeeded"
                    );
                    JobStatus::from_success(&execution_result, include_stdout)
                } else {
                    tracing::error!(
                        job_id = %job.job_id,
                        failed_step = ?execution_result.failed_step,
                        "Job failed"
                    );
                    JobStatus::from_failure(&execution_result, include_stdout)
                }
            }
            Err(e) => {
                tracing::error!(job_id = %job.job_id, error = %e, "Job execution error");
                JobStatus::failed(e.to_string(), None, None)
            }
        };

        self.ipc_client
            .update_job_status(&job.job_id, status)
            .await?;

        // Request next job
        self.ipc_client.request_next_job().await?;

        Ok(())
    }
}

// Note: Tests removed as they require a real Greengrass environment
// Integration tests should be run on actual devices with Greengrass installed
