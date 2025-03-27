use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use actix_web::web;
use reqwest::Client;
use tokio::time;

use crate::models::{Config, Printer};
use crate::utils::printer_storage::{load_printers, save_printers};
use crate::services::printer_sync::sync_printers_with_api;

/// Get all available printers from the CUPS system
pub async fn get_all_printers() -> Vec<Printer> {
    // Debug lpstat
    let debug_output = Command::new("lpstat")
        .arg("-a")
        .output()
        .expect("Failed to execute lpstat -a command");

    println!(
        "Debug lpstat -a: {}",
        String::from_utf8_lossy(&debug_output.stdout)
    );

    let lpstat_output = Command::new("lpstat")
        .arg("-a")
        .output()
        .expect("Failed to execute lpstat command");

    let printer_list_str = String::from_utf8_lossy(&lpstat_output.stdout);
    let printer_names: Vec<String> = printer_list_str
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                Some(parts[0].to_string())
            } else {
                None
            }
        })
        .collect();

    let mut final_printer_names = printer_names;
    if final_printer_names.is_empty() {
        let alt_output = Command::new("lpstat")
            .arg("-p")
            .output()
            .expect("Failed to execute lpstat -p command");

        println!(
            "Debug lpstat -p: {}",
            String::from_utf8_lossy(&alt_output.stdout)
        );

        let alt_list_str = String::from_utf8_lossy(&alt_output.stdout);
        final_printer_names = alt_list_str
            .lines()
            .filter_map(|line| {
                if line.starts_with("printer ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        return Some(parts[1].to_string());
                    }
                }
                None
            })
            .collect();
    }

    if final_printer_names.is_empty() {
        let v_output = Command::new("lpstat")
            .arg("-v")
            .output()
            .expect("Failed to execute lpstat -v command");

        println!(
            "Debug lpstat -v: {}",
            String::from_utf8_lossy(&v_output.stdout)
        );

        let v_list_str = String::from_utf8_lossy(&v_output.stdout);
        final_printer_names = v_list_str
            .lines()
            .filter_map(|line| {
                if line.starts_with("device for ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        return Some(parts[2].trim_end_matches(':').to_string());
                    }
                }
                None
            })
            .collect();
    }

    if final_printer_names.is_empty() {
        let shell_output = Command::new("sh")
            .arg("-c")
            .arg("lpstat -a | cut -d' ' -f1")
            .output()
            .expect("Failed to execute shell command");

        println!(
            "Debug shell command: {}",
            String::from_utf8_lossy(&shell_output.stdout)
        );

        let shell_list_str = String::from_utf8_lossy(&shell_output.stdout);
        final_printer_names = shell_list_str
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();
    }

    println!("Detected printers: {:?}", final_printer_names);

    let mut printers = Vec::new();

    // Load saved printers to preserve printer_id values
    let saved_printers = load_printers();

    for name in final_printer_names {
        // Try to get media sizes
        let lpoptions_output = Command::new("lpoptions")
            .arg("-p")
            .arg(&name)
            .arg("-l")
            .output();

        let mut media_sizes = Vec::new();

        if let Ok(output) = lpoptions_output {
            let printer_options = String::from_utf8_lossy(&output.stdout);

            for line in printer_options.lines() {
                if line.starts_with("PageSize/")
                    || line.starts_with("MediaSize/")
                    || line.contains("media size")
                {
                    if let Some(options_part) = line.split(':').nth(1) {
                        let sizes: Vec<String> = options_part
                            .split_whitespace()
                            .filter_map(|opt| {
                                if opt.starts_with('*') {
                                    Some(opt.trim_start_matches('*').to_string())
                                } else {
                                    Some(opt.to_string())
                                }
                            })
                            .collect();
                        media_sizes.extend(sizes);
                    }
                }
            }
        }

        let mut description = String::new();
        let mut location = String::new();
        let mut make_and_model = String::new();

        let lpstat_p_output = Command::new("lpstat")
            .arg("-l")
            .arg("-p")
            .arg(&name)
            .output();

        if let Ok(output) = lpstat_p_output {
            let printer_info = String::from_utf8_lossy(&output.stdout);

            for line in printer_info.lines() {
                if line.contains("Description:") {
                    description = line
                        .split("Description:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                } else if line.contains("Location:") {
                    location = line
                        .split("Location:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                } else if line.contains("Make and Model:") {
                    make_and_model = line
                        .split("Make and Model:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
            }
        }

        if description.is_empty() && make_and_model.is_empty() {
            let lpinfo_output = Command::new("lpinfo").arg("-m").output();

            if let Ok(output) = lpinfo_output {
                let lpinfo_str = String::from_utf8_lossy(&output.stdout);

                for line in lpinfo_str.lines() {
                    if line.contains(&name) {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() > 1 {
                            make_and_model = parts[1..].join(" ");
                            break;
                        }
                    }
                }
            }
        }

        // Get printer_id from saved printers if available
        let printer_id = saved_printers.get(&name).and_then(|p| p.printer_id);

        printers.push(Printer {
            name,
            description,
            location,
            make_and_model,
            media_sizes,
            printer_id, // Include the printer_id field
        });
    }

    printers
}

/// Check for new printers and update the stored printers
pub async fn check_for_new_printers(
    printers_data: web::Data<Arc<Mutex<HashSet<String>>>>,
    http_client: web::Data<Client>,
    config: web::Data<Arc<Mutex<Config>>>,
) -> Result<Vec<Printer>, Box<dyn std::error::Error>> {
    let current_printers = get_all_printers().await;

    // Load the saved printers
    let mut saved_printers = load_printers();

    // Convert current printers list to hashmap
    let mut current_printers_map: HashMap<String, Printer> = HashMap::new();
    for printer in current_printers {
        // If printer exists in saved_printers, preserve its printer_id
        if let Some(saved_printer) = saved_printers.get(&printer.name) {
            let mut updated_printer = printer.clone();
            updated_printer.printer_id = saved_printer.printer_id;
            current_printers_map.insert(printer.name.clone(), updated_printer);
        } else {
            current_printers_map.insert(printer.name.clone(), printer);
        }
    }

    // Get the required configuration
    let config_clone = {
        let guard = config.lock().unwrap();
        guard.clone()
    };

    // Sync with API
    let sync_result = sync_printers_with_api(
        &current_printers_map,
        &http_client,
        &config_clone
    ).await;

    let synced_printers = match sync_result {
        Ok(printers) => printers,
        Err(e) => {
            eprintln!("Error syncing printers with API: {}", e);
            current_printers_map
        }
    };

    // Save the updated printers
    save_printers(&synced_printers);

    // Update the printers_data set with current printer names
    {
        let mut printers_set = printers_data.lock().unwrap();
        printers_set.clear();
        for printer in synced_printers.keys() {
            printers_set.insert(printer.clone());
        }
    }

    // Return new printers (those not in the old saved_printers)
    let new_printers: Vec<Printer> = synced_printers
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
) {
    let printers_data = web::Data::new(printers_data);
    let config_data = web::Data::new(config);
    let client_data = web::Data::new(http_client);

    loop {
        let interval = { config_data.lock().unwrap().printer_check_interval };

        match check_for_new_printers(
            printers_data.clone(),
            client_data.clone(),
            config_data.clone(),
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

        time::sleep(Duration::from_secs(interval * 60)).await;
    }
}