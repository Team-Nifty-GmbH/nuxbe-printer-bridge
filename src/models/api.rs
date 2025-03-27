use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiPrinter {
    pub id: Option<u32>,
    pub name: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub make_and_model: Option<String>,
    pub media_sizes: Option<Vec<String>>,
    pub printer_server: String,
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
            description: Some(printer.description.clone()),
            location: Some(printer.location.clone()),
            make_and_model: Some(printer.make_and_model.clone()),
            media_sizes: Some(printer.media_sizes.clone()),
            printer_server: "".to_string(), // Will be set before sending
        }
    }
}

// Convert from ApiPrinter to local Printer
impl From<&ApiPrinter> for crate::models::Printer {
    fn from(api_printer: &ApiPrinter) -> Self {
        crate::models::Printer {
            name: api_printer.name.clone(),
            description: api_printer.description.clone().unwrap_or_default(),
            location: api_printer.location.clone().unwrap_or_default(),
            make_and_model: api_printer.make_and_model.clone().unwrap_or_default(),
            media_sizes: api_printer.media_sizes.clone().unwrap_or_default(),
            printer_id: api_printer.id,
        }
    }
}