use actix_multipart::Multipart;
use actix_web::{get, post, web, App, Error, HttpResponse, HttpServer, Responder};
use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

#[derive(Serialize, Deserialize)]
struct PrintRequest {
    printer: String,
}

#[derive(Serialize)]
struct Printer {
    name: String,
    description: String,
    location: String,
    make_and_model: String,
    media_sizes: Vec<String>,
}

#[derive(Serialize)]
struct PrinterList {
    printers: Vec<Printer>,
}

#[get("/printers")]
async fn get_printers() -> impl Responder {
    // First, log the raw output for debugging
    let debug_output = Command::new("lpstat")
        .arg("-a")
        .output()
        .expect("Failed to execute lpstat -a command");

    println!("Debug lpstat -a: {}", String::from_utf8_lossy(&debug_output.stdout));

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

        println!("Debug lpstat -p: {}", String::from_utf8_lossy(&alt_output.stdout));

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

        println!("Debug lpstat -v: {}", String::from_utf8_lossy(&v_output.stdout));

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

        println!("Debug shell command: {}", String::from_utf8_lossy(&shell_output.stdout));

        let shell_list_str = String::from_utf8_lossy(&shell_output.stdout);
        final_printer_names = shell_list_str
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();
    }

    println!("Detected printers: {:?}", final_printer_names);

    let mut printers = Vec::new();

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
                if line.starts_with("PageSize/") || line.starts_with("MediaSize/") || line.contains("media size") {
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
                    description = line.split("Description:").nth(1).unwrap_or("").trim().to_string();
                } else if line.contains("Location:") {
                    location = line.split("Location:").nth(1).unwrap_or("").trim().to_string();
                } else if line.contains("Make and Model:") {
                    make_and_model = line.split("Make and Model:").nth(1).unwrap_or("").trim().to_string();
                }
            }
        }

        if description.is_empty() && make_and_model.is_empty() {
            let lpinfo_output = Command::new("lpinfo")
                .arg("-m")
                .output();

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

        printers.push(Printer {
            name,
            description,
            location,
            make_and_model,
            media_sizes,
        });
    }

    HttpResponse::Ok().json(PrinterList { printers })
}

#[post("/print")]
async fn print_file(mut payload: Multipart, query: web::Query<PrintRequest>) -> Result<HttpResponse, Error> {
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
        let content_disposition = field.content_disposition();

        if let Some(_filename) = content_disposition.get_filename() {
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
                return Ok(HttpResponse::InternalServerError().body(format!("Print failed: {}", error_msg)));
            }
        }
    }

    Ok(HttpResponse::BadRequest().body("No file provided"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Starting CUPS print server on http://127.0.0.1:8080");

    HttpServer::new(|| {
        App::new()
            .service(get_printers)
            .service(print_file)
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}