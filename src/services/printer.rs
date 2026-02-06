use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use printers::{get_printer_by_name, get_printers};
use reqwest::Client;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace};

use crate::error::SpoolerResult;
use crate::models::Printer;
use crate::services::printer_sync::sync_printers_with_api;
use crate::utils::config::read_config;
use crate::utils::printer_storage::{load_printers, save_printers_if_changed};

/// Get all available printers from the CUPS system (blocking operation)
fn get_all_printers_blocking(verbose_debug: bool) -> Vec<Printer> {
    let system_printers = get_printers();
    let mut printers = Vec::with_capacity(system_printers.len());

    if verbose_debug {
        debug!(count = system_printers.len(), "Found system printers");
    }

    for system_printer in system_printers {
        if verbose_debug {
            trace!(printer = %system_printer.name, "Processing printer");
        }

        let detailed_info = get_printer_by_name(&system_printer.name);

        let printer = Printer {
            name: system_printer.name.clone(),
            system_name: system_printer.system_name.clone(),
            uri: Some(system_printer.uri.clone()),
            description: detailed_info
                .as_ref()
                .map(|p| p.description.clone())
                .unwrap_or_else(|| system_printer.description.clone()),
            location: detailed_info
                .as_ref()
                .map(|p| p.location.clone())
                .unwrap_or_else(|| system_printer.location.clone()),
            make_and_model: detailed_info
                .as_ref()
                .map(|p| p.driver_name.clone())
                .unwrap_or_else(|| system_printer.driver_name.clone()),
            media_sizes: Vec::new(),
            printer_id: None,
        };

        printers.push(printer);
    }

    if verbose_debug {
        debug!(count = printers.len(), "Successfully processed printers");
    }

    printers
}

/// Get all available printers from the CUPS system
pub async fn get_all_printers(verbose_debug: bool) -> Vec<Printer> {
    tokio::task::spawn_blocking(move || get_all_printers_blocking(verbose_debug))
        .await
        .unwrap_or_default()
}

/// Check for new printers and update the stored printers
pub async fn check_for_new_printers(
    printers_data: Arc<Mutex<HashSet<String>>>,
    http_client: &Client,
    config: &Arc<RwLock<crate::models::Config>>,
    verbose_debug: bool,
) -> SpoolerResult<Vec<Printer>> {
    let current_printers = get_all_printers(verbose_debug).await;
    let saved_printers = load_printers();
    let mut current_printers_map: HashMap<String, Printer> =
        HashMap::with_capacity(current_printers.len());

    for mut printer in current_printers {
        if let Some(saved_printer) = saved_printers.get(&printer.system_name) {
            printer.printer_id = saved_printer.printer_id;
        }
        current_printers_map.insert(printer.system_name.clone(), printer);
    }

    let config_clone = read_config(config);
    let sync_result = sync_printers_with_api(
        &current_printers_map,
        &saved_printers,
        http_client,
        &config_clone,
        verbose_debug,
    )
    .await;

    let updated_printers = match sync_result {
        Ok(printers) => printers,
        Err(e) => {
            error!(error = %e, "Error syncing printers with API");
            current_printers_map
        }
    };
    let printers_were_updated = save_printers_if_changed(&updated_printers, &saved_printers);
    if printers_were_updated {
        info!(
            count = updated_printers.len(),
            "Printer configuration updated"
        );
    }

    {
        let mut printers_set = printers_data.lock().expect("Failed to acquire printers_data lock");
        printers_set.clear();
        for printer in updated_printers.keys() {
            printers_set.insert(printer.clone());
        }
    }
    let new_printers: Vec<Printer> = updated_printers
        .values()
        .filter(|p| !saved_printers.contains_key(&p.system_name))
        .cloned()
        .collect();

    Ok(new_printers)
}

/// Log discovered printers
fn log_new_printers(printers: &[Printer], context: &str) {
    if printers.is_empty() {
        return;
    }
    info!(count = printers.len(), "Found new printers{}", context);
    for printer in printers {
        info!(printer = %printer.name, "New printer discovered");
    }
}

/// Background task to periodically check for new printers
pub async fn printer_checker_task(
    printers_data: Arc<Mutex<HashSet<String>>>,
    config: Arc<RwLock<crate::models::Config>>,
    http_client: Client,
    cancel_token: CancellationToken,
    verbose_debug: bool,
) {
    let interval = read_config(&config).printer_check_interval;
    info!("Starting printer sync (interval: {} minutes)", interval);

    // Initial check at startup
    match check_for_new_printers(printers_data.clone(), &http_client, &config, verbose_debug).await
    {
        Ok(new_printers) => log_new_printers(&new_printers, " at startup"),
        Err(e) => error!(error = %e, "Error checking for new printers at startup"),
    }

    // Periodic checks
    loop {
        let interval = read_config(&config).printer_check_interval;

        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Printer checker task shutting down");
                return;
            }
            _ = time::sleep(Duration::from_secs(interval * 60)) => {}
        }

        if cancel_token.is_cancelled() {
            return;
        }

        match check_for_new_printers(printers_data.clone(), &http_client, &config, verbose_debug)
            .await
        {
            Ok(new_printers) => log_new_printers(&new_printers, ""),
            Err(e) => error!(error = %e, "Error checking for new printers"),
        }
    }
}
