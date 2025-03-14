use reverb_rs::private_channel;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use reqwest::Client;
use tokio;
use serde_json;
use reverb_rs::{ReverbClient, EventHandler};
use async_trait::async_trait;
use cursive::reexports::enumset::__internal::EnumSetTypeRepr;
use crate::models::{Config, WebsocketPrintJob};
use crate::services::print_job::handle_print_job;

/// Background task to handle WebSocket connection for real-time print jobs
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

        let reverb_client = ReverbClient::new(
            app_key.as_str(),
            app_secret.as_str(),
            auth_endpoint.as_str(),
            host.unwrap().as_str(),
            use_tls
        );

        match reverb_client.connect().await {
            Ok(_) => println!("Connected to Reverb successfully"),
            Err(e) => {
                eprintln!("Failed to connect to Reverb: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        }

        // Subscribe to the channel
        let channel_name = "FluxErp.Models.PrintJobs";
        let channel = private_channel(channel_name);

        match reverb_client.subscribe(channel).await {
            Ok(_) => println!("Subscribed to channel: private-{}", channel_name),
            Err(e) => {
                eprintln!("Failed to subscribe to channel: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        }

        // Create a handler to process events
        struct PrintJobHandler {
            http_client: Client,
            config: Arc<Mutex<Config>>,
        }

        #[async_trait]
        impl EventHandler for PrintJobHandler {
            async fn on_connection_established(&self, socket_id: &str) {
                println!("Connection established with socket id: {}", socket_id);
            }

            async fn on_channel_subscription_succeeded(&self, channel: &str) {
                println!("Successfully subscribed to channel: {}", channel);
            }

            async fn on_channel_event(&self, channel: &str, event: &str, data: &str) {
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
                                // Get a direct reference to Config
                                let config_ref = {
                                    let guard = config_clone.lock().unwrap();
                                    guard.clone()
                                };

                                if let Err(e) = handle_print_job(print_job, &client_clone, &config_ref).await {
                                    eprintln!("Error handling print job: {}", e);
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

        // Register the handler
        let handler = PrintJobHandler {
            http_client: http_client.clone(),
            config: config.clone(),
        };

        reverb_client.add_event_handler(handler).await;
        println!("Event handler registered, listening for events");

        // Keep waiting for a long time - the handler will process events as they arrive
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;

        // If we reach here, we'll try to reconnect
        println!("Reconnecting to Reverb server...");
    }
}