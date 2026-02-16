use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use printers::common::base::job::PrinterJobOptions;
use printers::{get_printer_by_name, get_printers};
use reqwest::Client;
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::error::SpoolerResult;
use crate::models::{Config, PrintJob, PrintJobResponse, PrintJobStatus};
use crate::utils::config::read_config;
use crate::utils::http::with_auth_header;

/// A print job that has been submitted to CUPS and is awaiting final status.
#[derive(Debug, Clone)]
pub struct InFlightJob {
    pub api_job_id: u32,
    pub cups_job_id: u64,
    pub printer_name: String,
    pub submitted_at: Instant,
    /// Last known status sent to the API (to avoid redundant updates)
    pub last_status: PrintJobStatus,
}

/// Shared in-flight job tracker accessible from multiple tasks.
pub type InFlightJobs = Arc<Mutex<Vec<InFlightJob>>>;

/// Create a new empty in-flight jobs tracker.
pub fn new_in_flight_jobs() -> InFlightJobs {
    Arc::new(Mutex::new(Vec::new()))
}

/// Maximum time (seconds) to wait for a CUPS job before marking it as failed.
const CUPS_JOB_TIMEOUT_SECS: u64 = 300; // 5 minutes

// ── API helpers ─────────────────────────────────────────────────────────────

/// Update print job status in the API with full status tracking fields.
async fn update_print_job_status(
    job_id: u32,
    cups_job_id: Option<u64>,
    status: PrintJobStatus,
    error_message: Option<&str>,
    http_client: &Client,
    config: &Config,
) -> SpoolerResult<()> {
    let url = format!("{}/api/print-jobs", config.flux_url);

    let mut payload = serde_json::json!({
        "id": job_id,
        "is_completed": status.is_terminal(),
        "status": status,
    });

    if let Some(cups_id) = cups_job_id {
        payload["cups_job_id"] = serde_json::json!(cups_id);
    }

    if let Some(msg) = error_message {
        payload["error_message"] = serde_json::json!(msg);
    }

    if status == PrintJobStatus::Completed {
        payload["printed_at"] = serde_json::json!(chrono_now_utc());
    }

    let response = with_auth_header(http_client.put(&url), config)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status_code = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Failed to update print job status: {} - {}",
            status_code, error_text
        )
        .into());
    }

    Ok(())
}

/// Return the current UTC time as an ISO 8601 string for the `printed_at` field.
fn chrono_now_utc() -> String {
    // Format: YYYY-MM-DD HH:MM:SS (Laravel-compatible)
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Simple UTC formatting without pulling in the chrono crate
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to Y-M-D (simplified Gregorian)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i;
            break;
        }
        remaining_days -= md;
    }

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds
    )
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

/// Fetch in-flight jobs from the API (jobs with status queued/processing that have a cups_job_id).
/// Used on startup to re-populate the in-flight tracker.
pub async fn fetch_in_flight_jobs_from_api(
    http_client: &Client,
    config: &Config,
) -> Result<Vec<PrintJob>, String> {
    // Fetch jobs that are not completed — we'll filter for queued/processing client-side
    let jobs_url = format!(
        "{}/api/print-jobs?filter[is_completed]=false&include=printer",
        config.flux_url
    );

    debug!(url = %jobs_url, "Fetching in-flight jobs from API for status recovery");

    let response = match with_auth_header(http_client.get(&jobs_url), config)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(format!("Failed to fetch in-flight jobs: {}", e)),
    };

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch in-flight jobs: {}",
            response.status()
        ));
    }

    let response_text = match response.text().await {
        Ok(t) => t,
        Err(e) => return Err(format!("Failed to read response: {}", e)),
    };

    let parsed_response: PrintJobResponse = match serde_json::from_str(&response_text) {
        Ok(parsed) => parsed,
        Err(e) => return Err(format!("Failed to parse in-flight jobs: {}", e)),
    };

    // Filter to only jobs that have a cups_job_id and a queued/processing status
    let in_flight: Vec<PrintJob> = parsed_response
        .data
        .data
        .into_iter()
        .filter(|job| {
            job.cups_job_id.is_some()
                && matches!(
                    job.status,
                    Some(PrintJobStatus::Queued) | Some(PrintJobStatus::Processing)
                )
        })
        .collect();

    Ok(in_flight)
}

// ── Printer helpers ─────────────────────────────────────────────────────────

/// Get the default system printer system_name
fn get_default_printer_system_name() -> String {
    let system_printers = get_printers();
    if let Some(first) = system_printers.first() {
        return first.system_name.clone();
    }
    "default".to_string()
}

/// Resolve printer system_name from job data for stable CUPS addressing
async fn resolve_printer_name(job: &PrintJob) -> String {
    // Try to resolve via saved printers by ID for stable system_name
    let printer_id = job.printer.as_ref().map(|p| p.id).or(job.printer_id);
    if let Some(id) = printer_id {
        let saved_printers = crate::utils::printer_storage::load_printers();
        for (system_name, printer) in &saved_printers {
            if printer.printer_id == Some(id) {
                debug!(
                    job_id = job.id,
                    printer_name = %printer.name,
                    system_name = %system_name,
                    "Resolved printer system_name"
                );
                return system_name.clone();
            }
        }
    }

    // Fallback: use name from job data (get_printer_by_name matches both name and system_name)
    if let Some(ref printer) = job.printer {
        debug!(
            job_id = job.id,
            printer_name = %printer.name,
            "Falling back to printer name from job data"
        );
        printer.name.clone()
    } else {
        debug!(job_id = job.id, "Using default printer");
        get_default_printer_system_name()
    }
}

// ── Core print workflow ─────────────────────────────────────────────────────

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

/// Download, print, and track job status — core print workflow.
///
/// Instead of immediately marking the job as completed, this submits to CUPS
/// and registers the job as in-flight so the status checker can track it.
async fn process_print_job(
    job: &PrintJob,
    http_client: &Client,
    config: &Config,
    in_flight_jobs: &InFlightJobs,
) -> SpoolerResult<()> {
    let printer_name = resolve_printer_name(job).await;

    // Download file
    let temp_file = download_file(http_client, config, job.media_id).await?;

    // Get printer with fallback
    let printer = match get_printer_by_name(&printer_name) {
        Some(p) => p,
        None => {
            let mut printers = get_printers();
            let default_printer = printers.pop().ok_or("No printers available")?;
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
    let temp_path = temp_file.path().to_str().ok_or("Invalid temp file path")?;

    let job_options = PrinterJobOptions {
        name: Some(&format!("Print Job {}", job.id)),
        ..PrinterJobOptions::none()
    };

    let cups_job_id = printer
        .print_file(temp_path, job_options)
        .map_err(|e| format!("Failed to print: {:?}", e))?;

    info!(
        job_id = job.id,
        cups_job_id,
        printer = %printer.name,
        "Print job submitted to CUPS"
    );

    // Update API: mark as queued with cups_job_id
    match update_print_job_status(
        job.id,
        Some(cups_job_id),
        PrintJobStatus::Queued,
        None,
        http_client,
        config,
    )
    .await
    {
        Ok(_) => info!(job_id = job.id, cups_job_id, "Status updated to queued"),
        Err(e) => warn!(job_id = job.id, error = %e, "Failed to update job status to queued"),
    }

    // Register as in-flight for the status checker to track
    let in_flight = InFlightJob {
        api_job_id: job.id,
        cups_job_id,
        printer_name: printer.system_name.clone(),
        submitted_at: Instant::now(),
        last_status: PrintJobStatus::Queued,
    };

    in_flight_jobs
        .lock()
        .expect("Failed to acquire in_flight_jobs lock")
        .push(in_flight);

    Ok(())
}

/// Fetch print jobs from the API and process them
pub async fn fetch_print_jobs(
    http_client: &Client,
    config: &mut Config,
    in_flight_jobs: &InFlightJobs,
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

    let parsed_response: PrintJobResponse = serde_json::from_str(&response_text).map_err(|e| {
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
        // Skip jobs that are already in-flight (have a cups_job_id and queued/processing status)
        if job.cups_job_id.is_some()
            && matches!(
                job.status,
                Some(PrintJobStatus::Queued) | Some(PrintJobStatus::Processing)
            )
        {
            debug!(job_id = job.id, "Skipping in-flight job");
            continue;
        }

        if let Err(e) = process_print_job(job, http_client, config, in_flight_jobs).await {
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
    in_flight_jobs: &InFlightJobs,
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

    process_print_job(&job, http_client, config, in_flight_jobs).await
}

// ── Background tasks ────────────────────────────────────────────────────────

/// Background task to periodically check for print jobs (polling mode)
pub async fn job_checker_task(
    config: Arc<RwLock<Config>>,
    http_client: Client,
    cancel_token: CancellationToken,
    in_flight_jobs: InFlightJobs,
) {
    loop {
        let mut config_clone = read_config(&config);

        if !config_clone.reverb_disabled {
            info!("Polling is disabled. Using Reverb WebSockets instead");
            return;
        }

        let interval = config_clone.job_check_interval;

        match fetch_print_jobs(&http_client, &mut config_clone, &in_flight_jobs).await {
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

/// Background task that polls CUPS for the final status of in-flight print jobs.
///
/// Runs every 15 seconds and checks each in-flight job against CUPS job history.
/// When a job reaches a terminal state (completed, cancelled, aborted) or times out,
/// the API is updated and the job is removed from the in-flight tracker.
pub async fn job_status_checker_task(
    config: Arc<RwLock<Config>>,
    http_client: Client,
    cancel_token: CancellationToken,
    in_flight_jobs: InFlightJobs,
) {
    // Re-populate in-flight jobs from the API on startup
    {
        let config_snapshot = read_config(&config);
        match fetch_in_flight_jobs_from_api(&http_client, &config_snapshot).await {
            Ok(api_jobs) => {
                if !api_jobs.is_empty() {
                    let mut tracker = in_flight_jobs
                        .lock()
                        .expect("Failed to acquire in_flight_jobs lock");
                    for job in &api_jobs {
                        let Some(cups_id) = job.cups_job_id else {
                            continue;
                        };

                        // Resolve printer name for this job
                        let printer_name = if let Some(ref p) = job.printer {
                            p.name.clone()
                        } else {
                            get_default_printer_system_name()
                        };

                        // Only add if not already tracked
                        let already_tracked = tracker.iter().any(|j| j.api_job_id == job.id);
                        if !already_tracked {
                            tracker.push(InFlightJob {
                                api_job_id: job.id,
                                cups_job_id: cups_id as u64,
                                printer_name,
                                submitted_at: Instant::now(),
                                last_status: job.status.clone().unwrap_or(PrintJobStatus::Queued),
                            });
                        }
                    }
                    info!(
                        recovered = api_jobs.len(),
                        "Recovered in-flight jobs from API"
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to recover in-flight jobs from API");
            }
        }
    }

    let check_interval = Duration::from_secs(15);

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Job status checker task shutting down");
                return;
            }
            _ = time::sleep(check_interval) => {}
        }

        // Take a snapshot of in-flight jobs to avoid holding the lock during async work
        let snapshot: Vec<InFlightJob> = {
            let tracker = in_flight_jobs
                .lock()
                .expect("Failed to acquire in_flight_jobs lock");
            if tracker.is_empty() {
                continue;
            }
            tracker.clone()
        };

        trace!(
            count = snapshot.len(),
            "Checking CUPS status for in-flight jobs"
        );

        let config_snapshot = read_config(&config);
        let mut completed_ids: Vec<u32> = Vec::new();

        for job in &snapshot {
            let printer_name = job.printer_name.clone();
            let cups_job_id = job.cups_job_id;

            // Query CUPS in a blocking task (CUPS FFI is not async-safe)
            let cups_state = tokio::task::spawn_blocking(move || {
                let printer = match get_printer_by_name(&printer_name) {
                    Some(p) => p,
                    None => return None,
                };

                // Check active jobs first, then history
                let active = printer.get_active_jobs();
                if let Some(cups_job) = active.iter().find(|j| j.id == cups_job_id) {
                    return Some(cups_job.state.clone());
                }

                let history = printer.get_job_history();
                history
                    .iter()
                    .find(|j| j.id == cups_job_id)
                    .map(|j| j.state.clone())
            })
            .await;

            let cups_state = match cups_state {
                Ok(state) => state,
                Err(e) => {
                    error!(
                        job_id = job.api_job_id,
                        error = %e,
                        "Failed to query CUPS job status"
                    );
                    continue;
                }
            };

            match cups_state {
                Some(cups_state) => {
                    let new_status = PrintJobStatus::from(cups_state);

                    // Skip if status hasn't changed
                    if new_status == job.last_status {
                        trace!(
                            job_id = job.api_job_id,
                            cups_job_id = job.cups_job_id,
                            status = %new_status,
                            "CUPS job status unchanged"
                        );
                        continue;
                    }

                    let error_msg = if new_status == PrintJobStatus::Cancelled {
                        Some("Job cancelled or aborted by CUPS")
                    } else {
                        None
                    };

                    info!(
                        job_id = job.api_job_id,
                        cups_job_id = job.cups_job_id,
                        status = %new_status,
                        "CUPS job status changed"
                    );

                    match update_print_job_status(
                        job.api_job_id,
                        None,
                        new_status.clone(),
                        error_msg,
                        &http_client,
                        &config_snapshot,
                    )
                    .await
                    {
                        Ok(_) => {
                            info!(
                                job_id = job.api_job_id,
                                status = %new_status,
                                "Status updated in API"
                            );
                        }
                        Err(e) => {
                            error!(
                                job_id = job.api_job_id,
                                status = %new_status,
                                error = %e,
                                "Failed to update status in API"
                            );
                        }
                    }

                    if new_status.is_terminal() {
                        completed_ids.push(job.api_job_id);
                    } else {
                        // Update last_status in the tracker for non-terminal transitions
                        let mut tracker = in_flight_jobs
                            .lock()
                            .expect("Failed to acquire in_flight_jobs lock");
                        if let Some(tracked) =
                            tracker.iter_mut().find(|j| j.api_job_id == job.api_job_id)
                        {
                            tracked.last_status = new_status;
                        }
                    }
                }
                None => {
                    // Job not found in CUPS — check if it timed out
                    let elapsed = job.submitted_at.elapsed().as_secs();
                    if elapsed > CUPS_JOB_TIMEOUT_SECS {
                        warn!(
                            job_id = job.api_job_id,
                            cups_job_id = job.cups_job_id,
                            elapsed_secs = elapsed,
                            "CUPS job disappeared from queue after timeout"
                        );
                        match update_print_job_status(
                            job.api_job_id,
                            None,
                            PrintJobStatus::Failed,
                            Some("Job disappeared from CUPS queue"),
                            &http_client,
                            &config_snapshot,
                        )
                        .await
                        {
                            Ok(_) => {
                                info!(
                                    job_id = job.api_job_id,
                                    "Status updated to failed (timeout)"
                                );
                            }
                            Err(e) => {
                                error!(
                                    job_id = job.api_job_id,
                                    error = %e,
                                    "Failed to update timeout status"
                                );
                            }
                        }
                        completed_ids.push(job.api_job_id);
                    } else {
                        trace!(
                            job_id = job.api_job_id,
                            cups_job_id = job.cups_job_id,
                            elapsed_secs = elapsed,
                            "CUPS job not found yet, still within timeout"
                        );
                    }
                }
            }
        }

        // Remove completed/failed jobs from the in-flight tracker
        if !completed_ids.is_empty() {
            let mut tracker = in_flight_jobs
                .lock()
                .expect("Failed to acquire in_flight_jobs lock");
            tracker.retain(|j| !completed_ids.contains(&j.api_job_id));
            debug!(
                removed = completed_ids.len(),
                remaining = tracker.len(),
                "Cleaned up in-flight job tracker"
            );
        }
    }
}
