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
        let sdk = Sdk::init();

        sdk.connect()
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to connect to IPC: {:?}", e)))?;

        let thing_name = std::env::var("AWS_IOT_THING_NAME")
            .or_else(|_| Self::get_thing_name_from_config())
            .map_err(|_| {
                DeviceOpsError::IpcError(
                    "AWS_IOT_THING_NAME not set and could not be determined from config. \
                     Cannot construct MQTT topics without thing name."
                        .to_string(),
                )
            })?;

        tracing::info!(thing_name = %thing_name, "Connected to Greengrass IPC");

        Ok(Self { sdk, thing_name })
    }

    fn get_thing_name_from_config() -> std::result::Result<String, String> {
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

    /// Phase 1: Query for any pending job queued while offline.
    ///
    /// Subscribes to `$next/get/accepted`, publishes `$next/get`, and waits
    /// for a response. Returns the pending job if one exists, or None.
    /// This must be called BEFORE `subscribe_to_notify_next` to avoid duplicates.
    pub async fn query_pending_job(&mut self) -> Result<Option<JobOrError>> {
        let qos = Qos::AtLeastOnce;
        let (tx, mut rx) = mpsc::channel(1);

        // Subscribe to $next/get/accepted to receive the query response
        let next_topic = format!("$aws/things/{}/jobs/$next/get/accepted", self.thing_name);
        tracing::info!(topic = %next_topic, "Subscribing to job query responses");

        let callback = Box::leak(Box::new(move |_topic: &str, payload: &[u8]| {
            if let Some(job_or_error) = Self::parse_job_notification(payload) {
                let _ = tx.try_send(job_or_error);
            }
        }));

        let sub = self.sdk.subscribe_to_iot_core(&next_topic, qos, callback)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to subscribe to $next/get/accepted: {:?}", e)))?;
        std::mem::forget(sub);

        // Publish $next/get to ask IoT Jobs for the next pending job
        let get_topic = format!("$aws/things/{}/jobs/$next/get", self.thing_name);
        tracing::debug!(topic = %get_topic, "Requesting next pending job");
        self.sdk
            .publish_to_iot_core(&get_topic, b"{}", qos)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to request next job: {:?}", e)))?;

        // Wait briefly for a response — if no pending job, IoT Jobs responds
        // with an empty execution (parsed as None by parse_job_notification)
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
            Ok(Some(job_or_error)) => Ok(Some(job_or_error)),
            Ok(None) => Ok(None),
            Err(_) => {
                tracing::debug!("No pending job found (query timed out)");
                Ok(None)
            }
        }
    }

    /// Phase 2: Subscribe to notify-next for steady-state job delivery.
    ///
    /// Returns a channel that receives jobs as they arrive. This should be
    /// called AFTER `query_pending_job` to avoid processing the same job twice.
    pub async fn subscribe_to_notify_next(&mut self) -> Result<mpsc::Receiver<JobOrError>> {
        let qos = Qos::AtLeastOnce;
        let (tx, rx) = mpsc::channel(100);

        let callback = Box::leak(Box::new(move |_topic: &str, payload: &[u8]| {
            if let Some(job_or_error) = Self::parse_job_notification(payload) {
                if let Err(e) = tx.try_send(job_or_error) {
                    tracing::error!(error = %e, "Failed to send job to channel (channel full or closed)");
                }
            }
        }));

        let notify_topic = format!("$aws/things/{}/jobs/notify-next", self.thing_name);
        tracing::info!(topic = %notify_topic, "Subscribing to IoT Jobs notifications");
        let sub = self.sdk.subscribe_to_iot_core(&notify_topic, qos, callback)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to subscribe to notify-next: {:?}", e)))?;
        std::mem::forget(sub);

        // Debug subscriptions for operational visibility
        self.subscribe_to_update_responses(qos)?;

        Ok(rx)
    }

    /// Subscribe to job update accepted/rejected topics for debug logging
    fn subscribe_to_update_responses(&self, qos: Qos) -> Result<()> {
        let accepted_topic = format!("$aws/things/{}/jobs/+/update/accepted", self.thing_name);
        let rejected_topic = format!("$aws/things/{}/jobs/+/update/rejected", self.thing_name);

        let debug_callback = Box::leak(Box::new(move |topic: &str, payload: &[u8]| {
            let payload_str = String::from_utf8_lossy(payload);
            if topic.contains("/update/accepted") {
                tracing::info!(topic = %topic, payload = %payload_str, "AWS ACCEPTED job status update");
            } else if topic.contains("/update/rejected") {
                tracing::error!(topic = %topic, payload = %payload_str, "AWS REJECTED job status update");
            }
        }));

        let sub1 = self.sdk.subscribe_to_iot_core(&accepted_topic, qos, debug_callback)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to subscribe to update/accepted: {:?}", e)))?;
        let sub2 = self.sdk.subscribe_to_iot_core(&rejected_topic, qos, debug_callback)
            .map_err(|e| DeviceOpsError::IpcError(format!("Failed to subscribe to update/rejected: {:?}", e)))?;
        std::mem::forget(sub1);
        std::mem::forget(sub2);

        Ok(())
    }

    pub async fn update_job_status(&self, job_id: &str, status: JobStatus) -> Result<()> {
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
}
