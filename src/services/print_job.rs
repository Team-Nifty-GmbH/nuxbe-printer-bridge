use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use printers::common::base::job::{PrinterJobOptions, PrinterJobState};
use printers::{get_printer_by_name, get_printers};
use reqwest::Client;
use tempfile::NamedTempFile;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::error::SpoolerResult;
use crate::models::{Config, PrintJob, PrintJobResponse, PrintJobStatus, Printer};
use crate::services::printer::PrinterCache;
use crate::utils::config::read_config;
use crate::utils::http::with_auth_header;
use crate::utils::printer_storage::load_printers;
use crate::utils::sync::lock;

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

/// Tracks jobs between submission and their terminal CUPS state.
#[derive(Debug, Default)]
pub struct InFlightTracker {
    jobs: Vec<InFlightJob>,
    /// Jobs currently being downloaded/submitted, before a CUPS id exists.
    reserved: HashSet<u32>,
}

impl InFlightTracker {
    /// Reserve a job id for processing. Returns false if the job is already
    /// reserved or tracked, so concurrent triggers (poll + WebSocket event)
    /// can never print the same job twice.
    fn reserve(&mut self, api_job_id: u32) -> bool {
        !self.jobs.iter().any(|j| j.api_job_id == api_job_id) && self.reserved.insert(api_job_id)
    }

    /// Promote a reservation to a tracked in-flight job after CUPS accepted it.
    fn confirm(&mut self, job: InFlightJob) {
        self.reserved.remove(&job.api_job_id);
        self.jobs.push(job);
    }

    /// Drop a reservation after a failed submission.
    fn release(&mut self, api_job_id: u32) {
        self.reserved.remove(&api_job_id);
    }

    /// Add a recovered job unless it is already tracked.
    fn track_if_new(&mut self, job: InFlightJob) {
        if !self.jobs.iter().any(|j| j.api_job_id == job.api_job_id) {
            self.jobs.push(job);
        }
    }
}

/// Shared in-flight job tracker accessible from multiple tasks.
pub type InFlightJobs = Arc<Mutex<InFlightTracker>>;

/// Create a new empty in-flight jobs tracker.
pub fn new_in_flight_jobs() -> InFlightJobs {
    Arc::new(Mutex::new(InFlightTracker::default()))
}

/// Shared handles the print workflow needs, bundled to keep signatures small.
#[derive(Clone)]
pub struct JobContext {
    pub http_client: Client,
    pub in_flight_jobs: InFlightJobs,
    pub printer_cache: PrinterCache,
}

/// Maximum time (seconds) to wait for a CUPS job before marking it as failed.
const CUPS_JOB_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Maximum time (seconds) for a media file download (overrides the client default).
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

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
        payload["printed_at"] = serde_json::json!(now_utc());
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

/// Current UTC time as `YYYY-MM-DD HH:MM:SS` (Laravel-compatible) for `printed_at`.
fn now_utc() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Fetch all incomplete print jobs from the API, following pagination so a
/// backlog larger than one page is not silently truncated.
pub async fn fetch_incomplete_jobs(
    http_client: &Client,
    config: &Config,
) -> SpoolerResult<Vec<PrintJob>> {
    let base_url = format!(
        "{}/api/print-jobs?filter[is_completed]=false&include=printer",
        config.flux_url
    );

    let mut jobs = Vec::new();
    let mut page = 1u32;

    loop {
        let url = format!("{}&page={}", base_url, page);
        debug!(url = %url, "Fetching print jobs");

        let response = with_auth_header(http_client.get(&url), config)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to fetch print jobs: {}", response.status()).into());
        }

        let parsed: PrintJobResponse = serde_json::from_str(&response.text().await?)
            .map_err(|e| format!("Failed to parse print jobs: {}", e))?;

        jobs.extend(parsed.data.data);

        if parsed.data.current_page >= parsed.data.last_page {
            return Ok(jobs);
        }
        page = parsed.data.current_page + 1;
    }
}

// ── Printer helpers ─────────────────────────────────────────────────────────

/// System default printer (first CUPS printer) system_name, if any.
fn default_printer_system_name() -> Option<String> {
    get_printers().into_iter().next().map(|p| p.system_name)
}

/// Resolve a synced printer id to its CUPS system_name via the shared cache,
/// falling back to the on-disk printer store when the cache is empty (CLI use).
fn resolve_printer_system_name(printer_id: u32, cache: &PrinterCache) -> Option<String> {
    let find = |map: &HashMap<String, Printer>| {
        map.values()
            .find(|p| p.printer_id == Some(printer_id))
            .map(|p| p.system_name.clone())
    };

    let guard = crate::utils::sync::read(cache);
    if guard.is_empty() {
        drop(guard);
        find(&load_printers())
    } else {
        find(&guard)
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
        .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
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

    let temp_file = tokio::task::spawn_blocking(move || -> std::io::Result<NamedTempFile> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(&file_content)?;
        Ok(temp_file)
    })
    .await
    .map_err(|e| format!("Temp file task failed: {}", e))??;

    Ok(temp_file)
}

/// Resolve the target printer and submit the file to CUPS (blocking FFI).
///
/// Resolution order: synced printer id → printer name from the job → system
/// default when the job names no printer at all. A printer that is named but
/// missing from CUPS is an error — never print on an arbitrary device.
fn submit_to_cups(
    job_id: u32,
    printer_id: Option<u32>,
    printer_name: Option<String>,
    temp_path: &str,
    cache: &PrinterCache,
) -> Result<(u64, String), String> {
    let resolved = printer_id
        .and_then(|id| resolve_printer_system_name(id, cache))
        .or(printer_name)
        .or_else(default_printer_system_name);

    let Some(name) = resolved else {
        return Err("No printers available".to_string());
    };

    let Some(printer) = get_printer_by_name(&name) else {
        return Err(format!("Printer '{}' not found in CUPS", name));
    };

    let job_name = format!("Print Job {}", job_id);
    let job_options = PrinterJobOptions {
        name: Some(&job_name),
        ..PrinterJobOptions::none()
    };

    let cups_job_id = printer
        .print_file(temp_path, job_options)
        .map_err(|e| format!("Failed to print: {:?}", e))?;

    Ok((cups_job_id, printer.system_name))
}

/// Download, print, and track job status — core print workflow.
///
/// Reserves the job id up front so concurrent triggers cannot double-print,
/// submits to CUPS, and registers the job as in-flight for the status checker.
pub async fn process_print_job(
    job: &PrintJob,
    config: &Config,
    ctx: &JobContext,
) -> SpoolerResult<()> {
    if !lock(&ctx.in_flight_jobs).reserve(job.id) {
        info!(job_id = job.id, "Job already being processed, skipping");
        return Ok(());
    }

    let result = submit_print_job(job, config, ctx).await;
    if result.is_err() {
        lock(&ctx.in_flight_jobs).release(job.id);
    }
    result
}

async fn submit_print_job(job: &PrintJob, config: &Config, ctx: &JobContext) -> SpoolerResult<()> {
    let temp_file = download_file(&ctx.http_client, config, job.media_id).await?;
    let temp_path = temp_file
        .path()
        .to_str()
        .ok_or("Invalid temp file path")?
        .to_string();

    let job_id = job.id;
    let printer_id = job.printer.as_ref().map(|p| p.id).or(job.printer_id);
    let printer_name = job.printer.as_ref().map(|p| p.name.clone());
    let cache = ctx.printer_cache.clone();

    let submitted = tokio::task::spawn_blocking(move || {
        submit_to_cups(job_id, printer_id, printer_name, &temp_path, &cache)
    })
    .await
    .map_err(|e| format!("CUPS task failed: {}", e))?;

    let (cups_job_id, system_name) = match submitted {
        Ok(result) => result,
        Err(msg) => {
            // Submission failures don't fix themselves — mark the job failed
            // in the API instead of leaving it pending (or printing elsewhere).
            error!(job_id = job.id, error = %msg, "Failed to submit job to CUPS");
            if let Err(e) = update_print_job_status(
                job.id,
                None,
                PrintJobStatus::Failed,
                Some(&msg),
                &ctx.http_client,
                config,
            )
            .await
            {
                warn!(job_id = job.id, error = %e, "Failed to update job status to failed");
            }
            return Err(msg.into());
        }
    };

    info!(
        job_id = job.id,
        cups_job_id,
        printer = %system_name,
        "Print job submitted to CUPS"
    );

    match update_print_job_status(
        job.id,
        Some(cups_job_id),
        PrintJobStatus::Queued,
        None,
        &ctx.http_client,
        config,
    )
    .await
    {
        Ok(_) => info!(job_id = job.id, cups_job_id, "Status updated to queued"),
        Err(e) => warn!(job_id = job.id, error = %e, "Failed to update job status to queued"),
    }

    lock(&ctx.in_flight_jobs).confirm(InFlightJob {
        api_job_id: job.id,
        cups_job_id,
        printer_name: system_name,
        submitted_at: Instant::now(),
        last_status: PrintJobStatus::Queued,
    });

    Ok(())
}

/// Fetch print jobs from the API and process them concurrently.
pub async fn fetch_print_jobs(config: &Config, ctx: &JobContext) -> SpoolerResult<Vec<PrintJob>> {
    let jobs = fetch_incomplete_jobs(&ctx.http_client, config).await?;

    if jobs.is_empty() {
        debug!("No print jobs found for this instance");
        return Ok(jobs);
    }

    info!(job_count = jobs.len(), "Processing print jobs");

    let mut handles = Vec::new();
    for job in &jobs {
        if job.is_in_flight() {
            debug!(job_id = job.id, "Skipping in-flight job");
            continue;
        }

        let job = job.clone();
        let config = config.clone();
        let ctx = ctx.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = process_print_job(&job, &config, &ctx).await {
                error!(job_id = job.id, error = %e, "Failed to process print job");
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(jobs)
}

/// Single print job response from API (when fetching by ID)
#[derive(serde::Deserialize, Debug)]
struct SinglePrintJobResponse {
    data: PrintJob,
}

/// Fetch a single print job by ID from the API and print it
pub async fn fetch_and_print_job_by_id(
    job_id: u32,
    config: &Config,
    ctx: &JobContext,
) -> SpoolerResult<()> {
    let job_url = format!(
        "{}/api/print-jobs/{}?include=printer",
        config.flux_url, job_id
    );

    info!(job_id, url = %job_url, "Fetching print job by ID");

    let response = with_auth_header(ctx.http_client.get(&job_url), config)
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

    if job.is_completed {
        info!(job_id = job.id, "Job was already printed, skipping");
        return Ok(());
    }

    process_print_job(&job, config, ctx).await
}

// ── Background tasks ────────────────────────────────────────────────────────

/// Background task to periodically check for print jobs (polling mode)
pub async fn job_checker_task(
    config: Arc<RwLock<Config>>,
    ctx: JobContext,
    cancel_token: CancellationToken,
) {
    loop {
        let config_snapshot = read_config(&config);

        if !config_snapshot.reverb_disabled {
            info!("Polling is disabled. Using Reverb WebSockets instead");
            return;
        }

        let interval = config_snapshot.job_check_interval.max(1);

        match fetch_print_jobs(&config_snapshot, &ctx).await {
            Ok(jobs) => {
                if !jobs.is_empty() {
                    info!(job_count = jobs.len(), "Processed print jobs");
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

/// Re-populate the in-flight tracker from the API after a restart.
async fn recover_in_flight_jobs(config: &Config, ctx: &JobContext) {
    let jobs = match fetch_incomplete_jobs(&ctx.http_client, config).await {
        Ok(jobs) => jobs,
        Err(e) => {
            warn!(error = %e, "Failed to recover in-flight jobs from API");
            return;
        }
    };

    let default_name = tokio::task::spawn_blocking(default_printer_system_name)
        .await
        .ok()
        .flatten();

    let mut recovered = 0usize;
    let mut tracker = lock(&ctx.in_flight_jobs);
    for job in jobs.iter().filter(|j| j.is_in_flight()) {
        let Some(cups_id) = job.cups_job_id else {
            continue;
        };
        let printer_name = job
            .printer
            .as_ref()
            .map(|p| p.name.clone())
            .or_else(|| default_name.clone());
        let Some(printer_name) = printer_name else {
            continue;
        };

        tracker.track_if_new(InFlightJob {
            api_job_id: job.id,
            cups_job_id: cups_id,
            printer_name,
            submitted_at: Instant::now(),
            last_status: job.status.clone().unwrap_or(PrintJobStatus::Queued),
        });
        recovered += 1;
    }

    if recovered > 0 {
        info!(recovered, "Recovered in-flight jobs from API");
    }
}

/// Query CUPS once for the states of the given job ids on one printer.
fn query_cups_states(printer_name: &str, wanted: &[u64]) -> HashMap<u64, PrinterJobState> {
    let mut states = HashMap::new();
    let Some(printer) = get_printer_by_name(printer_name) else {
        return states;
    };

    for cups_job in printer
        .get_active_jobs()
        .into_iter()
        .chain(printer.get_job_history())
    {
        if wanted.contains(&cups_job.id) {
            states.insert(cups_job.id, cups_job.state);
        }
    }
    states
}

/// Push a status change for one in-flight job to the API and update/remove it
/// in the tracker.
async fn apply_status_change(
    job: &InFlightJob,
    new_status: PrintJobStatus,
    error_msg: Option<&str>,
    config: &Config,
    ctx: &JobContext,
) {
    match update_print_job_status(
        job.api_job_id,
        None,
        new_status.clone(),
        error_msg,
        &ctx.http_client,
        config,
    )
    .await
    {
        Ok(_) => info!(job_id = job.api_job_id, status = %new_status, "Status updated in API"),
        Err(e) => error!(
            job_id = job.api_job_id,
            status = %new_status,
            error = %e,
            "Failed to update status in API"
        ),
    }

    let mut tracker = lock(&ctx.in_flight_jobs);
    if new_status.is_terminal() {
        tracker.jobs.retain(|j| j.api_job_id != job.api_job_id);
    } else if let Some(tracked) = tracker
        .jobs
        .iter_mut()
        .find(|j| j.api_job_id == job.api_job_id)
    {
        tracked.last_status = new_status;
    }
}

/// Check all in-flight jobs of one printer against CUPS with a single query.
async fn check_printer_jobs(
    printer_name: String,
    jobs: Vec<InFlightJob>,
    config: &Config,
    ctx: &JobContext,
) {
    let wanted: Vec<u64> = jobs.iter().map(|j| j.cups_job_id).collect();
    let name = printer_name.clone();

    let states = match tokio::task::spawn_blocking(move || query_cups_states(&name, &wanted)).await
    {
        Ok(states) => states,
        Err(e) => {
            error!(printer = %printer_name, error = %e, "Failed to query CUPS job status");
            return;
        }
    };

    for job in &jobs {
        match states.get(&job.cups_job_id) {
            Some(state) => {
                let new_status = PrintJobStatus::from(state.clone());
                if new_status == job.last_status {
                    trace!(
                        job_id = job.api_job_id,
                        cups_job_id = job.cups_job_id,
                        status = %new_status,
                        "CUPS job status unchanged"
                    );
                    continue;
                }

                let error_msg = (new_status == PrintJobStatus::Cancelled)
                    .then_some("Job cancelled or aborted by CUPS");

                info!(
                    job_id = job.api_job_id,
                    cups_job_id = job.cups_job_id,
                    status = %new_status,
                    "CUPS job status changed"
                );
                apply_status_change(job, new_status, error_msg, config, ctx).await;
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
                    apply_status_change(
                        job,
                        PrintJobStatus::Failed,
                        Some("Job disappeared from CUPS queue"),
                        config,
                        ctx,
                    )
                    .await;
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
}

/// Background task that polls CUPS for the final status of in-flight print jobs.
///
/// Runs every 15 seconds; printers are checked concurrently, each with a single
/// CUPS query covering all of its in-flight jobs. When a job reaches a terminal
/// state (completed, cancelled, aborted) or times out, the API is updated and
/// the job is removed from the in-flight tracker.
pub async fn job_status_checker_task(
    config: Arc<RwLock<Config>>,
    ctx: JobContext,
    cancel_token: CancellationToken,
) {
    recover_in_flight_jobs(&read_config(&config), &ctx).await;

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
            let tracker = lock(&ctx.in_flight_jobs);
            if tracker.jobs.is_empty() {
                continue;
            }
            tracker.jobs.clone()
        };

        trace!(
            count = snapshot.len(),
            "Checking CUPS status for in-flight jobs"
        );

        let config_snapshot = read_config(&config);

        let mut by_printer: HashMap<String, Vec<InFlightJob>> = HashMap::new();
        for job in snapshot {
            by_printer
                .entry(job.printer_name.clone())
                .or_default()
                .push(job);
        }

        let mut handles = Vec::new();
        for (printer_name, jobs) in by_printer {
            let config = config_snapshot.clone();
            let ctx = ctx.clone();
            handles.push(tokio::spawn(async move {
                check_printer_jobs(printer_name, jobs, &config, &ctx).await;
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_flight(api_job_id: u32) -> InFlightJob {
        InFlightJob {
            api_job_id,
            cups_job_id: 42,
            printer_name: "printer".to_string(),
            submitted_at: Instant::now(),
            last_status: PrintJobStatus::Queued,
        }
    }

    #[test]
    fn tracker_prevents_duplicate_processing() {
        let mut tracker = InFlightTracker::default();

        assert!(tracker.reserve(1));
        assert!(!tracker.reserve(1), "reserved job must not be re-reserved");

        tracker.confirm(in_flight(1));
        assert!(!tracker.reserve(1), "tracked job must not be re-reserved");

        assert!(tracker.reserve(2));
        tracker.release(2);
        assert!(tracker.reserve(2), "released job can be reserved again");
    }

    #[test]
    fn tracker_track_if_new_ignores_duplicates() {
        let mut tracker = InFlightTracker::default();

        tracker.track_if_new(in_flight(1));
        tracker.track_if_new(in_flight(1));
        assert_eq!(tracker.jobs.len(), 1);
    }

    #[test]
    fn now_utc_is_laravel_format() {
        let timestamp = now_utc();
        // YYYY-MM-DD HH:MM:SS
        assert_eq!(timestamp.len(), 19);
        assert_eq!(&timestamp[4..5], "-");
        assert_eq!(&timestamp[7..8], "-");
        assert_eq!(&timestamp[10..11], " ");
        assert_eq!(&timestamp[13..14], ":");
        assert_eq!(&timestamp[16..17], ":");
    }
}
