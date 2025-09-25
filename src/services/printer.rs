use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use actix_web::web;
use printers::{get_printers, get_printer_by_name};
use reqwest::Client;
use tokio::time;

use crate::models::{Config, Printer};
use crate::services::printer_sync::sync_printers_with_api;
use crate::utils::printer_storage::{load_printers, save_printers_if_changed};

/// Get all available printers from the CUPS system
pub async fn get_all_printers(verbose_debug: bool) -> Vec<Printer> {
    let system_printers = get_printers();
    let mut printers = Vec::new();

    if verbose_debug {
        println!("Found {} system printer(s)", system_printers.len());
    }

    for system_printer in system_printers {
        if verbose_debug {
            println!("Processing printer: {}", system_printer.name);
        }

        // Get detailed printer info if available
        let detailed_info = get_printer_by_name(&system_printer.name);

        let printer = Printer {
            name: system_printer.name.clone(),
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
            media_sizes: Vec::new(), // The printers crate doesn't provide media_sizes, we'll need to get this separately if needed
            printer_id: None, // IDs will be populated from saved printers later
        };

        printers.push(printer);
    }

    if verbose_debug {
        println!("Successfully processed {} printer(s)", printers.len());
    }

    printers
}

/// Check for new printers and update the stored printers
pub async fn check_for_new_printers(
    printers_data: web::Data<Arc<Mutex<HashSet<String>>>>,
    http_client: web::Data<Client>,
    config: web::Data<Arc<Mutex<Config>>>,
    verbose_debug: bool,
) -> Result<Vec<Printer>, Box<dyn std::error::Error>> {
    // 1. Get current printers from CUPS
    let current_printers = get_all_printers(verbose_debug).await;

    // 2. Load saved printers from printer.json
    let saved_printers = load_printers();

    // Convert current printers list to hashmap with proper IDs from saved_printers
    let mut current_printers_map: HashMap<String, Printer> = HashMap::new();
    for printer in current_printers {
        let mut updated_printer = printer.clone();

        // If printer exists in saved_printers, preserve its printer_id
        if let Some(saved_printer) = saved_printers.get(&printer.name) {
            updated_printer.printer_id = saved_printer.printer_id;
        }

        current_printers_map.insert(printer.name.clone(), updated_printer);
    }

    // Get the required configuration
    let config_clone = {
        let guard = config.lock().unwrap();
        guard.clone()
    };

    // 3-6. Sync with API following the specified order of operations
    let sync_result = sync_printers_with_api(
        &current_printers_map,
        &saved_printers,
        &http_client,
        &config_clone,
        verbose_debug,
    )
    .await;

    let updated_printers = match sync_result {
        Ok(printers) => printers,
        Err(e) => {
            eprintln!("Error syncing printers with API: {}", e);
            // If sync fails, just use current printers with saved IDs
            current_printers_map
        }
    };

    // Save the updated printers only if they have changed
    let printers_were_updated = save_printers_if_changed(&updated_printers, &saved_printers);
    if printers_were_updated {
        println!("Printer configuration updated - saved {} printers", updated_printers.len());
    }

    // Update the printers_data set with current printer names
    {
        let mut printers_set = printers_data.lock().unwrap();
        printers_set.clear();
        for printer in updated_printers.keys() {
            printers_set.insert(printer.clone());
        }
    }

    // Return new printers (those not in the old saved_printers)
    let new_printers: Vec<Printer> = updated_printers
        .values()
        .filter(|p| !saved_printers.contains_key(&p.name))
        .cloned()
        .collect();

    Ok(new_printers)
}

/// Background task to periodically check for new printers
pub async fn printer_checker_task(
    printers_data: Arc<Mutex<HashSet<String>>>,
    config: Arc<Mutex<Config>>,
    http_client: Client,
    verbose_debug: bool,
) {
    let printers_data = web::Data::new(printers_data);
    let config_data = web::Data::new(config);
    let client_data = web::Data::new(http_client);

    // Initial check at startup
    match check_for_new_printers(
        printers_data.clone(),
        client_data.clone(),
        config_data.clone(),
        verbose_debug,
    )
    .await
    {
        Ok(new_printers) => {
            if !new_printers.is_empty() {
                println!("Found {} new printer(s) at startup", new_printers.len());
                for printer in new_printers {
                    println!("  - {}", printer.name);
                }
            }
        }
        Err(e) => eprintln!("Error checking for new printers at startup: {}", e),
    }

    // 7. Continue checking at the configured interval
    loop {
        let interval = { config_data.lock().unwrap().printer_check_interval };

        // Sleep first before checking again
        time::sleep(Duration::from_secs(interval * 60)).await;

        match check_for_new_printers(
            printers_data.clone(),
            client_data.clone(),
            config_data.clone(),
            verbose_debug,
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
    }
}
