use crate::models::{Config, WebsocketPrintJob};
use crate::services::print_job::handle_print_job;
use async_trait::async_trait;
use reqwest::Client;
use reverb_rs::private_channel;
use reverb_rs::{EventHandler, ReverbClient};
use serde_json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio;
use tracing::{debug, error, info};

pub async fn websocket_task(config: Arc<Mutex<Config>>, http_client: Client) {
    let disabled = {
        let guard = config.lock().unwrap();
        guard.reverb_disabled
    };

    if disabled {
        info!("WebSocket functionality is disabled. Not connecting to Reverb");
        return;
    }

    loop {
        let app_key;
        let app_secret;
        let auth_endpoint;
        let use_tls;
        let host;

        {
            let config_guard = config.lock().unwrap();
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
            config: Arc<Mutex<Config>>,
            client: Arc<ReverbClient>,
        }

        #[async_trait]
        impl EventHandler for PrintJobHandler {
            async fn on_connection_established(&self, socket_id: &str) {
                info!(socket_id, "Connection established");

                // Now that we have a socket_id, subscribe to the channel
                let channel_name = "FluxErp.Models.PrintJobs";
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
            }

            async fn on_channel_event(&self, channel: &str, event: &str, data: &str) {
                debug!(
                    event,
                    channel,
                    data_len = data.len(),
                    "Received channel event"
                );

                if event == "PrintJobCreated" {
                    info!(channel, "Received print job event");

                    // Parse the print job data
                    match serde_json::from_str::<WebsocketPrintJob>(data) {
                        Ok(print_job) => {
                            // Get references needed to handle the job
                            let client_clone = self.http_client.clone();
                            let config_clone = self.config.clone();

                            // Spawn a new task to handle the print job
                            tokio::spawn(async move {
                                // Get the original config by cloning
                                let mut config_copy = {
                                    let guard = config_clone.lock().unwrap();
                                    guard.clone()
                                };

                                if let Err(e) =
                                    handle_print_job(print_job, &client_clone, &mut config_copy)
                                        .await
                                {
                                    error!(error = %e, "Error handling print job");
                                } else {
                                    info!("Successfully handled print job from WebSocket");
                                }

                                // Update the shared config with any token changes
                                if let Ok(mut guard) = config_clone.lock() {
                                    guard.flux_api_token = config_copy.flux_api_token;
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
                // Wait for a long time to keep the connection alive
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            Err(e) => {
                error!(error = ?e, "Failed to connect to Reverb");
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }

        // If we reach here, we'll try to reconnect
        info!("Reconnecting to Reverb server");
    }
}
