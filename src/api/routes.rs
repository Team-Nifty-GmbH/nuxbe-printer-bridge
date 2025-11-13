use std::collections::HashSet;
use std::io::Write;
use std::sync::{Arc, Mutex};

use actix_multipart::Multipart;
use actix_web::{Error, HttpResponse, Responder, get, post, web};
use cursive::reexports::log::{debug, trace};
use futures::{StreamExt, TryStreamExt};
use printers::common::base::job::PrinterJobOptions;
use printers::get_printer_by_name;
use reqwest::Client;
use tempfile::NamedTempFile;

use crate::models::{Config, PrintRequest, PrinterList};
use crate::services::print_job::fetch_print_jobs;
use crate::services::printer::{check_for_new_printers, get_all_printers};
use crate::utils::printer_storage::load_printers;

/// GET /printers - List all available printers
#[get("/printers")]
pub async fn get_printers(verbose_debug: web::Data<bool>) -> impl Responder {
    let saved_printers = load_printers();

    if !saved_printers.is_empty() {
        let printers = saved_printers.values().cloned().collect();
        return HttpResponse::Ok().json(PrinterList { printers });
    }
    let printers = get_all_printers(**verbose_debug).await;
    HttpResponse::Ok().json(PrinterList { printers })
}

/// POST /print - Print a file to a specified printer
#[post("/print")]
pub async fn print_file(
    mut payload: Multipart,
    query: web::Query<PrintRequest>,
    verbose_debug: web::Data<bool>,
) -> Result<HttpResponse, Error> {
    let printer_name = &query.printer;

    while let Ok(Some(mut field)) = payload.try_next().await {
        if let Some(content_disposition) = field.content_disposition()
            && let Some(filename) = content_disposition.get_filename() {
                let filename_str = filename.to_string();

                if **verbose_debug {
                    trace!("Processing file upload: {}", filename_str);
                }

                let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    temp_file.write_all(&data)?;
                }

                let temp_path = temp_file.path().to_str().unwrap();

                if **verbose_debug {
                    debug!("Printing file: {} to printer: {}", temp_path, printer_name);
                }
                match get_printer_by_name(printer_name) {
                    Some(printer) => {
                        let job_options = PrinterJobOptions {
                            name: Some("API Print Job"),
                            ..PrinterJobOptions::none()
                        };

                        match printer.print_file(temp_path, job_options) {
                            Ok(job_id) => {
                                return Ok(HttpResponse::Ok().body(format!(
                                    "Print job submitted successfully (Job ID: {})",
                                    job_id
                                )));
                            }
                            Err(e) => {
                                return Ok(HttpResponse::InternalServerError()
                                    .body(format!("Print failed: {}", e)));
                            }
                        }
                    }
                    None => {
                        return Ok(HttpResponse::BadRequest()
                            .body(format!("Printer '{}' not found", printer_name)));
                    }
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
    let mut config_clone = {
        let guard = config.lock().unwrap();
        guard.clone()
    };

    match fetch_print_jobs(&http_client, &mut config_clone).await {
        Ok(jobs) => {
            if let Ok(mut guard) = config.lock() {
                guard.flux_api_token = config_clone.flux_api_token;
            }
            HttpResponse::Ok().json(jobs)
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to check jobs: {}", e)),
    }
}

/// GET /check_printers - Manually check for new printers
#[get("/check_printers")]
pub async fn check_printers_endpoint(
    printers_data: web::Data<Arc<Mutex<HashSet<String>>>>,
    config: web::Data<Arc<Mutex<Config>>>,
    http_client: web::Data<Client>,
    verbose_debug: web::Data<bool>,
) -> impl Responder {
    match check_for_new_printers(printers_data, http_client, config, **verbose_debug).await {
        Ok(_new_printers) => {
            let saved_printers = load_printers();
            let printers: Vec<_> = saved_printers.values().cloned().collect();
            HttpResponse::Ok().json(PrinterList { printers })
        }
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Failed to check printers: {}", e))
        }
    }
}
