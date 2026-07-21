use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use reverb_rs::private_channel;
use reverb_rs::{EventHandler, ReverbClient};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::models::Config;
use crate::services::print_job::{
    JobContext, fetch_and_print_job_by_id, fetch_incomplete_jobs, process_print_job,
};
use crate::utils::config::read_config;

/// Payload of a `PrintJobCreated` event: `{"model":{"id":20}}`
#[derive(serde::Deserialize)]
struct WebsocketMessage {
    model: WebsocketModel,
}

#[derive(serde::Deserialize)]
struct WebsocketModel {
    id: u32,
}

struct PrintJobHandler {
    config: Arc<RwLock<Config>>,
    client: Arc<ReverbClient>,
    ctx: JobContext,
}

#[async_trait]
impl EventHandler for PrintJobHandler {
    async fn on_connection_established(&self, socket_id: &str) {
        info!(socket_id, "Connection established");

        // Now that we have a socket_id, subscribe to the channel
        let channel_name = "print_job.";
        let channel = private_channel(channel_name);

        match self.client.subscribe(channel).await {
            Ok(_) => info!(channel = %channel_name, "Subscribed to channel"),
            Err(e) => {
                error!(channel = %channel_name, error = %e, "Failed to subscribe to channel");
            }
        }
    }

    async fn on_channel_subscription_succeeded(&self, channel: &str) {
        info!(channel, "Successfully subscribed to channel");

        // Fetch any pending jobs that were created while offline
        info!("Fetching pending print jobs from API...");
        let config = read_config(&self.config);
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            let jobs = match fetch_incomplete_jobs(&ctx.http_client, &config).await {
                Ok(jobs) => jobs,
                Err(e) => {
                    error!(error = %e, "Failed to fetch pending print jobs");
                    return;
                }
            };

            let pending: Vec<_> = jobs.into_iter().filter(|j| !j.is_in_flight()).collect();
            if pending.is_empty() {
                info!("No pending print jobs found");
                return;
            }

            info!(
                count = pending.len(),
                "Found pending print jobs, processing..."
            );
            for job in pending {
                info!(job_id = job.id, "Processing pending job");
                if let Err(e) = process_print_job(&job, &config, &ctx).await {
                    error!(job_id = job.id, error = %e, "Failed to process pending job");
                }
            }
        });
    }

    async fn on_channel_event(&self, channel: &str, event: &str, data: &str) {
        info!(
            event,
            channel,
            data_len = data.len(),
            "Received channel event"
        );

        // Check for both formats: "PrintJobCreated" and ".PrintJobCreated"
        if event != "PrintJobCreated" && event != ".PrintJobCreated" {
            return;
        }

        match serde_json::from_str::<WebsocketMessage>(data) {
            Ok(message) => {
                let job_id = message.model.id;
                info!(job_id, "Received print job creation event");

                let config = read_config(&self.config);
                let ctx = self.ctx.clone();

                // Spawn a new task to fetch and print the job
                tokio::spawn(async move {
                    match fetch_and_print_job_by_id(job_id, &config, &ctx).await {
                        Ok(_) => info!(job_id, "Successfully handled print job from WebSocket"),
                        Err(e) => {
                            error!(job_id, error = %e, "Error handling print job from WebSocket");
                        }
                    }
                });
            }
            Err(e) => {
                error!(error = %e, raw_data = %data, "Failed to parse print job data");
            }
        }
    }

    async fn on_error(&self, code: u32, message: &str) {
        error!(code, message, "Reverb error");
    }
}

pub async fn websocket_task(
    config: Arc<RwLock<Config>>,
    ctx: JobContext,
    cancel_token: CancellationToken,
) {
    if read_config(&config).reverb_disabled {
        info!("WebSocket functionality is disabled. Not connecting to Reverb");
        return;
    }

    loop {
        if cancel_token.is_cancelled() {
            info!("WebSocket task shutting down");
            return;
        }

        let config_snapshot = read_config(&config);

        let Some(host) = config_snapshot.reverb_host else {
            error!(
                "Reverb is enabled but no Reverb host is configured. \
                 Set a host via 'nuxbe-printer-bridge config' or disable Reverb."
            );
            return;
        };

        info!(app_key = %config_snapshot.reverb_app_key, "Initializing Reverb client");

        let reverb_client = ReverbClient::new(
            config_snapshot.reverb_app_key.as_str(),
            config_snapshot.reverb_app_secret.as_str(),
            config_snapshot.reverb_auth_endpoint.as_str(),
            host.as_str(),
            config_snapshot.reverb_use_tls,
        );

        let client_arc = Arc::new(reverb_client);

        let handler = PrintJobHandler {
            config: config.clone(),
            client: client_arc.clone(),
            ctx: ctx.clone(),
        };

        client_arc.add_event_handler(handler).await;

        match client_arc.connect().await {
            Ok(_) => {
                info!("Connected to Reverb successfully");
                // Wait until the connection is closed or cancellation
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!("WebSocket task received shutdown signal");
                        return;
                    }
                    _ = client_arc.wait_for_disconnect() => {
                        info!("WebSocket connection lost");
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to connect to Reverb");
            }
        }

        // Wait before reconnecting
        info!("Waiting 5 seconds before reconnecting...");
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("WebSocket task shutting down");
                return;
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }

        info!("Reconnecting to Reverb server");
    }
}
