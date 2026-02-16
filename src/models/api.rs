use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiPrinter {
    pub id: Option<u32>,
    pub name: String,
    pub system_name: Option<String>,
    pub uri: Option<String>,
    pub spooler_name: String, // Changed from printer_server
    pub location: Option<String>,
    pub make_and_model: Option<String>,
    pub media_sizes: Vec<String>, // Changed from Option<Vec<String>>
    pub is_active: Option<bool>,
    pub is_visible: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiPrinterResponse {
    pub status: u16,
    pub data: ApiPrinterData,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiPrinterData {
    pub data: Vec<ApiPrinter>,
}

// Convert from local Printer to ApiPrinter for sending to server
impl From<&crate::models::Printer> for ApiPrinter {
    fn from(printer: &crate::models::Printer) -> Self {
        ApiPrinter {
            id: printer.printer_id,
            name: printer.name.clone(),
            system_name: Some(printer.system_name.clone()),
            uri: printer.uri.clone(),
            spooler_name: "".to_string(), // Will be set before sending
            location: Some(printer.location.clone()),
            make_and_model: Some(printer.make_and_model.clone()),
            media_sizes: if printer.media_sizes.is_empty() {
                warn!(
                    printer = %printer.name,
                    "No media sizes from CUPS, falling back to [\"A4\"]"
                );
                vec!["A4".to_string()]
            } else {
                printer.media_sizes.clone()
            },
            is_active: Some(true),
            is_visible: Some(true),
        }
    }
}

// Convert from ApiPrinter to local Printer
impl From<&ApiPrinter> for crate::models::Printer {
    fn from(api_printer: &ApiPrinter) -> Self {
        crate::models::Printer {
            name: api_printer.name.clone(),
            system_name: api_printer
                .system_name
                .clone()
                .unwrap_or_else(|| api_printer.name.clone()),
            uri: api_printer.uri.clone(),
            description: "".to_string(), // No longer used in API
            location: api_printer.location.clone().unwrap_or_default(),
            make_and_model: api_printer.make_and_model.clone().unwrap_or_default(),
            media_sizes: api_printer.media_sizes.clone(),
            printer_id: api_printer.id,
        }
    }
}
