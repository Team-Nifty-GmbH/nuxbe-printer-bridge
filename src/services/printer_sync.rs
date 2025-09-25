use reqwest::{Client, StatusCode};
use std::collections::{HashMap, HashSet};

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
) -> Result<HashMap<String, Printer>, Box<dyn std::error::Error>> {
    // 1. We already have local printers from CUPS
    // 2. We already loaded saved_printers from printer.json

    let mut updated_printers = local_printers.clone(); // Start with local printers

    // First, get the existing printers from the API
    let api_printers = fetch_printers_from_api(http_client, config, verbose_debug).await?;

    // Create a map of API printers by name
    let mut api_printer_map = HashMap::new();
    for api_printer in api_printers {
        api_printer_map.insert(api_printer.name.clone(), api_printer);
    }

    // Update local printers with IDs from API printers (preserve IDs)
    for (name, printer) in &mut updated_printers {
        if let Some(api_printer) = api_printer_map.get(name) {
            printer.printer_id = api_printer.id;
            if verbose_debug {
                println!(
                    "Found existing printer in API: {} with ID {}",
                    name,
                    api_printer.id.unwrap_or(0)
                );
            }
        } else if let Some(saved_printer) = saved_printers.get(name) {
            // If not in API but in saved, preserve existing ID
            printer.printer_id = saved_printer.printer_id;
        }
    }

    // 3. Create new printers that don't have IDs yet
    for (name, printer) in updated_printers.iter_mut() {
        if printer.printer_id.is_none() {
            if verbose_debug {
                println!("Creating new printer in API: {}", name);
            }
            match create_printer_in_api(printer, http_client, config, verbose_debug).await {
                Ok(new_printer) => {
                    if verbose_debug {
                        println!(
                            "Created printer {} in API with ID {}",
                            new_printer.name,
                            new_printer.printer_id.unwrap_or(0)
                        );
                    }
                    *printer = new_printer.clone();
                }
                Err(e) => {
                    eprintln!("Failed to create printer {} in API: {}", name, e);
                }
            }
        }
    }

    // 4. Find removed printers (in saved_printers but not in local_printers)
    let local_printer_names: HashSet<String> = local_printers.keys().cloned().collect();
    let saved_printer_names: HashSet<String> = saved_printers.keys().cloned().collect();

    let removed_printers: Vec<&String> = saved_printer_names
        .difference(&local_printer_names)
        .collect();

    for name in removed_printers {
        if let Some(printer) = saved_printers.get(name) {
            if let Some(id) = printer.printer_id {
                // Delete from API
                match delete_printer_from_api(id, http_client, config, verbose_debug).await {
                    Ok(_) => {
                        if verbose_debug {
                            println!("Deleted printer {} (ID: {}) from API", name, id);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to delete printer {} (ID: {}) from API: {}",
                            name, id, e
                        );
                    }
                }
            }
        }
    }

    // 5. Update changed printers
    for (name, local_printer) in local_printers {
        if let Some(saved_printer) = saved_printers.get(name) {
            // Check if printer exists in both and has an ID
            if saved_printer.printer_id.is_some() && *local_printer != *saved_printer {
                // Get the updated printer from our map
                if let Some(printer) = updated_printers.get_mut(name) {
                    if verbose_debug {
                        println!("Updating printer {} in API", name);
                    }
                    match update_printer_in_api(printer, http_client, config, verbose_debug).await {
                        Ok(_) => {
                            if verbose_debug {
                                println!("Updated printer {} in API", name);
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to update printer {} in API: {}", name, e);
                        }
                    }
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
) -> Result<Vec<ApiPrinter>, Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers", config.flux_url);

    let response = with_auth_header(http_client.get(&api_url), config)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "instance_name": config.instance_name
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch printers from API: {}", response.status()).into());
    }

    let response_text = response.text().await?;
    if verbose_debug {
        println!("API response: {}", response_text);
    }

    let parsed_response: ApiPrinterResponse = serde_json::from_str(&response_text)?;
    Ok(parsed_response.data.data)
}

async fn create_printer_in_api(
    printer: &Printer,
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> Result<Printer, Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers", config.flux_url);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    api_printer.spooler_name = config.instance_name.clone(); // Set spooler_name instead of printer_server

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
        println!("API create response: {}", response_text);
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
) -> Result<Printer, Box<dyn std::error::Error>> {
    if printer.printer_id.is_none() {
        return Err("Cannot update printer without an ID".into());
    }

    let printer_id = printer.printer_id.unwrap();
    let api_url = format!("{}/api/printers/{}", config.flux_url, printer_id);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    api_printer.spooler_name = config.instance_name.clone();

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
        println!("API update response: {}", response_text);
    }

    // Return the updated printer
    Ok(printer.clone())
}

async fn delete_printer_from_api(
    printer_id: u32,
    http_client: &Client,
    config: &Config,
    verbose_debug: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers/{}", config.flux_url, printer_id);

    let response = with_auth_header(http_client.delete(&api_url), config)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "spooler_name": config.instance_name // Changed from instance_name
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status(); // Save the status before consuming the response
        let error_text = response.text().await?;
        return Err(format!("Failed to delete printer: {} - {}", status, error_text).into());
    }

    if verbose_debug {
        println!("Successfully deleted printer with ID: {}", printer_id);
    }

    Ok(())
}
