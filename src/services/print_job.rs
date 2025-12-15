use printers::common::base::job::PrinterJobOptions;
use printers::{get_printer_by_name, get_printers};
use reqwest::Client;
use std::io::Write;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::error::SpoolerResult;
use crate::models::{Config, PrintJob, PrintJobResponse};
use crate::utils::http::with_auth_header;

/// Update print job status in the API
async fn update_print_job_status(
    job_id: u32,
    is_completed: bool,
    http_client: &Client,
    config: &Config,
) -> SpoolerResult<()> {
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
        let status = response.status();
        let error_text = response.text().await?;
        return Err(format!(
            "Failed to update print job status: {} - {}",
            status, error_text
        )
        .into());
    }

    Ok(())
}

/// Fetch pending print job IDs from the API (Send-safe version for tokio::spawn)
pub async fn fetch_pending_job_ids(
    http_client: &Client,
    config: &Config,
) -> Result<Vec<u32>, String> {
    let jobs_url = format!(
        "{}/api/print-jobs?filter[is_completed]=false&include=printer",
        config.flux_url
    );

    debug!(url = %jobs_url, "Fetching pending print job IDs");

    let response = match with_auth_header(http_client.get(&jobs_url), config)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(format!("Failed to fetch print jobs: {}", e)),
    };

    if !response.status().is_success() {
        return Err(format!("Failed to fetch print jobs: {}", response.status()));
    }

    let response_text = match response.text().await {
        Ok(t) => t,
        Err(e) => return Err(format!("Failed to read response: {}", e)),
    };

    let parsed_response: PrintJobResponse = match serde_json::from_str(&response_text) {
        Ok(parsed) => parsed,
        Err(e) => return Err(format!("Failed to parse print jobs: {}", e)),
    };

    Ok(parsed_response.data.data.iter().map(|job| job.id).collect())
}

/// Get printer name by ID from saved printers, with fallback to default
async fn get_printer_name_by_id(printer_id: Option<u32>) -> String {
    let Some(id) = printer_id else {
        return get_default_printer_name();
    };

    let saved_printers = crate::utils::printer_storage::load_printers();
    for (name, printer) in saved_printers {
        if printer.printer_id == Some(id) {
            return name;
        }
    }

    get_default_printer_name()
}

/// Get the default system printer name
fn get_default_printer_name() -> String {
    let system_printers = get_printers();
    if let Some(first) = system_printers.first() {
        return first.name.clone();
    }
    "default".to_string()
}

/// Resolve printer name from job data
async fn resolve_printer_name(job: &PrintJob) -> String {
    if let Some(ref printer) = job.printer {
        debug!(
            job_id = job.id,
            printer_name = %printer.name,
            spooler_name = %printer.spooler_name,
            "Using printer from job data"
        );
        printer.name.clone()
    } else {
        debug!(
            job_id = job.id,
            printer_id = ?job.printer_id,
            "Looking up printer by ID"
        );
        get_printer_name_by_id(job.printer_id).await
    }
}


/// Download file from API and save to temp file
async fn download_file(
    http_client: &Client,
    config: &Config,
    media_id: u32,
) -> SpoolerResult<NamedTempFile> {
    let file_url = format!("{}/api/media/private/{}", config.flux_url, media_id);
    debug!(media_id, "Downloading file");

    let file_response = with_auth_header(http_client.get(&file_url), config)
        .header("Accept", "application/octet-stream")
        .send()
        .await?;

    if !file_response.status().is_success() {
        return Err(format!(
            "Failed to download file for media ID {}: {}",
            media_id,
            file_response.status()
        )
        .into());
    }

    let file_content = file_response.bytes().await?;

    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&file_content)?;

    Ok(temp_file)
}

/// Download, print, and update job status - core print workflow
async fn process_print_job(
    job: &PrintJob,
    http_client: &Client,
    config: &Config,
) -> SpoolerResult<()> {
    let printer_name = resolve_printer_name(job).await;

    // Download file
    let temp_file = download_file(http_client, config, job.media_id).await?;

    // Get printer with fallback
    let printer = match get_printer_by_name(&printer_name) {
        Some(p) => p,
        None => {
            let printers = get_printers();
            if printers.is_empty() {
                error!(job_id = job.id, "No printers available");
                return Err("No printers available".into());
            }
            let default_printer = printers.into_iter().next().unwrap();
            warn!(
                job_id = job.id,
                requested_printer = %printer_name,
                fallback_printer = %default_printer.name,
                "Printer not found, using default"
            );
            default_printer
        }
    };

    // Print file
    let temp_path = temp_file
        .path()
        .to_str()
        .ok_or("Invalid temp file path")?;

    let job_options = PrinterJobOptions {
        name: Some(&format!("Print Job {}", job.id)),
        ..PrinterJobOptions::none()
    };

    let cups_job_id = printer
        .print_file(temp_path, job_options)
        .map_err(|e| format!("Failed to print: {}", e))?;

    info!(
        job_id = job.id,
        cups_job_id,
        printer = %printer.name,
        "Print job submitted successfully"
    );

    // Update status
    match update_print_job_status(job.id, true, http_client, config).await {
        Ok(_) => info!(job_id = job.id, "Status updated to completed"),
        Err(e) => warn!(job_id = job.id, error = %e, "Failed to update job status"),
    }

    Ok(())
}

/// Fetch print jobs from the API and process them
pub async fn fetch_print_jobs(
    http_client: &Client,
    config: &mut Config,
) -> SpoolerResult<Vec<PrintJob>> {
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

    let parsed_response: PrintJobResponse = serde_json::from_str(&response_text)
        .map_err(|e| {
            error!(error = %e, "Failed to parse print jobs response");
            format!("Failed to parse API response: {}", e)
        })?;

    let jobs = parsed_response.data.data;

    if jobs.is_empty() {
        debug!("No print jobs found for this instance");
        return Ok(jobs);
    }

    info!(job_count = jobs.len(), "Processing print jobs");

    for job in &jobs {
        if let Err(e) = process_print_job(job, http_client, config).await {
            error!(job_id = job.id, error = %e, "Failed to process print job");
        }
    }

    Ok(jobs)
}

/// Single print job response from API (when fetching by ID)
#[derive(serde::Deserialize, Debug)]
struct SinglePrintJobResponse {
    #[allow(dead_code)]
    status: u16,
    data: PrintJob,
}

/// Fetch a single print job by ID from the API and print it
pub async fn fetch_and_print_job_by_id(
    job_id: u32,
    http_client: &Client,
    config: &Config,
) -> SpoolerResult<()> {
    let job_url = format!(
        "{}/api/print-jobs/{}?include=printer",
        config.flux_url, job_id
    );

    info!(job_id, url = %job_url, "Fetching print job by ID");

    let response = with_auth_header(http_client.get(&job_url), config)
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Failed to fetch print job {}: {} - {}",
            job_id, status, error_text
        )
        .into());
    }

    let response_text = response.text().await?;
    let parsed: SinglePrintJobResponse = serde_json::from_str(&response_text)
        .map_err(|e| format!("Failed to parse job response: {}", e))?;

    let job = parsed.data;

    info!(
        job_id = job.id,
        media_id = job.media_id,
        is_completed = job.is_completed,
        "Fetched print job"
    );

    // Check if job is already completed
    if job.is_completed {
        info!(job_id = job.id, "Job was already printed, skipping");
        return Ok(());
    }

    process_print_job(&job, http_client, config).await
}

/// Background task to periodically check for print jobs
pub async fn job_checker_task(
    config: Arc<RwLock<Config>>,
    http_client: Client,
    cancel_token: CancellationToken,
) {
    loop {
        let reverb_enabled = {
            let guard = config.read().expect("Failed to acquire config read lock");
            !guard.reverb_disabled
        };

        if reverb_enabled {
            info!("Polling is disabled. Using Reverb WebSockets instead");
            return;
        }

        let interval;
        let mut config_clone;

        {
            let guard = config.read().expect("Failed to acquire config read lock");
            interval = guard.job_check_interval;
            config_clone = guard.clone();
        }

        match fetch_print_jobs(&http_client, &mut config_clone).await {
            Ok(jobs) => {
                if !jobs.is_empty() {
                    info!(job_count = jobs.len(), "Processed print jobs");
                }

                if let Ok(mut guard) = config.write() {
                    guard.flux_api_token = config_clone.flux_api_token;
                }
            }
            Err(e) => error!(error = %e, "Error fetching print jobs"),
        }

        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Job checker task shutting down");
                return;
            }
            _ = time::sleep(Duration::from_secs(interval * 60)) => {}
        }
    }
}
