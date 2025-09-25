use std::collections::HashSet;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use actix_web::{App, HttpServer, web};
use clap::{ArgAction, Parser, Subcommand};
use local_ip_address::local_ip;
use reqwest::Client;
use tracing_subscriber::EnvFilter;

mod api;
mod models;
mod services;
mod tests;
mod utils;

use api::routes::{check_jobs_endpoint, check_printers_endpoint, get_printers, print_file};
use utils::config::load_config;
use services::print_job::job_checker_task;
use services::printer::{get_all_printers, printer_checker_task};
use services::websocket::websocket_task;
use utils::printer_storage::{load_printers, save_printers_if_changed};
use utils::tui::run_tui;

/// Command line arguments for the application
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable verbose debug logging
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    verbose: u8,
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
    let cli = Cli::parse();
    let env_filter = match cli.verbose {
        0 => EnvFilter::from_default_env()
            .add_directive("reverb_rs=warn".parse().unwrap())
            .add_directive("rust_spooler=info".parse().unwrap()),
        1 => EnvFilter::from_default_env()
            .add_directive("reverb_rs=info".parse().unwrap())
            .add_directive("rust_spooler=info".parse().unwrap()),
        2 => EnvFilter::from_default_env()
            .add_directive("reverb_rs=debug".parse().unwrap())
            .add_directive("rust_spooler=debug".parse().unwrap()),
        _ => EnvFilter::from_default_env()
            .add_directive("reverb_rs=trace".parse().unwrap())
            .add_directive("rust_spooler=trace".parse().unwrap()),
    };

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    match cli.command {
        Some(Commands::Config) => {
            run_tui();
            return Ok(());
        }
        _ => run_server(cli.verbose >= 3).await,
    }
}

/// Run the main server application
async fn run_server(verbose_debug: bool) -> std::io::Result<()> {
    // Load configuration
    let config = Arc::new(Mutex::new(load_config()));

    // Initialize HTTP client
    let http_client = Client::new();

    // Initialize printer set
    let printers_set = Arc::new(Mutex::new(HashSet::new()));

    // Initial population of printer set and ensure saved printers are up to date
    {
        let system_printers = get_all_printers(verbose_debug).await;
        let mut set = printers_set.lock().unwrap();

        // Load saved printers
        let original_saved_printers = load_printers();
        let mut updated_printers = original_saved_printers.clone();

        // Update saved printers with current system printers
        for printer in system_printers {
            set.insert(printer.name.clone());

            // If printer exists, preserve the printer_id
            if let Some(saved_printer) = original_saved_printers.get(&printer.name) {
                let mut updated_printer = printer.clone();
                updated_printer.printer_id = saved_printer.printer_id;
                updated_printers.insert(printer.name.clone(), updated_printer);
            } else {
                // New printer, add it
                updated_printers.insert(printer.name.clone(), printer);
            }
        }

        // Save updated printers only if they changed
        let printers_were_updated =
            save_printers_if_changed(&updated_printers, &original_saved_printers);
        if printers_were_updated {
            println!(
                "Initial printer configuration updated - saved {} printers",
                updated_printers.len()
            );
        }
    }

    // Spawn printer checker task
    let printers_set_clone = printers_set.clone();
    let config_checker = config.clone();
    let http_client_checker = http_client.clone();

    tokio::spawn(async move {
        printer_checker_task(
            printers_set_clone,
            config_checker,
            http_client_checker,
            verbose_debug,
        )
        .await;
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
            .app_data(web::Data::new(verbose_debug))
            .service(get_printers)
            .service(print_file)
            .service(check_jobs_endpoint)
            .service(check_printers_endpoint)
    })
    .bind(format!("0.0.0.0:{}", api_port))?;

    api_server.run().await?;

    Ok(())
}
