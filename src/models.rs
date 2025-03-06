use serde::{Deserialize, Serialize};

/// Configuration structure for the application
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub instance_name: String,        // Name for this printer server instance
    pub printer_check_interval: u64,  // in minutes
    pub job_check_interval: u64,      // in minutes
    pub host_url: String,             // Base URL for all API endpoints
    pub notification_token: String,   // Authentication token for notifications
    pub print_jobs_token: String,     // Authentication token for print jobs
    pub admin_port: u16,              // Admin interface port
    pub api_port: u16,                // API port
    pub reverb_app_id: String,
    pub reverb_app_key: String,
    pub reverb_app_secret: String,
    pub reverb_use_tls: bool,
    pub reverb_host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            instance_name: "default-instance".to_string(),
            printer_check_interval: 5,
            job_check_interval: 2,
            host_url: "http://example.com".to_string(),
            notification_token: "default-token".to_string(),
            print_jobs_token: "default-token".to_string(),
            admin_port: 8081,
            api_port: 8080,
            reverb_app_id: "default-app-id".to_string(),
            reverb_app_key: "default-app-key".to_string(),
            reverb_app_secret: "default-app-secret".to_string(),
            reverb_use_tls: true,
            reverb_host: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PrintRequest {
    pub printer: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Printer {
    pub name: String,
    pub description: String,
    pub location: String,
    pub make_and_model: String,
    pub media_sizes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PrinterList {
    pub printers: Vec<Printer>,
}

#[derive(Serialize, Deserialize)]
pub struct PrinterNotification {
    pub action: String,
    pub printer: Printer,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJob {
    pub id: String,
    pub printer: String,
    pub file_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WebsocketPrintJob {
    pub printer_name: String,
    pub printer_server: String,
    pub media_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigUpdateRequest {
    pub config: Config,
}