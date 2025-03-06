use std::sync::{Arc, Mutex};
use reqwest::Client;
use tokio;
use serde_json;

use crate::models::{Config, WebsocketPrintJob};
use crate::services::print_job::handle_print_job;

/// Background task to handle WebSocket connection for real-time print jobs
pub async fn websocket_task(config: Arc<Mutex<Config>>, http_client: Client) {
    loop {
        let app_key;
        let app_secret;
        let cluster;
        let use_tls;
        let config_clone;
        let host;

        {
            let config_guard = config.lock().unwrap();
            app_key = config_guard.reverb_app_key.clone();
            app_secret = config_guard.reverb_app_secret.clone();
            // Extract cluster from host or use default
            cluster = config_guard
                .reverb_host
                .clone()
                .unwrap_or_else(|| "mt1".to_string());
            use_tls = config_guard.reverb_use_tls;
            host = config_guard.reverb_host.clone();
            config_clone = config_guard.clone();
        }

        println!("Initializing Pusher client with app key: {}", app_key);

        // Create Pusher client configuration
        let pusher_config = pusher_rs::PusherConfig {
            app_key,
            app_secret,
            cluster,
            use_tls,
            host,
            max_reconnection_attempts: 5,
            ..Default::default()
        };

        // Create Pusher client
        let mut pusher = match pusher_rs::PusherClient::new(pusher_config) {
            Ok(client) => client,
            Err(e) => {
                eprintln!("Failed to create Pusher client: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        };

        match pusher.connect().await {
            Ok(_) => println!("Connected to Pusher successfully"),
            Err(e) => {
                eprintln!("Failed to connect to Pusher: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        }

        println!("Pusher client initialized, subscribing to print jobs channel");

        // Subscribe to the print jobs channel
        let channel_name = "private-FluxErp.Models.PrintJobs";

        // Subscribe to the channel (using the corrected method signature)
        match pusher.subscribe(channel_name).await {
            Ok(_) => println!("Successfully subscribed to channel: {}", channel_name),
            Err(e) => {
                eprintln!("Failed to subscribe to channel: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        }

        println!(
            "Subscribed to print jobs channel, setting up event handler for PrintJobCreated event"
        );

        // Set up an event handler using a callback function for the specific event
        let http_client_clone = http_client.clone();
        let config_for_handler = config_clone.clone();

        // The bind method requires a callback function instead of returning a stream
        let connection_result = pusher
            .bind("PrintJobCreated", move |event| {
                println!("Received print job event: {:?}", event);

                // The data field is of type Value (likely serde_json::Value)
                let data = event.data;

                // Convert the Value to a string for parsing
                match serde_json::to_string(&data) {
                    Ok(event_data) => {
                        println!("Event data as string: {}", event_data);

                        // Parse the print job data
                        match serde_json::from_str::<WebsocketPrintJob>(&event_data) {
                            Ok(print_job) => {
                                // Handle the print job
                                let client_clone = http_client_clone.clone();
                                let config_ref = config_for_handler.clone();

                                // Spawn a new task to handle the print job
                                tokio::spawn(async move {
                                    if let Err(e) =
                                        handle_print_job(print_job, &client_clone, &config_ref)
                                            .await
                                    {
                                        eprintln!("Error handling WebSocket print job: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("Failed to parse print job data: {}", e);
                                // Print the data to help with debugging
                                println!("Raw data: {}", event_data);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to convert event data to string: {:?}", e);
                    }
                }
            })
            .await;

        // Check if binding was successful
        if let Err(e) = connection_result {
            eprintln!("Failed to bind to event: {:?}", e);
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            continue;
        }

        println!("Bound to PrintJobCreated event, waiting for events");

        // Since pusher_rs handles events through callbacks, we need to keep the connection alive
        // The simplest way is to just wait indefinitely or until an error occurs
        match pusher.connect().await {
            Ok(_) => {
                println!("Connected to Pusher successfully");
                // Wait for disconnection
                tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;
            }
            Err(e) => {
                eprintln!("Failed to connect to Pusher: {:?}", e);
            }
        }

        // If we reach here, the connection was closed or failed, wait before reconnecting
        println!("Connection lost, reconnecting in 30 seconds...");
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    }
}