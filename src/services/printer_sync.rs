use std::collections::{HashMap, HashSet};
use reqwest::{Client, StatusCode};

use crate::models::{Config, Printer};
use crate::models::api::{ApiPrinter, ApiPrinterResponse};

/// Synchronize printers with the API server following the specified order
pub async fn sync_printers_with_api(
    local_printers: &HashMap<String, Printer>,
    saved_printers: &HashMap<String, Printer>,
    http_client: &Client,
    config: &Config,
) -> Result<HashMap<String, Printer>, Box<dyn std::error::Error>> {
    // 1. We already have local printers from CUPS
    // 2. We already loaded saved_printers from printer.json

    let mut updated_printers = saved_printers.clone();

    // 3. Find new printers (without ID) and send POST requests
    let mut new_printer_names: Vec<String> = Vec::new();
    for (name, printer) in local_printers {
        if !saved_printers.contains_key(name) || saved_printers[name].printer_id.is_none() {
            // This is a new printer, create it in API
            match create_printer_in_api(printer, http_client, config).await {
                Ok(new_printer) => {
                    println!("Created printer {} in API with ID {}",
                             new_printer.name, new_printer.printer_id.unwrap_or(0));
                    updated_printers.insert(name.clone(), new_printer.clone());
                    new_printer_names.push(name.clone());
                },
                Err(e) => {
                    eprintln!("Failed to create printer {} in API: {}", name, e);
                }
            }
        }
    }

    // 4. Get updated printer list with IDs from API
    let api_printers = fetch_printers_from_api(http_client, config).await?;
    let mut api_printer_map = HashMap::new();
    for api_printer in api_printers {
        api_printer_map.insert(api_printer.name.clone(), api_printer);
    }

    // Update local printers with IDs from API
    for name in &new_printer_names {
        if let Some(api_printer) = api_printer_map.get(name) {
            if let Some(printer) = updated_printers.get_mut(name) {
                printer.printer_id = api_printer.id;
            }
        }
    }

    // 5. Find removed printers (in saved_printers but not in local_printers)
    let local_printer_names: HashSet<String> = local_printers.keys().cloned().collect();
    let saved_printer_names: HashSet<String> = saved_printers.keys().cloned().collect();

    let removed_printers: Vec<&String> = saved_printer_names
        .difference(&local_printer_names)
        .collect();

    for name in removed_printers {
        if let Some(printer) = saved_printers.get(name) {
            if let Some(id) = printer.printer_id {
                // Delete from API
                match delete_printer_from_api(id, http_client, config).await {
                    Ok(_) => {
                        println!("Deleted printer {} (ID: {}) from API", name, id);
                        // Remove from updated_printers if deletion was successful
                        updated_printers.remove(name);
                    },
                    Err(e) => {
                        eprintln!("Failed to delete printer {} (ID: {}) from API: {}", name, id, e);
                    }
                }
            } else {
                // No ID, just remove locally
                updated_printers.remove(name);
            }
        }
    }

    // 6. Check for changed printers
    for (name, local_printer) in local_printers {
        if let Some(saved_printer) = saved_printers.get(name) {
            // Check if printer exists in both and has an ID
            if saved_printer.printer_id.is_some() && *local_printer != *saved_printer {
                // Printer has changed, update it
                let mut updated_printer = local_printer.clone();
                updated_printer.printer_id = saved_printer.printer_id;

                match update_printer_in_api(&updated_printer, http_client, config).await {
                    Ok(_) => {
                        println!("Updated printer {} in API", name);
                        updated_printers.insert(name.clone(), updated_printer);
                    },
                    Err(e) => {
                        eprintln!("Failed to update printer {} in API: {}", name, e);
                    }
                }
            } else if saved_printer.printer_id.is_none() {
                // Make sure this printer is in updated_printers
                updated_printers.insert(name.clone(), local_printer.clone());
            }
        } else {
            // This shouldn't happen as we've already processed new printers,
            // but include it for completeness
            updated_printers.insert(name.clone(), local_printer.clone());
        }
    }

    Ok(updated_printers)
}

/// Fetch printers from the API
async fn fetch_printers_from_api(
    http_client: &Client,
    config: &Config,
) -> Result<Vec<ApiPrinter>, Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers", config.flux_url);

    let response = http_client
        .get(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
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
    println!("API response: {}", response_text);

    let parsed_response: ApiPrinterResponse = serde_json::from_str(&response_text)?;
    Ok(parsed_response.data.data)
}

/// Create a new printer in the API
async fn create_printer_in_api(
    printer: &Printer,
    http_client: &Client,
    config: &Config,
) -> Result<Printer, Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers", config.flux_url);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    api_printer.printer_server = config.instance_name.clone();

    let response = http_client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("Accept", "application/json")
        .json(&api_printer)
        .send()
        .await?;

    if response.status() != StatusCode::CREATED && !response.status().is_success() {
        return Err(format!("Failed to create printer: {}", response.status()).into());
    }

    let response_text = response.text().await?;
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

/// Update an existing printer in the API
async fn update_printer_in_api(
    printer: &Printer,
    http_client: &Client,
    config: &Config,
) -> Result<Printer, Box<dyn std::error::Error>> {
    if printer.printer_id.is_none() {
        return Err("Cannot update printer without an ID".into());
    }

    let printer_id = printer.printer_id.unwrap();
    let api_url = format!("{}/api/printers/{}", config.flux_url, printer_id);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    api_printer.printer_server = config.instance_name.clone();

    let response = http_client
        .put(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("Accept", "application/json")
        .json(&api_printer)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to update printer: {}", response.status()).into());
    }

    // Return the updated printer
    Ok(printer.clone())
}

/// Delete a printer from the API
async fn delete_printer_from_api(
    printer_id: u32,
    http_client: &Client,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers/{}", config.flux_url, printer_id);

    let response = http_client
        .delete(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "instance_name": config.instance_name
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to delete printer: {}", response.status()).into());
    }

    Ok(())
}