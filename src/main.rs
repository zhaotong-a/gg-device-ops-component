mod config;
mod error;
mod executor;
mod ipc;
mod models;
mod security;

use config::Config;
use error::Result;
use ipc::{IpcClient, JobHandler};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "device_ops_component=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    tracing::info!(version = %VERSION, "Device Operations Component starting");

    // Load configuration
    let config = Config::load(None)?;
    tracing::info!(
        security_enabled = config.security.enabled,
        default_timeout = config.execution.default_timeout,
        "Configuration loaded"
    );

    // Create IPC client
    let ipc_client = IpcClient::new().await?;
    tracing::info!(thing_name = %ipc_client.thing_name(), "Connected to Greengrass IPC");

    // Create and run job handler
    let mut job_handler = JobHandler::new(ipc_client, config);

    // Handle graceful shutdown
    tokio::select! {
        result = job_handler.run() => {
            if let Err(e) = result {
                tracing::error!(error = %e, "Job handler error");
                return Err(e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received shutdown signal");
        }
    }

    tracing::info!("Device Operations Component stopped");
    Ok(())
}
