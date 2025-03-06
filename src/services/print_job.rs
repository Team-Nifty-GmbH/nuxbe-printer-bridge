use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Client;
use tempfile::NamedTempFile;
use tokio::time;

use crate::models::{Config, PrintJob, WebsocketPrintJob};

/// Process a print job received through WebSocket
pub async fn handle_print_job(
    print_job: WebsocketPrintJob,
    http_client: &Client,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Processing WebSocket print job for printer: {} from server: {} with media id: {}",
        print_job.printer_name, print_job.printer_server, print_job.media_id
    );

    // Check if this job is for this instance
    if print_job.printer_server != config.instance_name {
        println!(
            "Ignoring job for different printer server: {} (we are: {})",
            print_job.printer_server, config.instance_name
        );
        return Ok(());
    }

    // Construct the URL to get the file
    let file_url = format!(
        "{}/api/media/{}/download",
        config.host_url, print_job.media_id
    );

    // Download the file
    let file_response = http_client
        .get(&file_url)
        .header(
            "Authorization",
            format!("Bearer {}", config.print_jobs_token),
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
    } else {
        eprintln!(
            "Failed to print media ID {} on printer {}: {}",
            print_job.media_id,
            print_job.printer_name,
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(format!(
            "Failed to print: {}",
            String::from_utf8_lossy(&output.stderr)
        )
            .into());
    }

    Ok(())
}

/// Fetch print jobs from the API and process them
pub async fn fetch_print_jobs(
    http_client: &Client,
    config: &Config,
) -> Result<Vec<PrintJob>, Box<dyn std::error::Error>> {
    // Construct the URL for fetching print jobs
    let jobs_url = format!("{}/api/print-jobs", config.host_url);

    let response = http_client
        .get(&jobs_url)
        .header(
            "Authorization",
            format!("Bearer {}", config.print_jobs_token),
        )
        .header("X-Instance-Name", &config.instance_name)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch print jobs: {}", response.status()).into());
    }

    let jobs: Vec<PrintJob> = response.json().await?;

    // Process each job
    for job in &jobs {
        println!(
            "Processing print job {} for printer {}",
            job.id, job.printer
        );

        // Download the file
        let file_response = http_client.get(&job.file_url).send().await?;
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
            .arg(&job.printer)
            .arg(temp_path)
            .output()?;

        if output.status.success() {
            println!(
                "Successfully printed job {}: {}",
                job.id,
                String::from_utf8_lossy(&output.stdout)
            );
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

/// Background task to periodically check for print jobs
pub async fn job_checker_task(config: Arc<Mutex<Config>>, http_client: Client) {
    loop {
        // Get interval and clone config outside of the await
        let interval;
        let config_clone;

        {
            let config_guard = config.lock().unwrap();
            interval = config_guard.job_check_interval;
            config_clone = config_guard.clone();
        }

        match fetch_print_jobs(&http_client, &config_clone).await {
            Ok(jobs) => {
                if !jobs.is_empty() {
                    println!("Processed {} print job(s)", jobs.len());
                }
            }
            Err(e) => eprintln!("Error fetching print jobs: {}", e),
        }

        time::sleep(Duration::from_secs(interval * 60)).await;
    }
}