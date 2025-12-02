use serde::{Deserialize, Serialize};

pub mod api;

/// Configuration structure for the application
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub instance_name: String,
    pub printer_check_interval: u64,
    pub job_check_interval: u64,
    pub flux_url: String,
    pub flux_api_token: Option<String>,
    pub api_port: u16,
    pub reverb_disabled: bool,
    pub reverb_app_id: String,
    pub reverb_app_key: String,
    pub reverb_app_secret: String,
    pub reverb_use_tls: bool,
    pub reverb_host: Option<String>,
    pub reverb_auth_endpoint: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            instance_name: "default-instance".to_string(),
            printer_check_interval: 5,
            job_check_interval: 2,
            flux_url: "http://example.com".to_string(),
            flux_api_token: None,
            api_port: 8080,
            reverb_disabled: true,
            reverb_app_id: "default-app-id".to_string(),
            reverb_app_key: "default-app-key".to_string(),
            reverb_app_secret: "default-app-secret".to_string(),
            reverb_use_tls: true,
            reverb_host: None,
            reverb_auth_endpoint: "http://example.com/auth".to_string(),
        }
    }
}

// Used by API (currently disabled)
#[allow(dead_code)]
#[derive(Serialize, Deserialize)]
pub struct PrintRequest {
    pub printer: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Printer {
    pub name: String,
    pub description: String,
    pub location: String,
    pub make_and_model: String,
    pub media_sizes: Vec<String>,
    pub printer_id: Option<u32>,
}

// Used by API (currently disabled)
#[allow(dead_code)]
#[derive(Serialize, Deserialize)]
pub struct PrinterList {
    pub printers: Vec<Printer>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PageLink {
    pub url: Option<String>,
    pub label: String,
    pub active: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJobResponse {
    pub status: u16,
    pub data: PrintJobPaginatedData,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJobPaginatedData {
    pub current_page: u32,
    pub data: Vec<PrintJob>,
    pub first_page_url: String,
    pub from: Option<u32>,
    pub last_page: u32,
    pub last_page_url: String,
    pub links: Vec<PageLink>,
    pub next_page_url: Option<String>,
    pub path: String,
    pub per_page: u32,
    pub prev_page_url: Option<String>,
    pub to: Option<u32>,
    pub total: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJob {
    pub id: u32,
    pub media_id: u32,
    pub printer_id: Option<u32>,
    pub user_id: Option<u32>,
    pub quantity: u32,
    pub size: String,
    pub is_completed: bool,
    pub created_at: String,
    pub created_by: Option<u32>,
    pub updated_at: String,
    pub updated_by: Option<u32>,
    /// Included printer relationship (when using ?include=printer)
    pub printer: Option<PrintJobPrinter>,
}

/// Printer data included in print job response
#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJobPrinter {
    pub id: u32,
    pub name: String,
    pub spooler_name: String,
    pub is_active: bool,
}
