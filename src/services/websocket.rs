use crate::models::{Config, WebsocketPrintJob};
use crate::services::print_job::handle_print_job;
use async_trait::async_trait;
use reqwest::Client;
use reverb_rs::private_channel;
use reverb_rs::{EventHandler, ReverbClient};
use serde_json;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio;

pub async fn websocket_task(config: Arc<Mutex<Config>>, http_client: Client) {
    // Flag to track connection status
    let is_connected = Arc::new(AtomicBool::new(false));

    // Store the client in an Option to keep it in scope
    let mut client_holder: Option<ReverbClient> = None;

    loop {
        // Only attempt to connect if we're not already connected
        if !is_connected.load(Ordering::Relaxed) {
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
                use_tls,
            );

            // Add a delay before trying to connect
            tokio::time::sleep(Duration::from_millis(500)).await;

            match reverb_client.connect().await {
                Ok(_) => {
                    println!("Connected to Reverb successfully");
                    // Store the client
                    client_holder = Some(reverb_client);

                    // Need a small delay to ensure socket_id is received and stored
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    // Only set connected state after ensuring the connection is stable
                    is_connected.store(true, Ordering::Relaxed);
                }
                Err(e) => {
                    eprintln!("Failed to connect to Reverb: {:?}", e);
                    is_connected.store(false, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
            }

            // Get the client from the holder
            if let Some(ref client) = client_holder {
                // Subscribe to the channel
                let channel_name = "FluxErp.Models.PrintJobs";
                let channel = private_channel(channel_name);

                // Add a delay to ensure the socket_id is set
                tokio::time::sleep(Duration::from_secs(1)).await;

                match client.subscribe(channel).await {
                    Ok(_) => println!("Subscribed to channel: private-{}", channel_name),
                    Err(e) => {
                        eprintln!("Failed to subscribe to channel: {:?}", e);
                        is_connected.store(false, Ordering::Relaxed);
                        client_holder = None;
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        continue;
                    }
                }

                // Create a handler to process events
                struct PrintJobHandler {
                    http_client: Client,
                    config: Arc<Mutex<Config>>,
                    is_connected: Arc<AtomicBool>,
                }

                #[async_trait]
                impl EventHandler for PrintJobHandler {
                    async fn on_connection_established(&self, socket_id: &str) {
                        println!("Connection established with socket id: {}", socket_id);
                        self.is_connected.store(true, Ordering::Relaxed);
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

                                        if let Err(e) =
                                            handle_print_job(print_job, &client_clone, &config_ref)
                                                .await
                                        {
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

                        // Mark as disconnected if we get certain errors
                        if code == 4000 || code == 4001 || code == 4009 || code >= 4200 {
                            eprintln!("Connection error detected");
                            self.is_connected.store(false, Ordering::Relaxed);
                        }
                    }
                }

                // Register the handler
                let is_connected_clone = is_connected.clone();
                let handler = PrintJobHandler {
                    http_client: http_client.clone(),
                    config: config.clone(),
                    is_connected: is_connected_clone,
                };

                client.add_event_handler(handler).await;
                println!("Event handler registered, listening for events");
            }
        }

        // Wait for a short time before checking connection status again
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
