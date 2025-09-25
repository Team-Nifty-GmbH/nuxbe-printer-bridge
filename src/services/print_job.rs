use printers::common::base::job::PrinterJobOptions;
use printers::{get_printer_by_name, get_printers};
use reqwest::Client;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time;

use crate::models::{Config, PrintJob, PrintJobResponse, WebsocketPrintJob};
use crate::utils::http::with_auth_header;

/// Process a print job received through WebSocket
pub async fn handle_print_job(
    print_job: WebsocketPrintJob,
    http_client: &Client,
    config: &mut Config,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Processing WebSocket print job for printer: {} from server: {} with media id: {}",
        print_job.printer_name, print_job.spooler_name, print_job.media_id
    );

    if print_job.spooler_name != config.instance_name {
        println!(
            "Ignoring job for different printer server: {} (we are: {})",
            print_job.spooler_name, config.instance_name
        );
        return Ok(());
    }

    let file_url = format!(
        "{}/api/media/private/{}",
        config.flux_url, print_job.media_id
    );

    let file_response = with_auth_header(http_client.get(&file_url), config)
        .header("Accept", "application/octet-stream")
        .send()
        .await?;

    if !file_response.status().is_success() {
        return Err(format!(
            "Failed to download file for media ID {}: {}",
            print_job.media_id,
            file_response.status()
        )
        .into());
    }

    let file_content = file_response.bytes().await?;

    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&file_content)?;
    let temp_path = temp_file.path().to_str().unwrap();
    match get_printer_by_name(&print_job.printer_name) {
        Some(printer) => {
            let job_options = PrinterJobOptions {
                name: Some(&format!("Media ID {}", print_job.media_id)),
                ..PrinterJobOptions::none()
            };

            match printer.print_file(temp_path, job_options) {
                Ok(job_id) => {
                    println!(
                        "Successfully printed media ID {} on printer {} (Job ID: {})",
                        print_job.media_id, print_job.printer_name, job_id
                    );

                    if let Some(job_id) = print_job.job_id {
                        match update_print_job_status(job_id, true, http_client, config).await {
                            Ok(_) => println!("Updated print job {} status to completed", job_id),
                            Err(e) => eprintln!("Failed to update print job status: {}", e),
                        }
                    }

                    Ok(())
                }
                Err(e) => {
                    let error_msg = format!(
                        "Failed to print media ID {} on printer {}: {}",
                        print_job.media_id, print_job.printer_name, e
                    );
                    eprintln!("{}", error_msg);
                    Err(error_msg.into())
                }
            }
        }
        None => {
            let error_msg = format!(
                "Printer '{}' not found for media ID {}",
                print_job.printer_name, print_job.media_id
            );
            eprintln!("{}", error_msg);
            Err(error_msg.into())
        }
    }
}

async fn update_print_job_status(
    job_id: u32,
    is_completed: bool,
    http_client: &Client,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/api/print-jobs", config.flux_url);

    let response = with_auth_header(http_client.put(&url), config)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "id": job_id,
            "is_completed": is_completed,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status(); // Save the status before consuming the response
        let error_text = response.text().await?;
        return Err(format!(
            "Failed to update print job status: {} - {}",
            status, error_text
        )
        .into());
    }

    Ok(())
}

/// Fetch print jobs from the API and process them
pub async fn fetch_print_jobs(
    http_client: &Client,
    config: &mut Config,
) -> Result<Vec<PrintJob>, Box<dyn std::error::Error>> {
    let jobs_url = format!(
        "{}/api/print-jobs?filter[printer.spooler_name]={}&filter[is_completed]=false",
        config.flux_url, config.instance_name
    );

    let response = with_auth_header(http_client.get(&jobs_url), config)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "spooler_name": config.instance_name,
            "is_completed": false
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch print jobs: {}", response.status()).into());
    }

    let response_text = response.text().await?;

    // Show full API response for debugging purposes
    // println!("Full API response: {}", response_text);

    // Try to parse as generic JSON first to see actual structure
    // match serde_json::from_str::<serde_json::Value>(&response_text) {
    //     Ok(json_value) => {
    //         println!("Successfully parsed as generic JSON: {}",
    //                  if json_value.is_object() { "object" }
    //                  else if json_value.is_array() { "array" }
    //                  else { "other" }
    //         );
    //     },
    //     Err(e) => {
    //         println!("Warning: Could not parse as generic JSON: {}", e);
    //     }
    // }

    let parsed_response: PrintJobResponse = match serde_json::from_str(&response_text) {
        Ok(parsed) => parsed,
        Err(e) => {
            println!("JSON parse error: {}", e);
            println!("Error location: {}", e.to_string());

            // Try to provide more context about where the error occurred
            if let Some(line_col) = e.to_string().find("at line") {
                let error_context = &e.to_string()[line_col..];
                println!("Error context: {}", error_context);

                // Try to show the part of JSON where error occurred
                if error_context.contains("line") && error_context.contains("column") {
                    if let Ok(err_line) = error_context
                        .split_whitespace()
                        .nth(2)
                        .unwrap_or("0")
                        .parse::<usize>()
                    {
                        if let Ok(err_col) = error_context
                            .split_whitespace()
                            .nth(5)
                            .unwrap_or("0")
                            .parse::<usize>()
                        {
                            let lines: Vec<&str> = response_text.lines().collect();
                            if err_line <= lines.len() {
                                let line = lines[err_line - 1];
                                println!("Problematic line: {}", line);
                                if err_col <= line.len() {
                                    let marker = " ".repeat(err_col - 1) + "^";
                                    println!("Position: {}", marker);
                                }
                            }
                        }
                    }
                }
            }

            return Err(format!("Failed to parse API response: {}", e).into());
        }
    };

    let jobs = parsed_response.data.data;

    if !jobs.is_empty() {
        println!("Processing {} print job(s)", jobs.len());

        for job in &jobs {
            println!(
                "Processing print job {} for printer ID {:?}",
                job.id, job.printer_id
            );

            let printer_name = get_printer_name_by_id(job.printer_id).await;

            let file_url = format!("{}/api/media/private/{}", config.flux_url, job.media_id);
            let file_response = with_auth_header(http_client.get(&file_url), config)
                .header("Accept", "application/octet-stream")
                .send()
                .await?;

            if !file_response.status().is_success() {
                eprintln!(
                    "Failed to download file for job {}: {}",
                    job.id,
                    file_response.status()
                );
                continue;
            }

            let file_content = file_response.bytes().await?;

            let mut temp_file = NamedTempFile::new()?;
            temp_file.write_all(&file_content)?;
            let temp_path = temp_file.path().to_str().unwrap();
            match get_printer_by_name(&printer_name) {
                Some(printer) => {
                    let job_options = PrinterJobOptions {
                        name: Some(&format!("Print Job {}", job.id)),
                        ..PrinterJobOptions::none()
                    };

                    match printer.print_file(temp_path, job_options) {
                        Ok(print_job_id) => {
                            println!(
                                "Successfully printed job {} (Print Job ID: {})",
                                job.id, print_job_id
                            );

                            match update_print_job_status(job.id, true, http_client, config).await {
                                Ok(_) => {
                                    println!("Updated print job {} status to completed", job.id)
                                }
                                Err(e) => eprintln!("Failed to update print job status: {}", e),
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to print job {}: {}", job.id, e);
                        }
                    }
                }
                None => {
                    eprintln!("Printer '{}' not found for job {}", printer_name, job.id);
                }
            }
        }
    } else {
        println!("No print jobs found for this instance.");
    }
    Ok(jobs)
}

async fn get_printer_name_by_id(printer_id: Option<u32>) -> String {
    if printer_id.is_none() {
        return get_default_printer_name().await;
    }

    let printer_id = printer_id.unwrap();
    let saved_printers = crate::utils::printer_storage::load_printers();
    for (name, printer) in saved_printers {
        if let Some(id) = printer.printer_id {
            if id == printer_id {
                return name;
            }
        }
    }

    get_default_printer_name().await
}

/// Background task to periodically check for print jobs
pub async fn job_checker_task(config: Arc<Mutex<Config>>, http_client: Client) {
    loop {
        let disabled = {
            let guard = config.lock().unwrap();
            !guard.reverb_disabled
        };

        if disabled {
            println!("Polling is disabled. Using Reverb WebSockets instead.");
            return;
        }

        let interval;
        let mut config_clone;

        {
            let guard = config.lock().unwrap();
            interval = guard.job_check_interval;
            config_clone = guard.clone();
        }

        match fetch_print_jobs(&http_client, &mut config_clone).await {
            Ok(jobs) => {
                if !jobs.is_empty() {
                    println!("Processed {} print job(s)", jobs.len());
                }

                if let Ok(mut guard) = config.lock() {
                    guard.flux_api_token = config_clone.flux_api_token;
                }
            }
            Err(e) => eprintln!("Error fetching print jobs: {}", e),
        }

        time::sleep(Duration::from_secs(interval * 60)).await;
    }
}

async fn get_default_printer_name() -> String {
    let system_printers = get_printers();
    if !system_printers.is_empty() {
        return system_printers[0].name.clone();
    }

    "default".to_string()
}
