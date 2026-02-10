use std::collections::{HashMap, HashSet};

use reqwest::{Client, StatusCode};
use tracing::{debug, error, info, trace};

use crate::error::SpoolerResult;
use crate::models::api::{ApiPrinter, ApiPrinterResponse};
use crate::models::{Config, Printer};
use crate::utils::http::with_auth_header;

/// Synchronize printers with the API server following the specified order
pub async fn sync_printers_with_api(
    local_printers: &HashMap<String, Printer>,
    saved_printers: &HashMap<String, Printer>,
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> SpoolerResult<HashMap<String, Printer>> {
    info!(
        local_count = local_printers.len(),
        saved_count = saved_printers.len(),
        api_url = %config.flux_url,
        "Syncing printers with API"
    );

    let mut updated_printers = local_printers.clone();

    let api_printers = fetch_printers_from_api(http_client, config, verbose_debug).await?;
    info!(api_count = api_printers.len(), "Fetched printers from API");

    // Split API printers into two maps for matching:
    // 1. Printers WITH system_name -- keyed by system_name (stable identification)
    // 2. Legacy printers WITHOUT system_name -- keyed by display name (fallback match)
    let mut api_by_system_name: HashMap<String, ApiPrinter> = HashMap::new();
    let mut api_by_name: HashMap<String, ApiPrinter> = HashMap::new();
    for api_printer in api_printers {
        if api_printer.spooler_name != config.instance_name {
            continue;
        }
        if let Some(ref sys_name) = api_printer.system_name {
            api_by_system_name.insert(sys_name.clone(), api_printer);
        } else {
            api_by_name.insert(api_printer.name.clone(), api_printer);
        }
    }

    // Track printers matched via name fallback so we can force-update them with system_name/uri
    let mut legacy_matched: HashSet<String> = HashSet::new();

    for (system_name, printer) in &mut updated_printers {
        // Pass 1: Match by system_name (stable identification)
        if let Some(api_printer) = api_by_system_name.get(system_name) {
            printer.printer_id = api_printer.id;
            if verbose_debug {
                trace!(
                    printer = %printer.name,
                    system_name = %system_name,
                    id = api_printer.id.unwrap_or(0),
                    "Found existing printer in API by system_name"
                );
            }
        }
        // Pass 2: Fallback match by display name for legacy printers (system_name is null in API)
        else if let Some(api_printer) = api_by_name.get(&printer.name) {
            printer.printer_id = api_printer.id;
            legacy_matched.insert(system_name.clone());
            info!(
                printer = %printer.name,
                system_name = %system_name,
                id = api_printer.id.unwrap_or(0),
                "Matched legacy printer by name, will update with system_name and uri"
            );
        }
        // Pass 3: Fall back to saved printer_id
        else if let Some(saved_printer) = saved_printers.get(system_name) {
            printer.printer_id = saved_printer.printer_id;
        }
    }

    for (_system_name, printer) in updated_printers.iter_mut() {
        if printer.printer_id.is_none() {
            if verbose_debug {
                debug!(printer = %printer.name, "Creating new printer in API");
            }
            match create_printer_in_api(printer, http_client, config, verbose_debug).await {
                Ok(new_printer) => {
                    if verbose_debug {
                        debug!(
                            printer = %new_printer.name,
                            id = new_printer.printer_id.unwrap_or(0),
                            "Created printer in API"
                        );
                    }
                    *printer = new_printer.clone();
                }
                Err(e) => {
                    error!(printer = %printer.name, error = %e, "Failed to create printer in API");
                }
            }
        }
    }

    // 4. Find removed printers (in saved_printers but not in local_printers)
    // Iterate directly instead of creating intermediate HashSets
    for (system_name, printer) in saved_printers {
        // Skip if printer exists in local_printers
        if local_printers.contains_key(system_name) {
            continue;
        }

        let Some(id) = printer.printer_id else {
            continue;
        };

        // Delete from API
        match delete_printer_from_api(id, http_client, config, verbose_debug).await {
            Ok(_) => {
                if verbose_debug {
                    debug!(printer = %printer.name, id, "Deleted printer from API");
                }
            }
            Err(e) => {
                error!(
                    printer = %printer.name,
                    id,
                    error = %e,
                    "Failed to delete printer from API"
                );
            }
        }
    }

    // 5. Update changed printers (including legacy-matched ones that need system_name/uri)
    for (system_name, local_printer) in local_printers {
        let needs_update = if let Some(saved_printer) = saved_printers.get(system_name) {
            saved_printer.printer_id.is_some() && *local_printer != *saved_printer
        } else {
            false
        };

        // Also force update for legacy-matched printers missing system_name/uri in API
        let is_legacy = legacy_matched.contains(system_name);

        if (needs_update || is_legacy)
            && let Some(printer) = updated_printers.get_mut(system_name)
            && printer.printer_id.is_some()
        {
            if verbose_debug || is_legacy {
                debug!(
                    printer = %printer.name,
                    is_legacy,
                    "Updating printer in API"
                );
            }
            match update_printer_in_api(printer, http_client, config, verbose_debug).await {
                Ok(_) => {
                    if verbose_debug || is_legacy {
                        debug!(printer = %printer.name, "Updated printer in API");
                    }
                }
                Err(e) => {
                    error!(printer = %printer.name, error = %e, "Failed to update printer in API");
                }
            }
        }
    }

    Ok(updated_printers)
}

/// Fetch printers from the API
async fn fetch_printers_from_api(
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> SpoolerResult<Vec<ApiPrinter>> {
    // Fetch active printers for this instance (spooler_name = instance_name)
    let api_url = format!(
        "{}/api/printers?filter[is_active]=true&filter[spooler_name]={}",
        config.flux_url,
        urlencoding::encode(&config.instance_name)
    );

    if verbose_debug {
        trace!(url = %api_url, "Fetching printers from API");
    }

    let response = with_auth_header(http_client.get(&api_url), config)
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Failed to fetch printers from API: {} - {}",
            status, error_text
        )
        .into());
    }

    let response_text = response.text().await?;
    if verbose_debug {
        trace!(response = %response_text, "API response");
    }

    let parsed_response: ApiPrinterResponse = serde_json::from_str(&response_text)?;
    Ok(parsed_response.data.data)
}

async fn create_printer_in_api(
    printer: &Printer,
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> SpoolerResult<Printer> {
    let api_url = format!("{}/api/printers", config.flux_url);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    // spooler_name is the instance name (identifies which print server this printer belongs to)
    api_printer.spooler_name = config.instance_name.clone();

    if verbose_debug {
        trace!(payload = ?api_printer, "Creating printer with payload");
    }

    let response = with_auth_header(http_client.post(&api_url), config)
        .header("Accept", "application/json")
        .json(&api_printer)
        .send()
        .await?;

    if response.status() != StatusCode::CREATED && !response.status().is_success() {
        let status = response.status(); // Save the status before consuming the response
        let error_text = response.text().await?;
        return Err(format!("Failed to create printer: {} - {}", status, error_text).into());
    }

    let response_text = response.text().await?;
    if verbose_debug {
        trace!(response = %response_text, "API create response");
    }

    let response_data: serde_json::Value = serde_json::from_str(&response_text)?;

    // Extract the new printer ID
    let printer_id = response_data["data"]["id"]
        .as_u64()
        .ok_or("Failed to get printer ID from response")?;

    // Create a new printer with the ID
    let mut new_printer = printer.clone();
    new_printer.printer_id = Some(printer_id as u32);

    Ok(new_printer)
}

async fn update_printer_in_api(
    printer: &Printer,
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> SpoolerResult<Printer> {
    if printer.printer_id.is_none() {
        return Err("Cannot update printer without an ID".into());
    }

    // PUT /api/printers expects ID in the request body, not URL path
    let api_url = format!("{}/api/printers", config.flux_url);

    // Convert to ApiPrinter - id will be included in the JSON body
    let mut api_printer: ApiPrinter = printer.into();
    // spooler_name is the instance name (identifies which print server this printer belongs to)
    api_printer.spooler_name = config.instance_name.clone();
    // Ensure ID is set for update
    api_printer.id = printer.printer_id;

    if verbose_debug {
        trace!(payload = ?api_printer, "Updating printer with payload");
    }

    let response = with_auth_header(http_client.put(&api_url), config)
        .header("Accept", "application/json")
        .json(&api_printer)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status(); // Save the status before consuming the response
        let error_text = response.text().await?;
        return Err(format!("Failed to update printer: {} - {}", status, error_text).into());
    }

    if verbose_debug {
        let response_text = response.text().await?;
        trace!(response = %response_text, "API update response");
    }

    // Return the updated printer
    Ok(printer.clone())
}

async fn delete_printer_from_api(
    printer_id: u32,
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> SpoolerResult<()> {
    let api_url = format!("{}/api/printers/{}", config.flux_url, printer_id);

    let response = with_auth_header(http_client.delete(&api_url), config)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "spooler_name": config.instance_name // Changed from instance_name
        }))
        .send()
        .await?;

    if response.status() == StatusCode::NOT_FOUND {
        // Printer already gone from API — treat as success
        if verbose_debug {
            debug!(id = printer_id, "Printer already deleted from API");
        }
        return Ok(());
    }

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        return Err(format!("Failed to delete printer: {} - {}", status, error_text).into());
    }

    if verbose_debug {
        debug!(id = printer_id, "Successfully deleted printer from API");
    }

    Ok(())
}
