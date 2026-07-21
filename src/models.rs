use std::fmt;

use printers::common::base::job::PrinterJobState;
use serde::{Deserialize, Serialize};

pub mod api;

/// Status of a print job as tracked by the bridge.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PrintJobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for PrintJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrintJobStatus::Queued => write!(f, "queued"),
            PrintJobStatus::Processing => write!(f, "processing"),
            PrintJobStatus::Completed => write!(f, "completed"),
            PrintJobStatus::Failed => write!(f, "failed"),
            PrintJobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl From<PrinterJobState> for PrintJobStatus {
    fn from(state: PrinterJobState) -> Self {
        match state {
            PrinterJobState::PENDING | PrinterJobState::PAUSED => PrintJobStatus::Queued,
            PrinterJobState::PROCESSING => PrintJobStatus::Processing,
            PrinterJobState::COMPLETED => PrintJobStatus::Completed,
            PrinterJobState::CANCELLED => PrintJobStatus::Cancelled,
            PrinterJobState::UNKNOWN => PrintJobStatus::Failed,
        }
    }
}

impl PrintJobStatus {
    /// Whether this status represents a terminal state (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            PrintJobStatus::Completed | PrintJobStatus::Failed | PrintJobStatus::Cancelled
        )
    }
}

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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Printer {
    pub name: String,
    pub system_name: String,
    pub uri: Option<String>,
    pub description: String,
    pub location: String,
    pub make_and_model: String,
    pub media_sizes: Vec<String>,
    pub printer_id: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJobResponse {
    pub data: PrintJobPaginatedData,
}

/// Laravel paginator envelope; only the fields needed to walk all pages.
#[derive(Serialize, Deserialize, Debug)]
pub struct PrintJobPaginatedData {
    pub current_page: u32,
    pub data: Vec<PrintJob>,
    pub last_page: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrintJob {
    pub id: u32,
    pub media_id: u32,
    pub printer_id: Option<u32>,
    pub user_id: Option<u32>,
    pub quantity: u32,
    pub size: String,
    pub is_completed: bool,
    pub cups_job_id: Option<u64>,
    pub status: Option<PrintJobStatus>,
    pub error_message: Option<String>,
    pub printed_at: Option<String>,
    pub created_at: String,
    pub created_by: Option<u32>,
    pub updated_at: String,
    pub updated_by: Option<u32>,
    /// Included printer relationship (when using ?include=printer)
    pub printer: Option<PrintJobPrinter>,
}

impl PrintJob {
    /// Whether this job is already submitted to CUPS and being tracked.
    pub fn is_in_flight(&self) -> bool {
        self.cups_job_id.is_some()
            && matches!(
                self.status,
                Some(PrintJobStatus::Queued) | Some(PrintJobStatus::Processing)
            )
    }
}

/// Printer data included in print job response
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrintJobPrinter {
    pub id: u32,
    pub name: String,
    pub spooler_name: String,
    pub is_active: bool,
}
