use crate::error::{DeviceOpsError, Result};
use crate::models::{Job, JobNotification, JobOrError, JobStatus};
use gg_sdk::{Qos, Sdk};
use tokio::sync::mpsc;

/// Greengrass IPC client using the official AWS SDK
#[derive(Debug)]
pub struct IpcClient {
    sdk: Sdk,
    thing_name: String,
}

impl IpcClient {
    pub async fn new() -> Result<Self> {
        // Initialize the Greengrass SDK
        let sdk = Sdk::init();

        // Connect to Greengrass IPC
        sdk.connect()
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to connect to IPC: {:?}", e)))?;

        // Get thing name from environment or configuration
        let thing_name = std::env::var("AWS_IOT_THING_NAME")
            .or_else(|_| Self::get_thing_name_from_config())
            .unwrap_or_else(|_| {
                tracing::warn!("Could not determine thing name, using default");
                "unknown-thing".to_string()
            });

        tracing::info!(thing_name = %thing_name, "Connected to Greengrass IPC");

        Ok(Self { sdk, thing_name })
    }

    fn get_thing_name_from_config() -> std::result::Result<String, String> {
        // Try to get thing name from Greengrass configuration
        // This would use GetConfiguration IPC call in production
        Err("Not implemented".to_string())
    }

    pub fn thing_name(&self) -> &str {
        &self.thing_name
    }

    /// Parse job notification and extract job or error
    fn parse_job_notification(payload: &[u8]) -> Option<JobOrError> {
        match serde_json::from_slice::<JobNotification>(payload) {
            Ok(notification) => {
                if let Some(job) = Option::<Job>::from(notification) {
                    tracing::debug!(job_id = %job.job_id, "Received job notification");
                    Some(JobOrError::Valid(job))
                } else {
                    tracing::debug!("Received notification without execution details");
                    None
                }
            }
            Err(e) => {
                let payload_str = String::from_utf8_lossy(payload);
                let error_msg = e.to_string();
                tracing::error!(
                    error = %error_msg,
                    payload = %payload_str,
                    "Failed to parse job notification - job document format is invalid"
                );

                // Try to extract job ID and send parse error
                if let Ok(raw_json) = serde_json::from_slice::<serde_json::Value>(payload) {
                    if let Some(execution) = raw_json.get("execution") {
                        if let Some(job_id) = execution.get("jobId").and_then(|id| id.as_str()) {
                            tracing::warn!(job_id = %job_id, "Sending parse error for malformed job");
                            return Some(JobOrError::ParseError {
                                job_id: job_id.to_string(),
                                error: error_msg,
                            });
                        }
                    }
                }
                None
            }
        }
    }

    pub async fn subscribe_to_jobs(
        &mut self,
    ) -> Result<(mpsc::Receiver<JobOrError>, mpsc::Receiver<()>)> {
        // Subscribe to IoT Jobs notification topic
        let notify_topic = format!("$aws/things/{}/jobs/notify-next", self.thing_name);
        let qos = Qos::AtLeastOnce;

        tracing::info!(topic = %notify_topic, "Subscribing to IoT Jobs notifications");

        let (job_tx, job_rx) = mpsc::channel(100);
        let (reconnect_tx, reconnect_rx) = mpsc::channel(100);

        // Create callback for job notifications
        // Note: Box::leak is intentional - callbacks must live for program lifetime
        let job_callback = Box::leak(Box::new(move |_topic: &str, payload: &[u8]| {
            if let Some(job_or_error) = Self::parse_job_notification(payload) {
                if let Err(e) = job_tx.blocking_send(job_or_error) {
                    tracing::error!(error = %e, "Failed to send job to channel");
                }
            }
        }));

        // Subscribe to notify-next topic
        let subscription = self
            .sdk
            .subscribe_to_iot_core(&notify_topic, qos, job_callback)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to subscribe: {:?}", e)))?;

        // Keep subscription alive by leaking it (intentional for program lifetime)
        std::mem::forget(subscription);

        // Subscribe to $next/get/accepted for job request responses
        let next_topic = format!("$aws/things/{}/jobs/$next/get/accepted", self.thing_name);
        tracing::info!(topic = %next_topic, "Subscribing to job request responses");

        let next_subscription = self
            .sdk
            .subscribe_to_iot_core(&next_topic, qos, job_callback)
            .map_err(|e| {
                DeviceOpsError::IpcError(format!(
                    "Failed to subscribe to $next/get/accepted: {:?}",
                    e
                ))
            })?;

        std::mem::forget(next_subscription);

        // Subscribe to reconnection signal topic (zdb11 pattern)
        let reconnect_topic = format!("reconnect/{}", self.thing_name);
        tracing::info!(topic = %reconnect_topic, "Subscribing to reconnection signals");

        // Note: Box::leak is intentional - callbacks must live for program lifetime
        let reconnect_callback = Box::leak(Box::new(move |topic: &str, payload: &[u8]| {
            tracing::info!(
                topic = %topic,
                payload = ?String::from_utf8_lossy(payload),
                "Reconnection detected - will query pending jobs"
            );
            if let Err(e) = reconnect_tx.blocking_send(()) {
                tracing::error!(error = %e, "Failed to send reconnection signal");
            }
        }));

        let reconnect_subscription = self
            .sdk
            .subscribe_to_iot_core(&reconnect_topic, qos, reconnect_callback)
            .map_err(|e| {
                DeviceOpsError::IpcError(format!("Failed to subscribe to reconnect topic: {:?}", e))
            })?;

        std::mem::forget(reconnect_subscription);

        // Subscribe to update response topics to see AWS's actual response
        let update_accepted_topic =
            format!("$aws/things/{}/jobs/+/update/accepted", self.thing_name);
        let update_rejected_topic =
            format!("$aws/things/{}/jobs/+/update/rejected", self.thing_name);

        tracing::info!(topic = %update_accepted_topic, "Subscribing to update accepted responses");
        tracing::info!(topic = %update_rejected_topic, "Subscribing to update rejected responses");

        // Create debug callback for update responses
        // Note: Box::leak is intentional - callbacks must live for program lifetime
        let debug_callback = Box::leak(Box::new(move |topic: &str, payload: &[u8]| {
            let payload_str = String::from_utf8_lossy(payload);
            if topic.contains("/update/accepted") {
                tracing::info!(
                    topic = %topic,
                    payload = %payload_str,
                    "AWS ACCEPTED job status update"
                );
            } else if topic.contains("/update/rejected") {
                tracing::error!(
                    topic = %topic,
                    payload = %payload_str,
                    "AWS REJECTED job status update"
                );
            }
        }));

        let update_accepted_sub = self
            .sdk
            .subscribe_to_iot_core(&update_accepted_topic, qos, debug_callback)
            .map_err(|e| {
                DeviceOpsError::IpcError(format!("Failed to subscribe to update/accepted: {:?}", e))
            })?;

        let update_rejected_sub = self
            .sdk
            .subscribe_to_iot_core(&update_rejected_topic, qos, debug_callback)
            .map_err(|e| {
                DeviceOpsError::IpcError(format!("Failed to subscribe to update/rejected: {:?}", e))
            })?;

        std::mem::forget(update_accepted_sub);
        std::mem::forget(update_rejected_sub);

        Ok((job_rx, reconnect_rx))
    }

    pub async fn update_job_status(&self, job_id: &str, status: JobStatus) -> Result<()> {
        // Publish job status update to IoT Core
        let topic = format!("$aws/things/{}/jobs/{}/update", self.thing_name, job_id);
        let qos = Qos::AtLeastOnce;

        let status_json = status.to_json();
        let payload = serde_json::to_vec(&status_json)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to serialize status: {}", e)))?;

        tracing::info!(
            job_id = %job_id,
            topic = %topic,
            payload = ?String::from_utf8_lossy(&payload),
            "Updating job status"
        );

        self.sdk
            .publish_to_iot_core(&topic, &payload, qos)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to publish: {:?}", e)))?;

        Ok(())
    }

    pub async fn request_next_job(&self) -> Result<()> {
        // Publish to $next/get to request pending jobs
        let topic = format!("$aws/things/{}/jobs/$next/get", self.thing_name);
        let qos = Qos::AtLeastOnce;
        let payload = b"{}"; // Empty JSON object

        tracing::debug!(topic = %topic, "Requesting next pending job");

        self.sdk
            .publish_to_iot_core(&topic, payload, qos)
            .map_err(|e| {
                DeviceOpsError::IpcError(format!("Failed to request next job: {:?}", e))
            })?;

        Ok(())
    }
}

// Note: Tests removed as they require a real Greengrass environment
// Integration tests should be run on actual devices
