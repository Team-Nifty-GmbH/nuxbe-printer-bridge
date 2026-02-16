use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

use reqwest::Client;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::services::print_job::{
    InFlightJobs, job_checker_task, job_status_checker_task, new_in_flight_jobs,
};
use crate::services::printer::{get_all_printers, printer_checker_task};
use crate::services::websocket::websocket_task;
use crate::utils::config::load_config;
use crate::utils::printer_storage::{load_printers, save_printers_if_changed};

/// Run the main server application
pub async fn run_server(verbose_debug: bool) -> std::io::Result<()> {
    let config = Arc::new(RwLock::new(load_config()));
    let http_client = Client::new();
    let printers_set = Arc::new(Mutex::new(HashSet::new()));
    let cancel_token = CancellationToken::new();
    let in_flight_jobs = new_in_flight_jobs();

    initialize_printers(&printers_set, verbose_debug).await;
    let handles = spawn_background_tasks(
        &config,
        &http_client,
        &printers_set,
        &cancel_token,
        &in_flight_jobs,
        verbose_debug,
    );

    info!("Print server started");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, stopping background tasks...");

    // Signal all tasks to stop
    cancel_token.cancel();

    // Wait for all tasks to finish (with timeout)
    for handle in handles {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
    }

    info!("Shutdown complete");
    Ok(())
}

/// Initialize printers from system and sync with saved state
async fn initialize_printers(printers_set: &Arc<Mutex<HashSet<String>>>, verbose_debug: bool) {
    let system_printers = get_all_printers(verbose_debug).await;
    let mut set = printers_set
        .lock()
        .expect("Failed to acquire printers_set lock");

    let original_saved_printers = load_printers();
    let mut updated_printers = original_saved_printers.clone();

    for printer in system_printers {
        set.insert(printer.system_name.clone());

        if let Some(saved_printer) = original_saved_printers.get(&printer.system_name) {
            let mut updated_printer = printer.clone();
            updated_printer.printer_id = saved_printer.printer_id;
            updated_printers.insert(printer.system_name.clone(), updated_printer);
        } else {
            updated_printers.insert(printer.system_name.clone(), printer);
        }
    }

    let printers_were_updated =
        save_printers_if_changed(&updated_printers, &original_saved_printers);
    if printers_were_updated {
        info!(
            printers_count = updated_printers.len(),
            "Initial printer configuration updated"
        );
    }
}

/// Spawn all background tasks
fn spawn_background_tasks(
    config: &Arc<RwLock<crate::models::Config>>,
    http_client: &Client,
    printers_set: &Arc<Mutex<HashSet<String>>>,
    cancel_token: &CancellationToken,
    in_flight_jobs: &InFlightJobs,
    verbose_debug: bool,
) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::new();

    // Printer checker task
    let printers_set_clone = printers_set.clone();
    let config_checker = config.clone();
    let http_client_checker = http_client.clone();
    let token_checker = cancel_token.clone();

    handles.push(tokio::spawn(async move {
        printer_checker_task(
            printers_set_clone,
            config_checker,
            http_client_checker,
            token_checker,
            verbose_debug,
        )
        .await;
    }));

    // Job checker task (polling mode)
    let config_jobs = config.clone();
    let http_client_jobs = http_client.clone();
    let token_jobs = cancel_token.clone();
    let in_flight_jobs_polling = in_flight_jobs.clone();

    handles.push(tokio::spawn(async move {
        job_checker_task(
            config_jobs,
            http_client_jobs,
            token_jobs,
            in_flight_jobs_polling,
        )
        .await;
    }));

    // WebSocket listener task
    let config_ws = config.clone();
    let http_client_ws = http_client.clone();
    let token_ws = cancel_token.clone();
    let in_flight_jobs_ws = in_flight_jobs.clone();

    handles.push(tokio::spawn(async move {
        websocket_task(config_ws, http_client_ws, token_ws, in_flight_jobs_ws).await;
    }));

    // Job status checker task (polls CUPS for final job status)
    let config_status = config.clone();
    let http_client_status = http_client.clone();
    let token_status = cancel_token.clone();
    let in_flight_jobs_status = in_flight_jobs.clone();

    handles.push(tokio::spawn(async move {
        job_status_checker_task(
            config_status,
            http_client_status,
            token_status,
            in_flight_jobs_status,
        )
        .await;
    }));

    handles
}
