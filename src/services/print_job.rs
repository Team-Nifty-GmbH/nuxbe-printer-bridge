use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use reqwest::{Client, StatusCode};
use tempfile::NamedTempFile;
use tokio::time;

use crate::models::{Config, PrintJob, PrintJobResponse, WebsocketPrintJob};

/// Process a print job received through WebSocket
// Update handle_print_job to use the new format when needed
pub async fn handle_print_job(
    print_job: WebsocketPrintJob,
    http_client: &Client,
    config: &mut Config,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Processing WebSocket print job for printer: {} from server: {} with media id: {}",
        print_job.printer_name, print_job.spooler_name, print_job.media_id
    );

    // Check if this job is for this instance
    if print_job.spooler_name != config.instance_name {
        println!(
            "Ignoring job for different printer server: {} (we are: {})",
            print_job.spooler_name, config.instance_name
        );
        return Ok(());
    }


    // Construct the URL to get the file
    let file_url = format!(
        "{}/api/media/{}/download",
        config.flux_url, print_job.media_id
    );

    // Download the file
    let file_response = http_client
        .get(&file_url)
        .header(
            "Authorization",
            format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&String::new())),
        )
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

    // Create a temporary file
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&file_content)?;
    let temp_path = temp_file.path().to_str().unwrap();

    // Print the file
    let output = Command::new("lp")
        .arg("-d")
        .arg(&print_job.printer_name)
        .arg(temp_path)
        .output()?;

    if output.status.success() {
        println!(
            "Successfully printed media ID {} on printer {}: {}",
            print_job.media_id,
            print_job.printer_name,
            String::from_utf8_lossy(&output.stdout)
        );

        // Update print job status to is_printed = true
        if let Some(job_id) = print_job.job_id {
            match update_print_job_status(job_id, true, http_client, config).await {
                Ok(_) => println!("Updated print job {} status to completed", job_id),
                Err(e) => eprintln!("Failed to update print job status: {}", e),
            }
        }

        Ok(())
    } else {
        let error_msg = format!(
            "Failed to print media ID {} on printer {}: {}",
            print_job.media_id,
            print_job.printer_name,
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("{}", error_msg);
        Err(error_msg.into())
    }
}

// In the update_print_job_status function in src/services/print_job.rs
async fn update_print_job_status(
    job_id: u32,
    is_completed: bool,
    http_client: &Client,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/api/print-jobs/{}", config.flux_url, job_id);

    let response = http_client
        .put(&url)
        .header(
            "Authorization",
            format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&String::new())),
        )
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "is_completed": is_completed,
            "spooler_name": config.instance_name // Changed from instance_name
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status(); // Save the status before consuming the response
        let error_text = response.text().await?;
        return Err(format!("Failed to update print job status: {} - {}", status, error_text).into());
    }

    Ok(())
}

/// Fetch print jobs from the API and process them
// Also in the fetch_print_jobs function, update the JSON payload
pub async fn fetch_print_jobs(
    http_client: &Client,
    config: &mut Config,
) -> Result<Vec<PrintJob>, Box<dyn std::error::Error>> {
    // Construct the URL for fetching print jobs
    let jobs_url = format!("{}/api/print-jobs", config.flux_url);

    let response = http_client
        .get(&jobs_url)
        .header("Authorization", format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&String::new())))
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "spooler_name": config.instance_name, // Changed from instance_name
            "is_completed": false  // Only fetch jobs that haven't been printed yet
        }))
        .send()
        .await?;


    if !response.status().is_success() {
        return Err(format!("Failed to fetch print jobs: {}", response.status()).into());
    }

    let response_text = response.text().await?;
    println!("Response from API: {}", response_text);

    let parsed_response: PrintJobResponse = serde_json::from_str(&response_text)?;
    let jobs = parsed_response.data.data;

    // Process each job
    for job in &jobs {
        println!(
            "Processing print job {} for printer ID {}",
            job.id, job.printer_id
        );

        // Get printer name from printer_id
        let printer_name = get_printer_name_by_id(job.printer_id).await;

        let file_url = format!("{}/api/media/{}/download", config.flux_url, job.media_id);

        // Download the file
        let file_response = http_client
            .get(&file_url)
            .header(
                "Authorization",
                format!("Bearer {}", config.flux_api_token.as_ref().unwrap_or(&String::new())),
            )
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

        // Create a temporary file
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(&file_content)?;
        let temp_path = temp_file.path().to_str().unwrap();

        // Print the file
        let output = Command::new("lp")
            .arg("-d")
            .arg(&printer_name)
            .arg(temp_path)
            .output()?;

        if output.status.success() {
            println!(
                "Successfully printed job {}: {}",
                job.id,
                String::from_utf8_lossy(&output.stdout)
            );

            // Update job status to is_printed = true
            match update_print_job_status(job.id, true, http_client, config).await {
                Ok(_) => println!("Updated print job {} status to completed", job.id),
                Err(e) => eprintln!("Failed to update print job status: {}", e),
            }
        } else {
            eprintln!(
                "Failed to print job {}: {}",
                job.id,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(jobs)
}

/// Helper function to get printer name by ID
async fn get_printer_name_by_id(printer_id: u32) -> String {
    // Look up the printer name from the saved printers
    let saved_printers = crate::utils::printer_storage::load_printers();

    // Find the printer with the matching ID
    for (name, printer) in saved_printers {
        if let Some(id) = printer.printer_id {
            if id == printer_id {
                return name;
            }
        }
    }

    // Fallback if no printer with that ID is found
    format!("printer_{}", printer_id)
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

        // Get interval and create a mutable config clone
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

                // Save any token changes back to the shared config
                if let Ok(mut guard) = config.lock() {
                    guard.flux_api_token = config_clone.flux_api_token;
                }
            }
            Err(e) => eprintln!("Error fetching print jobs: {}", e),
        }

        time::sleep(Duration::from_secs(interval * 60)).await;
    }
}