use serde::{Deserialize, Serialize};

/// Configuration structure for the application
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub instance_name: String,        // Name for this printer server instance
    pub printer_check_interval: u64,  // in minutes
    pub job_check_interval: u64,      // in minutes
    pub flux_url: String,             // Base URL for all API endpoints
    pub flux_interface_user_name: String,   // Authentication token for notifications
    pub flux_interface_user_password: String,     // Authentication token for print jobs
    pub flux_api_token: Option<String>,    // Authentication token for print jobs
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
            flux_interface_user_name: "spooler".to_string(),
            flux_interface_user_password: "strong-password".to_string(),
            flux_api_token: None,
            api_port: 8080,
            reverb_disabled: false,
            reverb_app_id: "default-app-id".to_string(),
            reverb_app_key: "default-app-key".to_string(),
            reverb_app_secret: "default-app-secret".to_string(),
            reverb_use_tls: true,
            reverb_host: None,
            reverb_auth_endpoint: "http://example.com/auth".to_string(),
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