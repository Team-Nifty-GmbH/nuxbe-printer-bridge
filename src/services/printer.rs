use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use printers::get_printers;
use reqwest::Client;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::error::SpoolerResult;
use crate::models::Printer;
use crate::services::printer_sync::sync_printers_with_api;
use crate::utils::config::read_config;
use crate::utils::printer_storage::{load_printers, save_printers_if_changed};

/// Shared cache of synced printers, keyed by CUPS system_name. Written by the
/// printer checker task, read by the print workflow to resolve printer ids.
pub type PrinterCache = Arc<RwLock<HashMap<String, Printer>>>;

/// Create a new empty printer cache.
pub fn new_printer_cache() -> PrinterCache {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Query CUPS for supported media sizes of a printer via `lpoptions -p <name> -l`
fn query_media_sizes(printer_name: &str, verbose_debug: bool) -> Vec<String> {
    let output = match Command::new("lpoptions")
        .args(["-p", printer_name, "-l"])
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            debug!(printer = %printer_name, error = %e, "Failed to run lpoptions");
            return Vec::new();
        }
    };

    if !output.status.success() {
        if verbose_debug {
            debug!(
                printer = %printer_name,
                stderr = %String::from_utf8_lossy(&output.stderr),
                "lpoptions returned non-zero exit code"
            );
        }
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the PageSize line, e.g.: "PageSize/Media Size: *A4 Env10 EnvC5 Letter"
    // The default size is prefixed with '*'
    for line in stdout.lines() {
        if line.starts_with("PageSize/") || line.starts_with("PageSize:") {
            let Some(sizes_part) = line.split(':').nth(1) else {
                continue;
            };
            let sizes: Vec<String> = sizes_part
                .split_whitespace()
                .map(|s| s.trim_start_matches('*').to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if verbose_debug {
                trace!(
                    printer = %printer_name,
                    count = sizes.len(),
                    sizes = ?sizes,
                    "Queried media sizes from CUPS"
                );
            }

            return sizes;
        }
    }

    if verbose_debug {
        debug!(printer = %printer_name, "No PageSize line found in lpoptions output");
    }

    Vec::new()
}

/// Get all available printers from the CUPS system (blocking operation)
fn get_all_printers_blocking(verbose_debug: bool) -> Vec<Printer> {
    // Skip mDNS implicit-class duplicates (e.g. "Printer@hostname.local").
    // These are CUPS-discovered shadows of real printers with implicitclass://
    // URIs that cannot be printed to directly.
    let system_printers: Vec<_> = get_printers()
        .into_iter()
        .filter(|p| {
            if p.system_name.contains('@') {
                debug!(
                    printer = %p.name,
                    system_name = %p.system_name,
                    "Skipping mDNS implicit-class duplicate"
                );
                false
            } else {
                true
            }
        })
        .collect();

    if verbose_debug {
        debug!(count = system_printers.len(), "Found system printers");
    }

    // Query media sizes concurrently — one lpoptions subprocess per printer.
    // CUPS expects the queue name (system_name), not the display name.
    let media_sizes: Vec<Vec<String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = system_printers
            .iter()
            .map(|p| scope.spawn(move || query_media_sizes(&p.system_name, verbose_debug)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });

    system_printers
        .into_iter()
        .zip(media_sizes)
        .map(|(system_printer, media_sizes)| {
            if media_sizes.is_empty() {
                warn!(
                    printer = %system_printer.name,
                    system_name = %system_printer.system_name,
                    "No media sizes returned from CUPS, printer may not be fully configured"
                );
            }

            Printer {
                name: system_printer.name,
                system_name: system_printer.system_name,
                uri: if system_printer.uri.is_empty() {
                    None
                } else {
                    Some(system_printer.uri)
                },
                description: system_printer.description,
                location: system_printer.location,
                make_and_model: system_printer.driver_name,
                media_sizes,
                printer_id: None,
            }
        })
        .collect()
}

/// Get all available printers from the CUPS system
pub async fn get_all_printers(verbose_debug: bool) -> Vec<Printer> {
    tokio::task::spawn_blocking(move || get_all_printers_blocking(verbose_debug))
        .await
        .unwrap_or_default()
}

/// Check for new printers, sync with the API, and refresh the shared cache
pub async fn check_for_new_printers(
    printer_cache: &PrinterCache,
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

    let new_printers: Vec<Printer> = updated_printers
        .values()
        .filter(|p| !saved_printers.contains_key(&p.system_name))
        .cloned()
        .collect();

    *crate::utils::sync::write(printer_cache) = updated_printers;

    Ok(new_printers)
}

/// Log discovered printers
fn log_new_printers(printers: &[Printer]) {
    if printers.is_empty() {
        return;
    }
    info!(count = printers.len(), "Found new printers");
    for printer in printers {
        info!(printer = %printer.name, "New printer discovered");
    }
}

/// Background task to periodically check for new printers.
/// The first iteration runs immediately and doubles as startup initialization.
pub async fn printer_checker_task(
    printer_cache: PrinterCache,
    config: Arc<RwLock<crate::models::Config>>,
    http_client: Client,
    cancel_token: CancellationToken,
    verbose_debug: bool,
) {
    let interval = read_config(&config).printer_check_interval;
    info!("Starting printer sync (interval: {} minutes)", interval);

    loop {
        match check_for_new_printers(&printer_cache, &http_client, &config, verbose_debug).await {
            Ok(new_printers) => log_new_printers(&new_printers),
            Err(e) => error!(error = %e, "Error checking for new printers"),
        }

        let interval = read_config(&config).printer_check_interval.max(1);

        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Printer checker task shutting down");
                return;
            }
            _ = time::sleep(Duration::from_secs(interval * 60)) => {}
        }
    }
}
