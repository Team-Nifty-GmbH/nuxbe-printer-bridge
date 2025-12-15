use crate::models::Config;
use crate::services::print_job::fetch_and_print_job_by_id;
use async_trait::async_trait;
use reqwest::Client;
use reverb_rs::private_channel;
use reverb_rs::{EventHandler, ReverbClient};
use serde_json;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub async fn websocket_task(
    config: Arc<RwLock<Config>>,
    http_client: Client,
    cancel_token: CancellationToken,
) {
    let disabled = {
        let guard = config.read().expect("Failed to acquire config read lock");
        guard.reverb_disabled
    };

    if disabled {
        info!("WebSocket functionality is disabled. Not connecting to Reverb");
        return;
    }

    loop {
        if cancel_token.is_cancelled() {
            info!("WebSocket task shutting down");
            return;
        }

        let app_key;
        let app_secret;
        let auth_endpoint;
        let use_tls;
        let host;

        {
            let config_guard = config.read().expect("Failed to acquire config read lock");
            app_key = config_guard.reverb_app_key.clone();
            app_secret = config_guard.reverb_app_secret.clone();
            auth_endpoint = config_guard.reverb_auth_endpoint.clone();
            use_tls = config_guard.reverb_use_tls;
            host = config_guard.reverb_host.clone();
        }

        info!(app_key = %app_key, "Initializing Reverb client");

        // Create the client directly
        let reverb_client = ReverbClient::new(
            app_key.as_str(),
            app_secret.as_str(),
            auth_endpoint.as_str(),
            host.unwrap().as_str(),
            use_tls,
        );

        // Create a handler with cloned client for subscription
        struct PrintJobHandler {
            http_client: Client,
            config: Arc<RwLock<Config>>,
            client: Arc<ReverbClient>,
        }

        #[async_trait]
        impl EventHandler for PrintJobHandler {
            async fn on_connection_established(&self, socket_id: &str) {
                info!(socket_id, "Connection established");

                // Now that we have a socket_id, subscribe to the channel
                let channel_name = "print_job.";
                let channel = private_channel(channel_name);

                // Use the client directly - no mutex lock needed
                match self.client.subscribe(channel).await {
                    Ok(_) => info!(channel = %channel_name, "Subscribed to channel"),
                    Err(e) => {
                        error!(channel = %channel_name, error = ?e, "Failed to subscribe to channel");
                    }
                }
            }

            async fn on_channel_subscription_succeeded(&self, channel: &str) {
                info!(channel, "Successfully subscribed to channel");

                // Fetch any pending jobs that were created while offline
                info!("Fetching pending print jobs from API...");
                let client_clone = self.http_client.clone();
                let config_clone = self.config.clone();

                tokio::spawn(async move {
                    let config_copy = {
                        let guard = config_clone.read().expect("Failed to acquire config read lock");
                        guard.clone()
                    };

                    // Fetch pending jobs and collect their IDs
                    let job_ids: Vec<u32> = match crate::services::print_job::fetch_pending_job_ids(
                        &client_clone,
                        &config_copy,
                    )
                    .await
                    {
                        Ok(ids) => ids,
                        Err(e) => {
                            error!(error = %e, "Failed to fetch pending print jobs");
                            return;
                        }
                    };

                    if job_ids.is_empty() {
                        info!("No pending print jobs found");
                        return;
                    }

                    info!(
                        count = job_ids.len(),
                        "Found pending print jobs, processing..."
                    );
                    for job_id in job_ids {
                        info!(job_id, "Processing pending job");
                        if let Err(e) = crate::services::print_job::fetch_and_print_job_by_id(
                            job_id,
                            &client_clone,
                            &config_copy,
                        )
                        .await
                        {
                            error!(job_id, error = %e, "Failed to process pending job");
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
                if event == "PrintJobCreated" || event == ".PrintJobCreated" {
                    info!(channel, "Received print job event");

                    // Parse the job ID from the WebSocket message
                    // Format: {"model":{"id":20}}
                    #[derive(serde::Deserialize)]
                    struct WebsocketMessage {
                        model: WebsocketModel,
                    }
                    #[derive(serde::Deserialize)]
                    struct WebsocketModel {
                        id: u32,
                    }

                    match serde_json::from_str::<WebsocketMessage>(data) {
                        Ok(message) => {
                            let job_id = message.model.id;
                            info!(job_id, "Received print job creation event");

                            // Get references needed to handle the job
                            let client_clone = self.http_client.clone();
                            let config_clone = self.config.clone();

                            // Spawn a new task to fetch and print the job
                            tokio::spawn(async move {
                                let config_copy = {
                                    let guard = config_clone.read().expect("Failed to acquire config read lock");
                                    guard.clone()
                                };

                                if let Err(e) =
                                    fetch_and_print_job_by_id(job_id, &client_clone, &config_copy)
                                        .await
                                {
                                    error!(job_id, error = %e, "Error handling print job from WebSocket");
                                } else {
                                    info!(job_id, "Successfully handled print job from WebSocket");
                                }
                            });
                        }
                        Err(e) => {
                            error!(error = %e, raw_data = %data, "Failed to parse print job data");
                        }
                    }
                }
            }

            async fn on_error(&self, code: u32, message: &str) {
                error!(code, message, "Reverb error");
            }
        }

        // Wrap the client in an Arc for sharing
        let client_arc = Arc::new(reverb_client);

        // Register the handler
        let handler = PrintJobHandler {
            http_client: http_client.clone(),
            config: config.clone(),
            client: client_arc.clone(),
        };

        // Add the event handler and connect
        client_arc.add_event_handler(handler).await;

        // Connect to the server
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
                error!(error = ?e, "Failed to connect to Reverb");
            }
        }

        // Check for cancellation before reconnecting
        if cancel_token.is_cancelled() {
            info!("WebSocket task shutting down");
            return;
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

        // If we reach here, we'll try to reconnect
        info!("Reconnecting to Reverb server");
    }
}
