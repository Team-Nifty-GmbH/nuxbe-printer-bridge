use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use actix_web::web;
use reqwest::{Client, StatusCode};

use crate::models::{Config, Printer};
use crate::utils::printer_storage::{load_printers, save_printers};
use crate::models::api::{ApiPrinter, ApiPrinterResponse};

/// Synchronize printers with the API server
pub async fn sync_printers_with_api(
    local_printers: &HashMap<String, Printer>,
    http_client: &Client,
    config: &Config,
) -> Result<HashMap<String, Printer>, Box<dyn std::error::Error>> {
    // Get list of printers from API
    let api_printers = fetch_printers_from_api(http_client, config).await?;

    let mut updated_printers = local_printers.clone();
    let mut api_printer_names: HashSet<String> = HashSet::new();

    // Process API printers
    for api_printer in &api_printers {
        api_printer_names.insert(api_printer.name.clone());

        // Convert ApiPrinter to local Printer
        let printer: Printer = api_printer.into();

        if let Some(existing) = local_printers.get(&printer.name) {
            // Check if printer has changed
            if *existing != printer {
                println!("Printer {} has changed, updating", printer.name);
                updated_printers.insert(printer.name.clone(), printer);
            }
        } else {
            // New printer from API
            println!("Found new printer {} from API", printer.name);
            updated_printers.insert(printer.name.clone(), printer);
        }
    }

    // Find local printers that need to be created in API
    let local_printer_names: HashSet<String> = local_printers.keys().cloned().collect();
    let printers_to_create: Vec<&Printer> = local_printers
        .values()
        .filter(|p| !api_printer_names.contains(&p.name))
        .collect();

    // Create new printers in API
    for printer in printers_to_create {
        match create_printer_in_api(printer, http_client, config).await {
            Ok(new_printer) => {
                println!("Created printer {} in API with ID {}", new_printer.name,
                         new_printer.printer_id.unwrap_or(0));
                updated_printers.insert(new_printer.name.clone(), new_printer);
            },
            Err(e) => {
                eprintln!("Failed to create printer {} in API: {}", printer.name, e);
            }
        }
    }

    // Check for local printers that need updates
    for (name, local_printer) in local_printers {
        if api_printer_names.contains(name) && local_printer.printer_id.is_some() {
            // Check if we need to update this printer in the API
            if let Some(api_printer) = api_printers.iter().find(|p| p.name == *name) {
                let api_printer_local: Printer = api_printer.into();

                if api_printer_local != *local_printer {
                    // Update printer in API
                    match update_printer_in_api(local_printer, http_client, config).await {
                        Ok(updated) => {
                            println!("Updated printer {} in API", updated.name);
                            updated_printers.insert(updated.name.clone(), updated);
                        },
                        Err(e) => {
                            eprintln!("Failed to update printer {} in API: {}", local_printer.name, e);
                        }
                    }
                }
            }
        }
    }

    // Find API printers that are no longer present locally and should be deleted
    let printers_to_delete: Vec<&ApiPrinter> = api_printers
        .iter()
        .filter(|p|
            !local_printer_names.contains(&p.name) &&
                p.printer_server == config.instance_name
        )
        .collect();

    // Delete printers from API that are no longer present locally
    for api_printer in printers_to_delete {
        if let Some(id) = api_printer.id {
            match delete_printer_from_api(id, http_client, config).await {
                Ok(_) => {
                    println!("Deleted printer {} (ID: {}) from API as it's no longer present locally",
                             api_printer.name, id);
                    // Remove from updated_printers if it exists there
                    updated_printers.remove(&api_printer.name);
                },
                Err(e) => {
                    eprintln!("Failed to delete printer {} (ID: {}) from API: {}",
                              api_printer.name, id, e);
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
) -> Result<Vec<ApiPrinter>, Box<dyn std::error::Error>> {
    let api_url = format!("{}/api/printers", config.flux_url);

    let response = http_client
        .get(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("X-Instance-Name", &config.instance_name)
        .header("Accept", "application/json")
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
    let api_url = format!("{}/api/printer", config.flux_url);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    api_printer.printer_server = config.instance_name.clone();

    let response = http_client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("X-Instance-Name", &config.instance_name)
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
    let api_url = format!("{}/api/printer/{}", config.flux_url, printer_id);

    // Convert to ApiPrinter
    let mut api_printer: ApiPrinter = printer.into();
    api_printer.printer_server = config.instance_name.clone();

    let response = http_client
        .put(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("X-Instance-Name", &config.instance_name)
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
    let api_url = format!("{}/api/printer/{}", config.flux_url, printer_id);

    let response = http_client
        .delete(&api_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&"".to_string())))
        .header("X-Instance-Name", &config.instance_name)
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to delete printer: {}", response.status()).into());
    }

    Ok(())
}