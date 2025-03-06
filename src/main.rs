use std::collections::HashSet;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use actix_web::{web, App, HttpServer};
use clap::{Parser, Subcommand};
use local_ip_address::local_ip;
use reqwest::Client;

mod api;
mod config;
mod models;
mod services;
mod utils;

use api::routes::{check_jobs_endpoint, check_printers_endpoint, get_printers, print_file};
use config::load_config;
use services::print_job::job_checker_task;
use services::printer::{get_all_printers, printer_checker_task};
use services::websocket::websocket_task;
use utils::tui::run_tui;

/// Command line arguments for the application
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the server normally
    Run,

    /// Configure application settings using a text-based UI
    Config,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Handle subcommands
    match cli.command {
        Some(Commands::Config) => {
            // Run the TUI configuration editor
            run_tui();
            return Ok(());
        }
        _ => {
            // Default: run the server
            run_server().await
        }
    }
}

/// Run the main server application
async fn run_server() -> std::io::Result<()> {
    // Load configuration
    let config = Arc::new(Mutex::new(load_config()));

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

    let local_address = local_ip().unwrap_or_else(|_| IpAddr::from_str("127.0.0.1").unwrap());

    println!(
        "Starting CUPS print server API on http://{}:{}",
        local_address, api_port
    );

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

    api_server.run().await?;

    Ok(())
}
