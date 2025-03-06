use std::collections::HashSet;
use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};

use actix_multipart::Multipart;
use actix_web::{get, post, web, Error, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use reqwest::Client;
use tempfile::NamedTempFile;

use crate::models::{Config, PrintRequest, PrinterList};
use crate::services::print_job::fetch_print_jobs;
use crate::services::printer::{check_for_new_printers, get_all_printers};

/// GET /printers - List all available printers
#[get("/printers")]
pub async fn get_printers() -> impl Responder {
    let printers = get_all_printers().await;
    HttpResponse::Ok().json(PrinterList { printers })
}

/// POST /print - Print a file to a specified printer
#[post("/print")]
pub async fn print_file(
    mut payload: Multipart,
    query: web::Query<PrintRequest>,
) -> Result<HttpResponse, Error> {
    let printer_name = &query.printer;

    let lpstat_output = Command::new("lpstat")
        .arg("-p")
        .arg(printer_name)
        .output()
        .expect("Failed to execute lpstat command");

    if !lpstat_output.status.success() {
        return Ok(HttpResponse::BadRequest().body(format!("Printer '{}' not found", printer_name)));
    }

    // Process uploaded file
    while let Ok(Some(mut field)) = payload.try_next().await {
        // Get the content disposition directly
        let content_disposition = field.content_disposition();

        // Check if a filename exists
        if let Some(filename) = content_disposition.get_filename() {
            let _filename_str = filename.to_string();

            // Create a temporary file to store the uploaded content
            let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

            // Write file contents to temp file
            while let Some(chunk) = field.next().await {
                let data = chunk?;
                temp_file.write_all(&data)?;
            }

            // Get path to temp file
            let temp_path = temp_file.path().to_str().unwrap();

            // Print the file using lp command
            let output = Command::new("lp")
                .arg("-d")
                .arg(printer_name)
                .arg(temp_path)
                .output()
                .expect("Failed to execute lp command");

            if output.status.success() {
                let success_msg = String::from_utf8_lossy(&output.stdout);
                return Ok(HttpResponse::Ok().body(format!("Print job submitted: {}", success_msg)));
            } else {
                let error_msg = String::from_utf8_lossy(&output.stderr);
                return Ok(HttpResponse::InternalServerError()
                    .body(format!("Print failed: {}", error_msg)));
            }
        }
    }

    Ok(HttpResponse::BadRequest().body("No file provided"))
}

/// GET /check_jobs - Manually check for print jobs
#[get("/check_jobs")]
pub async fn check_jobs_endpoint(
    config: web::Data<Arc<Mutex<Config>>>,
    http_client: web::Data<Client>,
) -> impl Responder {
    let config_guard = config.lock().unwrap();
    match fetch_print_jobs(&http_client, &config_guard).await {
        Ok(jobs) => HttpResponse::Ok().json(jobs),
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to check jobs: {}", e)),
    }
}

/// GET /check_printers - Manually check for new printers
#[get("/check_printers")]
pub async fn check_printers_endpoint(
    printers_data: web::Data<Arc<Mutex<HashSet<String>>>>,
    config: web::Data<Arc<Mutex<Config>>>,
    http_client: web::Data<Client>,
) -> impl Responder {
    match check_for_new_printers(printers_data, http_client, config).await {
        Ok(new_printers) => HttpResponse::Ok().json(new_printers),
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Failed to check printers: {}", e))
        }
    }
}