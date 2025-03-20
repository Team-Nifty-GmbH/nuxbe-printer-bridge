use crate::models::{Config, WebsocketPrintJob};
use crate::services::print_job::handle_print_job;
use async_trait::async_trait;
use reqwest::Client;
use reverb_rs::private_channel;
use reverb_rs::{EventHandler, ReverbClient};
use serde_json;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio;

pub async fn websocket_task(config: Arc<Mutex<Config>>, http_client: Client) {
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

        println!("Initializing Reverb client with app key: {}", app_key);

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
                println!("Connection established with socket id: {}", socket_id);

                // Now that we have a socket_id, subscribe to the channel
                let channel_name = "FluxErp.Models.PrintJobs";
                let channel = private_channel(channel_name);

                // Use the client directly - no mutex lock needed
                match self.client.subscribe(channel).await {
                    Ok(_) => println!("Subscribed to channel: private-{}", channel_name),
                    Err(e) => {
                        eprintln!("Failed to subscribe to channel: {:?}", e);
                    }
                }
            }

            async fn on_channel_subscription_succeeded(&self, channel: &str) {
                println!("Successfully subscribed to channel: {}", channel);
            }

            async fn on_channel_event(&self, channel: &str, event: &str, data: &str) {
                println!("Received event: {} on channel: {} with data: {}", event, channel, data);

                if event == "PrintJobCreated" {
                    println!("Received print job event on channel {}: {}", channel, data);

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

                                if let Err(e) = handle_print_job(print_job, &client_clone, &mut config_copy).await {
                                    eprintln!("Error handling print job: {}", e);
                                }

                                // Update the shared config with any token changes
                                if let Ok(mut guard) = config_clone.lock() {
                                    guard.flux_api_token = config_copy.flux_api_token;
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Failed to parse print job data: {}", e);
                            println!("Raw data: {}", data);
                        }
                    }
                }
            }
            
            async fn on_error(&self, code: u32, message: &str) {
                eprintln!("Reverb error: {} (code: {})", message, code);
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
                println!("Connected to Reverb successfully");
                // Wait for a long time to keep the connection alive
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            Err(e) => {
                eprintln!("Failed to connect to Reverb: {:?}", e);
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }

        // If we reach here, we'll try to reconnect
        println!("Reconnecting to Reverb server...");
    }
}