use actix_files as fs_web;
use actix_multipart::Multipart;
use actix_web::{get, post, web, App, Error, HttpResponse, HttpServer, Responder};
use futures::{StreamExt, TryStreamExt};
use local_ip_address::local_ip;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time;

// Configuration structure
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Config {
    instance_name: String,        // Name for this printer server instance
    printer_check_interval: u64,  // in minutes
    job_check_interval: u64,      // in minutes
    host_url: String,             // Base URL for all API endpoints
    notification_token: String,   // Authentication token for notifications
    print_jobs_token: String,     // Authentication token for print jobs
    admin_port: u16,              // Admin interface port
    api_port: u16,                // API port
    websocket_url: String,        // WebSocket URL for real-time job notifications
    websocket_auth_token: String, // Authentication token for WebSocket
    reverb_app_id: String,
    reverb_app_key: String,
    reverb_app_secret: String,
    reverb_use_tls: bool,
    reverb_host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            instance_name: "default-instance".to_string(),
            printer_check_interval: 5,
            job_check_interval: 2,
            host_url: "http://example.com".to_string(),
            notification_token: "default-token".to_string(),
            print_jobs_token: "default-token".to_string(),
            admin_port: 8081,
            api_port: 8080,
            websocket_url: "ws://example.com/socket".to_string(),
            websocket_auth_token: "default-websocket-token".to_string(),
            reverb_app_id: "default-app-id".to_string(),
            reverb_app_key: "default-app-key".to_string(),
            reverb_app_secret: "default-app-secret".to_string(),
            reverb_use_tls: true,
            reverb_host: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PrintRequest {
    printer: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Printer {
    name: String,
    description: String,
    location: String,
    make_and_model: String,
    media_sizes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct PrinterList {
    printers: Vec<Printer>,
}

#[derive(Serialize, Deserialize)]
struct PrinterNotification {
    action: String,
    printer: Printer,
}

#[derive(Serialize, Deserialize, Debug)]
struct PrintJob {
    id: String,
    printer: String,
    file_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct WebsocketPrintJob {
    printer_name: String,
    printer_server: String,
    media_id: String,
}

#[derive(Serialize, Deserialize)]
struct ConfigUpdateRequest {
    config: Config,
}

// Function to load the configuration from file
fn load_config() -> Config {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    let config_dir = home_dir.join(".config/flux-spooler");
    let config_path = config_dir.join("config.json");

    // Create the directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    match fs::read_to_string(&config_path) {
        Ok(contents) => {
            serde_json::from_str(&contents).unwrap_or_else(|e| {
                eprintln!(
                    "Error parsing config file: {}. Using default configuration.",
                    e
                );
                let default_config = Config::default();
                // Save the default config
                save_config(&default_config);
                default_config
            })
        }
        Err(_) => {
            println!("Config file not found. Creating with default values.");
            let default_config = Config::default();
            save_config(&default_config);
            default_config
        }
    }
}

// Function to save the configuration to file
fn save_config(config: &Config) {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    let config_dir = home_dir.join(".config/flux-spooler");
    let config_path = config_dir.join("config.json");

    // Create the directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(e) = fs::write(&config_path, json) {
                eprintln!("Failed to save config file: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to serialize config: {}", e),
    }
}

// Function to get all available printers
async fn get_all_printers() -> Vec<Printer> {
    // Debug lpstat
    let debug_output = Command::new("lpstat")
        .arg("-a")
        .output()
        .expect("Failed to execute lpstat -a command");

    println!(
        "Debug lpstat -a: {}",
        String::from_utf8_lossy(&debug_output.stdout)
    );

    let lpstat_output = Command::new("lpstat")
        .arg("-a")
        .output()
        .expect("Failed to execute lpstat command");

    let printer_list_str = String::from_utf8_lossy(&lpstat_output.stdout);
    let printer_names: Vec<String> = printer_list_str
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                Some(parts[0].to_string())
            } else {
                None
            }
        })
        .collect();

    let mut final_printer_names = printer_names;
    if final_printer_names.is_empty() {
        let alt_output = Command::new("lpstat")
            .arg("-p")
            .output()
            .expect("Failed to execute lpstat -p command");

        println!(
            "Debug lpstat -p: {}",
            String::from_utf8_lossy(&alt_output.stdout)
        );

        let alt_list_str = String::from_utf8_lossy(&alt_output.stdout);
        final_printer_names = alt_list_str
            .lines()
            .filter_map(|line| {
                if line.starts_with("printer ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        return Some(parts[1].to_string());
                    }
                }
                None
            })
            .collect();
    }

    if final_printer_names.is_empty() {
        let v_output = Command::new("lpstat")
            .arg("-v")
            .output()
            .expect("Failed to execute lpstat -v command");

        println!(
            "Debug lpstat -v: {}",
            String::from_utf8_lossy(&v_output.stdout)
        );

        let v_list_str = String::from_utf8_lossy(&v_output.stdout);
        final_printer_names = v_list_str
            .lines()
            .filter_map(|line| {
                if line.starts_with("device for ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        return Some(parts[2].trim_end_matches(':').to_string());
                    }
                }
                None
            })
            .collect();
    }

    if final_printer_names.is_empty() {
        let shell_output = Command::new("sh")
            .arg("-c")
            .arg("lpstat -a | cut -d' ' -f1")
            .output()
            .expect("Failed to execute shell command");

        println!(
            "Debug shell command: {}",
            String::from_utf8_lossy(&shell_output.stdout)
        );

        let shell_list_str = String::from_utf8_lossy(&shell_output.stdout);
        final_printer_names = shell_list_str
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();
    }

    println!("Detected printers: {:?}", final_printer_names);

    let mut printers = Vec::new();

    for name in final_printer_names {
        // Try to get media sizes
        let lpoptions_output = Command::new("lpoptions")
            .arg("-p")
            .arg(&name)
            .arg("-l")
            .output();

        let mut media_sizes = Vec::new();

        if let Ok(output) = lpoptions_output {
            let printer_options = String::from_utf8_lossy(&output.stdout);

            for line in printer_options.lines() {
                if line.starts_with("PageSize/")
                    || line.starts_with("MediaSize/")
                    || line.contains("media size")
                {
                    if let Some(options_part) = line.split(':').nth(1) {
                        let sizes: Vec<String> = options_part
                            .split_whitespace()
                            .filter_map(|opt| {
                                if opt.starts_with('*') {
                                    Some(opt.trim_start_matches('*').to_string())
                                } else {
                                    Some(opt.to_string())
                                }
                            })
                            .collect();
                        media_sizes.extend(sizes);
                    }
                }
            }
        }

        let mut description = String::new();
        let mut location = String::new();
        let mut make_and_model = String::new();

        let lpstat_p_output = Command::new("lpstat")
            .arg("-l")
            .arg("-p")
            .arg(&name)
            .output();

        if let Ok(output) = lpstat_p_output {
            let printer_info = String::from_utf8_lossy(&output.stdout);

            for line in printer_info.lines() {
                if line.contains("Description:") {
                    description = line
                        .split("Description:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                } else if line.contains("Location:") {
                    location = line
                        .split("Location:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                } else if line.contains("Make and Model:") {
                    make_and_model = line
                        .split("Make and Model:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
            }
        }

        if description.is_empty() && make_and_model.is_empty() {
            let lpinfo_output = Command::new("lpinfo").arg("-m").output();

            if let Ok(output) = lpinfo_output {
                let lpinfo_str = String::from_utf8_lossy(&output.stdout);

                for line in lpinfo_str.lines() {
                    if line.contains(&name) {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() > 1 {
                            make_and_model = parts[1..].join(" ");
                            break;
                        }
                    }
                }
            }
        }

        printers.push(Printer {
            name,
            description,
            location,
            make_and_model,
            media_sizes,
        });
    }

    printers
}

#[get("/printers")]
async fn get_printers() -> impl Responder {
    let printers = get_all_printers().await;
    HttpResponse::Ok().json(PrinterList { printers })
}

#[post("/print")]
async fn print_file(
    mut payload: Multipart,
    query: web::Query<PrintRequest>,
) -> Result<HttpResponse, Error> {
    let printer_name = &query.printer;

    let lpstat_output = Command::new("lpstat")
        .arg("-p")
        .arg(printer_name)
        .output()
        .expect("Failed to execute lpstat command");

    if !lpstat_output.status.success() {
        return Ok(HttpResponse::BadRequest().body(format!("Printer '{}' not found", printer_name)));
    }

    // Process uploaded file
    while let Ok(Some(mut field)) = payload.try_next().await {
        // Get the content disposition directly
        let content_disposition = field.content_disposition();

        // Check if a filename exists
        if let Some(filename) = content_disposition.get_filename() {
            let _filename_str = filename.to_string();

            // Create a temporary file to store the uploaded content
            let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

            // Write file contents to temp file
            while let Some(chunk) = field.next().await {
                let data = chunk?;
                temp_file.write_all(&data)?;
            }

            // Get path to temp file
            let temp_path = temp_file.path().to_str().unwrap();

            // Print the file using lp command
            let output = Command::new("lp")
                .arg("-d")
                .arg(printer_name)
                .arg(temp_path)
                .output()
                .expect("Failed to execute lp command");

            if output.status.success() {
                let success_msg = String::from_utf8_lossy(&output.stdout);
                return Ok(HttpResponse::Ok().body(format!("Print job submitted: {}", success_msg)));
            } else {
                let error_msg = String::from_utf8_lossy(&output.stderr);
                return Ok(HttpResponse::InternalServerError()
                    .body(format!("Print failed: {}", error_msg)));
            }
        }
    }

    Ok(HttpResponse::BadRequest().body("No file provided"))
}

// API endpoint to get current configuration
#[get("/config")]
async fn get_config(config: web::Data<Arc<Mutex<Config>>>) -> impl Responder {
    let config = config.lock().unwrap().clone();
    HttpResponse::Ok().json(config)
}

// API endpoint to update configuration
#[post("/config")]
async fn update_config(
    config_data: web::Data<Arc<Mutex<Config>>>,
    new_config: web::Json<ConfigUpdateRequest>,
) -> impl Responder {
    let mut config = config_data.lock().unwrap();
    *config = new_config.config.clone();
    save_config(&config);
    HttpResponse::Ok().json(config.clone())
}

// API endpoint to manually check for print jobs
#[get("/check_jobs")]
async fn check_jobs_endpoint(
    config: web::Data<Arc<Mutex<Config>>>,
    http_client: web::Data<Client>,
) -> impl Responder {
    let config_guard = config.lock().unwrap();
    match fetch_print_jobs(&http_client, &config_guard).await {
        Ok(jobs) => HttpResponse::Ok().json(jobs),
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to check jobs: {}", e)),
    }
}

// API endpoint to manually check for new printers
#[get("/check_printers")]
async fn check_printers_endpoint(
    printers_data: web::Data<Arc<Mutex<HashSet<String>>>>,
    config: web::Data<Arc<Mutex<Config>>>,
    http_client: web::Data<Client>,
) -> impl Responder {
    match check_for_new_printers(printers_data, http_client, config).await {
        Ok(new_printers) => HttpResponse::Ok().json(new_printers),
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Failed to check printers: {}", e))
        }
    }
}

// Function to check for new printers and notify
async fn check_for_new_printers(
    printers_data: web::Data<Arc<Mutex<HashSet<String>>>>,
    http_client: web::Data<Client>,
    config: web::Data<Arc<Mutex<Config>>>,
) -> Result<Vec<Printer>, Box<dyn std::error::Error>> {
    let current_printers = get_all_printers().await;
    let mut new_printers = Vec::new();

    {
        let mut printers_set = printers_data.lock().unwrap();
        for printer in &current_printers {
            if !printers_set.contains(&printer.name) {
                printers_set.insert(printer.name.clone());
                new_printers.push(printer.clone());
            }
        }
    }

    // Notify about new printers
    if !new_printers.is_empty() {
        // Get what we need from config before to await
        let host_url;
        let notification_token;
        let instance_name;

        {
            let config_guard = config.lock().unwrap();
            host_url = config_guard.host_url.clone();
            notification_token = config_guard.notification_token.clone();
            instance_name = config_guard.instance_name.clone();
        }

        // Construct the notification URL using the host
        let notification_url = format!("{}/api/printer-notification", host_url);

        for printer in &new_printers {
            let notification = PrinterNotification {
                action: "new_printer".to_string(),
                printer: printer.clone(),
            };

            let res = http_client
                .post(&notification_url)
                .header("Authorization", format!("Bearer {}", notification_token))
                .header("X-Instance-Name", instance_name.clone())
                .json(&notification)
                .send()
                .await?;

            if !res.status().is_success() {
                println!("Failed to notify about new printer: {}", res.status());
            }
        }
    }

    Ok(new_printers)
}

// Function to handle a print job received via WebSocket or API
async fn handle_print_job(
    print_job: WebsocketPrintJob,
    http_client: &Client,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Processing WebSocket print job for printer: {} from server: {} with media id: {}",
        print_job.printer_name, print_job.printer_server, print_job.media_id
    );

    // Check if this job is for this instance
    if print_job.printer_server != config.instance_name {
        println!(
            "Ignoring job for different printer server: {} (we are: {})",
            print_job.printer_server, config.instance_name
        );
        return Ok(());
    }

    // Construct the URL to get the file
    let file_url = format!(
        "{}/api/media/{}/download",
        config.host_url, print_job.media_id
    );

    // Download the file
    let file_response = http_client
        .get(&file_url)
        .header(
            "Authorization",
            format!("Bearer {}", config.print_jobs_token),
        )
        .send()
        .await?;

    if !file_response.status().is_success() {
        return Err(format!(
            "Failed to download file for media ID {}: {}",
            print_job.media_id,
            file_response.status()
        )
        .into());
    }

    let file_content = file_response.bytes().await?;

    // Create a temporary file
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&file_content)?;
    let temp_path = temp_file.path().to_str().unwrap();

    // Print the file
    let output = Command::new("lp")
        .arg("-d")
        .arg(&print_job.printer_name)
        .arg(temp_path)
        .output()?;

    if output.status.success() {
        println!(
            "Successfully printed media ID {} on printer {}: {}",
            print_job.media_id,
            print_job.printer_name,
            String::from_utf8_lossy(&output.stdout)
        );
    } else {
        eprintln!(
            "Failed to print media ID {} on printer {}: {}",
            print_job.media_id,
            print_job.printer_name,
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(format!(
            "Failed to print: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

// Background task to periodically check for new printers
async fn printer_checker_task(
    printers_data: Arc<Mutex<HashSet<String>>>,
    config: Arc<Mutex<Config>>,
    http_client: Client,
) {
    let printers_data = web::Data::new(printers_data);
    let config_data = web::Data::new(config);
    let client_data = web::Data::new(http_client);

    loop {
        let interval = { config_data.lock().unwrap().printer_check_interval };

        match check_for_new_printers(
            printers_data.clone(),
            client_data.clone(),
            config_data.clone(),
        )
        .await
        {
            Ok(new_printers) => {
                if !new_printers.is_empty() {
                    println!("Found {} new printer(s)", new_printers.len());
                    for printer in new_printers {
                        println!("  - {}", printer.name);
                    }
                }
            }
            Err(e) => eprintln!("Error checking for new printers: {}", e),
        }

        time::sleep(Duration::from_secs(interval * 60)).await;
    }
}

// Function to fetch and process print jobs from API
async fn fetch_print_jobs(
    http_client: &Client,
    config: &Config,
) -> Result<Vec<PrintJob>, Box<dyn std::error::Error>> {
    // Construct the URL for fetching print jobs
    let jobs_url = format!("{}/api/print-jobs", config.host_url);

    let response = http_client
        .get(&jobs_url)
        .header(
            "Authorization",
            format!("Bearer {}", config.print_jobs_token),
        )
        .header("X-Instance-Name", &config.instance_name)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch print jobs: {}", response.status()).into());
    }

    let jobs: Vec<PrintJob> = response.json().await?;

    // Process each job
    for job in &jobs {
        println!(
            "Processing print job {} for printer {}",
            job.id, job.printer
        );

        // Download the file
        let file_response = http_client.get(&job.file_url).send().await?;
        if !file_response.status().is_success() {
            eprintln!(
                "Failed to download file for job {}: {}",
                job.id,
                file_response.status()
            );
            continue;
        }

        let file_content = file_response.bytes().await?;

        // Create a temporary file
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(&file_content)?;
        let temp_path = temp_file.path().to_str().unwrap();

        // Print the file
        let output = Command::new("lp")
            .arg("-d")
            .arg(&job.printer)
            .arg(temp_path)
            .output()?;

        if output.status.success() {
            println!(
                "Successfully printed job {}: {}",
                job.id,
                String::from_utf8_lossy(&output.stdout)
            );
        } else {
            eprintln!(
                "Failed to print job {}: {}",
                job.id,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(jobs)
}

// Background task to periodically check for print jobs from API
async fn job_checker_task(config: Arc<Mutex<Config>>, http_client: Client) {
    loop {
        // Get interval and clone config outside of the await
        let interval;
        let config_clone;

        {
            let config_guard = config.lock().unwrap();
            interval = config_guard.job_check_interval;
            config_clone = config_guard.clone();
        }

        match fetch_print_jobs(&http_client, &config_clone).await {
            Ok(jobs) => {
                if !jobs.is_empty() {
                    println!("Processed {} print job(s)", jobs.len());
                }
            }
            Err(e) => eprintln!("Error fetching print jobs: {}", e),
        }

        time::sleep(Duration::from_secs(interval * 60)).await;
    }
}

// Function to handle WebSocket connection for real-time print jobs
async fn websocket_task(config: Arc<Mutex<Config>>, http_client: Client) {
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
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load configuration
    let config = Arc::new(Mutex::new(load_config()));

    // Clone config for use in servers
    let config_clone = config.clone();

    // Initialize HTTP client
    let http_client = Client::new();

    // Initialize printer set
    let printers_set = Arc::new(Mutex::new(HashSet::new()));

    // Initial population of printer set
    {
        let printers = get_all_printers().await;
        let mut set = printers_set.lock().unwrap();
        for printer in printers {
            set.insert(printer.name);
        }
    }

    // Spawn printer checker task
    let printers_set_clone = printers_set.clone();
    let config_checker = config.clone();
    let http_client_checker = http_client.clone();

    tokio::spawn(async move {
        printer_checker_task(printers_set_clone, config_checker, http_client_checker).await;
    });

    // Spawn job checker task
    let config_jobs = config.clone();
    let http_client_jobs = http_client.clone();

    tokio::spawn(async move {
        job_checker_task(config_jobs, http_client_jobs).await;
    });

    // Spawn WebSocket listener task
    let config_ws = config.clone();
    let http_client_ws = http_client.clone();

    tokio::spawn(async move {
        websocket_task(config_ws, http_client_ws).await;
    });

    // Start API server
    let api_port = config.lock().unwrap().api_port;
    let admin_port = config.lock().unwrap().admin_port;

    match local_ip() {
        Ok(ip) => println!("Local IP address: {}", ip),
        Err(e) => eprintln!("Failed to get local IP: {}", e),
    }

    println!(
        "Starting CUPS print server API on http://0.0.0.0:{}",
        api_port
    );
    println!("Starting Admin interface on http://0.0.0.0:{}", admin_port);

    // API Server
    let api_server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&config)))
            .app_data(web::Data::new(Arc::clone(&printers_set)))
            .app_data(web::Data::new(http_client.clone()))
            .service(get_printers)
            .service(print_file)
            .service(check_jobs_endpoint)
            .service(check_printers_endpoint)
    })
    .bind(format!("0.0.0.0:{}", api_port))?;

    // Admin Server with static files and config API
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    let admin_dir = home_dir.join(".config/flux-spooler/admin");

    let admin_server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&config_clone)))
            .service(get_config)
            .service(update_config)
            .service(fs_web::Files::new("/", admin_dir.to_str().unwrap()).index_file("index.html"))
    })
    .bind(format!("0.0.0.0:{}", admin_port))?;

    // Create admin folder and HTML
    create_admin_interface()?;

    // Run both servers
    futures::future::try_join(api_server.run(), admin_server.run()).await?;

    Ok(())
}

// Create the admin interface files
fn create_admin_interface() -> std::io::Result<()> {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    let admin_dir = home_dir.join(".config/flux-spooler/admin");

    // Create admin directory if it doesn't exist
    fs::create_dir_all(&admin_dir)?;

    // Create index.html
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>FLUX <-> CUPS Print Server - Admin Panel</title>
    <style>
        body {
            font-family: Arial, sans-serif;
            line-height: 1.6;
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
        }
        h1 {
            color: #333;
            border-bottom: 1px solid #ddd;
            padding-bottom: 10px;
        }
        label {
            display: block;
            margin-top: 15px;
            font-weight: bold;
        }
        input, button {
            padding: 8px;
            margin-top: 5px;
        }
        input[type="text"], input[type="number"] {
            width: 100%;
            box-sizing: border-box;
        }
        button {
            background-color: #4CAF50;
            color: white;
            border: none;
            cursor: pointer;
            margin-top: 20px;
        }
        button:hover {
            background-color: #45a049;
        }
        .card {
            border: 1px solid #ddd;
            padding: 20px;
            margin-top: 20px;
            border-radius: 5px;
            background-color: #f9f9f9;
        }
        .actions {
            margin-top: 30px;
            display: flex;
            gap: 10px;
        }
        .actions button {
            flex: 1;
        }
        #status {
            margin-top: 20px;
            padding: 10px;
            border-radius: 5px;
        }
        .success {
            background-color: #d4edda;
            color: #155724;
        }
        .error {
            background-color: #f8d7da;
            color: #721c24;
        }
    </style>
</head>
<body>
    <h1>FLUX <-> CUPS Print Server - Admin Panel</h1>

    <div class="card">
        <h2>Configuration</h2>
        <form id="configForm">
            <label for="instanceName">Instance Name:</label>
            <input type="text" id="instanceName" name="instance_name">

            <label for="hostUrl">Host URL:</label>
            <input type="text" id="hostUrl" name="host_url">

            <label for="apiPort">API Port:</label>
            <input type="number" id="apiPort" name="api_port" min="1" max="65535">

            <label for="adminPort">Admin Port:</label>
            <input type="number" id="adminPort" name="admin_port" min="1" max="65535">

            <label for="printerCheckInterval">Printer Check Interval (minutes):</label>
            <input type="number" id="printerCheckInterval" name="printer_check_interval" min="1">

            <label for="jobCheckInterval">Job Check Interval (minutes):</label>
            <input type="number" id="jobCheckInterval" name="job_check_interval" min="1">

            <label for="notificationToken">Printer Broadcast Bearer Token:</label>
            <input type="text" id="notificationToken" name="notification_token">

            <label for="printJobsToken">Print Jobs Bearer Token:</label>
            <input type="text" id="printJobsToken" name="print_jobs_token">

            <label for="websocketUrl">WebSocket URL:</label>
            <input type="text" id="websocketUrl" name="websocket_url">

            <label for="websocketAuthToken">WebSocket Auth Token:</label>
            <input type="text" id="websocketAuthToken" name="websocket_auth_token">

            <button type="submit">Save Configuration</button>
        </form>
    </div>

    <div class="actions">
        <button id="checkPrinters">Check for New Printers</button>
        <button id="checkJobs">Check for Print Jobs</button>
        <button id="reconnectWebsocket">Reconnect WebSocket</button>
    </div>

    <div id="status" style="display: none;"></div>

    <script>
        // Load the configuration on page load
        document.addEventListener('DOMContentLoaded', function() {
            loadConfig();

            // Form submission
            document.getElementById('configForm').addEventListener('submit', function(e) {
                e.preventDefault();
                saveConfig();
            });

            // Action buttons
            document.getElementById('checkPrinters').addEventListener('click', checkPrinters);
            document.getElementById('checkJobs').addEventListener('click', checkJobs);
            document.getElementById('reconnectWebsocket').addEventListener('click', reconnectWebsocket);
        });

        // Load configuration from the server
        function loadConfig() {
            fetch('/config')
                .then(response => response.json())
                .then(config => {
                    document.getElementById('instanceName').value = config.instance_name;
                    document.getElementById('hostUrl').value = config.host_url;
                    document.getElementById('apiPort').value = config.api_port;
                    document.getElementById('adminPort').value = config.admin_port;
                    document.getElementById('printerCheckInterval').value = config.printer_check_interval;
                    document.getElementById('jobCheckInterval').value = config.job_check_interval;
                    document.getElementById('notificationToken').value = config.notification_token;
                    document.getElementById('printJobsToken').value = config.print_jobs_token;
                    document.getElementById('websocketUrl').value = config.websocket_url;
                    document.getElementById('websocketAuthToken').value = config.websocket_auth_token;
                })
                .catch(error => {
                    showStatus('Failed to load configuration: ' + error, 'error');
                });
        }

        // Save configuration to the server
        function saveConfig() {
            const config = {
                instance_name: document.getElementById('instanceName').value,
                host_url: document.getElementById('hostUrl').value,
                api_port: parseInt(document.getElementById('apiPort').value),
                admin_port: parseInt(document.getElementById('adminPort').value),
                printer_check_interval: parseInt(document.getElementById('printerCheckInterval').value),
                job_check_interval: parseInt(document.getElementById('jobCheckInterval').value),
                notification_token: document.getElementById('notificationToken').value,
                print_jobs_token: document.getElementById('printJobsToken').value,
                websocket_url: document.getElementById('websocketUrl').value,
                websocket_auth_token: document.getElementById('websocketAuthToken').value
            };

            fetch('/config', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ config })
            })
            .then(response => {
                if (!response.ok) {
                    throw new Error('Failed to save configuration');
                }
                return response.json();
            })
            .then(data => {
                showStatus('Configuration saved successfully!', 'success');

                // Alert user about port changes if they occurred
                if (data.api_port !== parseInt(window.location.port) &&
                    data.admin_port !== parseInt(window.location.port)) {
                    alert('Port settings have been changed. You will need to restart the server for these changes to take effect.');
                }
            })
            .catch(error => {
                showStatus('Error: ' + error.message, 'error');
            });
        }

       // Check for new printers
        function checkPrinters() {
            showStatus('Checking for new printers...', '');

            const apiPort = document.getElementById('apiPort').value;
            const url = `http://${window.location.hostname}:${apiPort}/check_printers`;

            fetch(url)
                .then(response => response.json())
                .then(data => {
                    if (data.length === 0) {
                        showStatus('No new printers found.', 'success');
                    } else {
                        showStatus(`Found ${data.length} new printer(s)!`, 'success');
                    }
                })
                .catch(error => {
                    showStatus('Error checking printers: ' + error, 'error');
                });
        }

        // Check for print jobs
        function checkJobs() {
            showStatus('Checking for print jobs...', '');

            const apiPort = document.getElementById('apiPort').value;
            const url = `http://${window.location.hostname}:${apiPort}/check_jobs`;

            fetch(url)
                .then(response => response.json())
                .then(data => {
                    if (data.length === 0) {
                        showStatus('No print jobs found.', 'success');
                    } else {
                        showStatus(`Processed ${data.length} print job(s)!`, 'success');
                    }
                })
                .catch(error => {
                    showStatus('Error checking jobs: ' + error, 'error');
                });
        }

        // Trigger WebSocket reconnection
        function reconnectWebsocket() {
            showStatus('Requesting WebSocket reconnection...', '');

            const apiPort = document.getElementById('apiPort').value;
            const url = `http://${window.location.hostname}:${apiPort}/reconnect_websocket`;

            fetch(url)
                .then(response => {
                    if (!response.ok) {
                        throw new Error(`HTTP error! status: ${response.status}`);
                    }
                    return response.text();
                })
                .then(() => {
                    showStatus('WebSocket reconnection initiated.', 'success');
                })
                .catch(error => {
                    showStatus('Error reconnecting WebSocket: ' + error, 'error');
                });
        }

        // Show status message
        function showStatus(message, type) {
            const statusDiv = document.getElementById('status');
            statusDiv.textContent = message;
            statusDiv.style.display = 'block';

            // Remove existing classes
            statusDiv.classList.remove('success', 'error');

            // Add class based on type
            if (type === 'success') {
                statusDiv.classList.add('success');
            } else if (type === 'error') {
                statusDiv.classList.add('error');
            }
        }
    </script>
</body>
</html>"#;

    fs::write(admin_dir.join("index.html"), html)?;

    Ok(())
}
