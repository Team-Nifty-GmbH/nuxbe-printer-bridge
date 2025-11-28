use printers::common::base::job::PrinterJobOptions;
use printers::{get_printer_by_name, get_printers};
use reqwest::Client;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time;
use tracing::{debug, error, info};

use crate::models::{Config, PrintJob, PrintJobResponse, WebsocketPrintJob};
use crate::utils::http::with_auth_header;

/// Process a print job received through WebSocket
pub async fn handle_print_job(
    print_job: WebsocketPrintJob,
    http_client: &Client,
    config: &mut Config,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        printer = %print_job.printer_name,
        spooler = %print_job.spooler_name,
        media_id = %print_job.media_id,
        "Processing WebSocket print job"
    );

    if print_job.spooler_name != config.instance_name {
        debug!(
            requested_spooler = %print_job.spooler_name,
            our_spooler = %config.instance_name,
            "Ignoring job for different printer server"
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
                    info!(
                        media_id = %print_job.media_id,
                        printer = %print_job.printer_name,
                        cups_job_id = job_id,
                        "Successfully printed document"
                    );

                    if let Some(job_id) = print_job.job_id {
                        match update_print_job_status(job_id, true, http_client, config).await {
                            Ok(_) => info!(job_id, "Updated print job status to completed"),
                            Err(e) => {
                                error!(job_id, error = %e, "Failed to update print job status")
                            }
                        }
                    }

                    Ok(())
                }
                Err(e) => {
                    let error_msg = format!(
                        "Failed to print media ID {} on printer {}: {}",
                        print_job.media_id, print_job.printer_name, e
                    );
                    error!("{}", error_msg);
                    Err(error_msg.into())
                }
            }
        }
        None => {
            let error_msg = format!(
                "Printer '{}' not found for media ID {}",
                print_job.printer_name, print_job.media_id
            );
            error!("{}", error_msg);
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
    // Include printer relationship to get printer details and filter by is_completed
    // Note: We fetch all incomplete jobs and filter by printer locally since the printer
    // might be matched by spooler_name (CUPS printer name)
    let jobs_url = format!(
        "{}/api/print-jobs?filter[is_completed]=false&include=printer",
        config.flux_url
    );

    debug!(url = %jobs_url, "Fetching print jobs");

    let response = with_auth_header(http_client.get(&jobs_url), config)
        .header("Accept", "application/json")
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
            error!(error = %e, "JSON parse error");
            debug!(error = %e, "Error location");

            // Try to provide more context about where the error occurred
            if let Some(line_col) = e.to_string().find("at line") {
                let error_context = &e.to_string()[line_col..];
                debug!(context = %error_context, "Error context");

                // Try to show the part of JSON where error occurred
                if error_context.contains("line")
                    && error_context.contains("column")
                    && let Ok(err_line) = error_context
                        .split_whitespace()
                        .nth(2)
                        .unwrap_or("0")
                        .parse::<usize>()
                    && let Ok(err_col) = error_context
                        .split_whitespace()
                        .nth(5)
                        .unwrap_or("0")
                        .parse::<usize>()
                {
                    let lines: Vec<&str> = response_text.lines().collect();
                    if err_line <= lines.len() {
                        let line = lines[err_line - 1];
                        debug!(line, "Problematic line");
                        if err_col <= line.len() {
                            let marker = " ".repeat(err_col - 1) + "^";
                            debug!(position = %marker, "Error position");
                        }
                    }
                }
            }

            return Err(format!("Failed to parse API response: {}", e).into());
        }
    };

    let jobs = parsed_response.data.data;

    if !jobs.is_empty() {
        info!(job_count = jobs.len(), "Processing print jobs");

        for job in &jobs {
            // Get the printer's spooler_name (CUPS printer name) from included printer data
            let printer_name = if let Some(ref printer) = job.printer {
                debug!(
                    job_id = job.id,
                    printer_name = %printer.name,
                    spooler_name = %printer.spooler_name,
                    "Processing print job with included printer"
                );
                printer.spooler_name.clone()
            } else {
                debug!(
                    job_id = job.id,
                    printer_id = ?job.printer_id,
                    "Processing print job (no included printer, looking up by ID)"
                );
                get_printer_name_by_id(job.printer_id).await
            };

            let file_url = format!("{}/api/media/private/{}", config.flux_url, job.media_id);
            let file_response = with_auth_header(http_client.get(&file_url), config)
                .header("Accept", "application/octet-stream")
                .send()
                .await?;

            if !file_response.status().is_success() {
                error!(
                    job_id = job.id,
                    status = %file_response.status(),
                    "Failed to download file for job"
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
                            info!(
                                job_id = job.id,
                                cups_job_id = print_job_id,
                                "Successfully printed job"
                            );

                            match update_print_job_status(job.id, true, http_client, config).await {
                                Ok(_) => {
                                    info!(job_id = job.id, "Updated print job status to completed")
                                }
                                Err(e) => {
                                    error!(job_id = job.id, error = %e, "Failed to update print job status")
                                }
                            }
                        }
                        Err(e) => {
                            error!(job_id = job.id, error = %e, "Failed to print job");
                        }
                    }
                }
                None => {
                    error!(
                        printer = %printer_name,
                        job_id = job.id,
                        "Printer not found for job"
                    );
                }
            }
        }
    } else {
        debug!("No print jobs found for this instance");
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
        if let Some(id) = printer.printer_id
            && id == printer_id
        {
            return name;
        }
    }

    get_default_printer_name().await
}

/// Background task to periodically check for print jobs
pub async fn job_checker_task(config: Arc<Mutex<Config>>, http_client: Client) {
    loop {
        let reverb_enabled = {
            let guard = config.lock().unwrap();
            !guard.reverb_disabled
        };

        if reverb_enabled {
            info!("Polling is disabled. Using Reverb WebSockets instead");
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
                    info!(job_count = jobs.len(), "Processed print jobs");
                }

                if let Ok(mut guard) = config.lock() {
                    guard.flux_api_token = config_clone.flux_api_token;
                }
            }
            Err(e) => error!(error = %e, "Error fetching print jobs"),
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
