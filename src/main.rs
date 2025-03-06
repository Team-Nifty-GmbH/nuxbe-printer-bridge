use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use actix_files as fs_web;
use actix_web::{App, HttpServer, web};
use local_ip_address::local_ip;
use reqwest::Client;

mod api;
mod config;
mod models;
mod services;
mod utils;

use api::admin::{get_config, update_config};
use api::routes::{check_jobs_endpoint, check_printers_endpoint, get_printers, print_file};
use config::{admin_dir, load_config};
use services::print_job::job_checker_task;
use services::printer::{get_all_printers, printer_checker_task};
use services::websocket::websocket_task;
use utils::admin_interface::create_admin_interface;

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
    let admin_server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&config_clone)))
            .service(get_config)
            .service(update_config)
            .service(fs_web::Files::new("/", admin_dir().to_str().unwrap()).index_file("index.html"))
    })
        .bind(format!("0.0.0.0:{}", admin_port))?;

    // Create admin folder and HTML
    create_admin_interface()?;

    // Run both servers
    futures::future::try_join(api_server.run(), admin_server.run()).await?;

    Ok(())
}