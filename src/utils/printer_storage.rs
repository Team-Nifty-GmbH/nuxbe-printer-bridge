use crate::models::Printer;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Path to the printers JSON file
pub fn printers_file_path() -> PathBuf {
    let config_dir = crate::utils::config::config_dir();
    config_dir.join("printers.json")
}

/// Load printers from JSON file
pub fn load_printers() -> HashMap<String, Printer> {
    let path = printers_file_path();

    if !path.exists() {
        return HashMap::new();
    }

    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            warn!(error = %e, "Error parsing printers file, using empty list");
            HashMap::new()
        }),
        Err(_) => {
            debug!("Printers file not found, starting with empty list");
            HashMap::new()
        }
    }
}

/// Check if printer maps are different (ignoring order)
pub fn printers_have_changed(
    current: &HashMap<String, Printer>,
    saved: &HashMap<String, Printer>,
) -> bool {
    // Quick check: different number of printers
    if current.len() != saved.len() {
        return true;
    }

    // Check if all printers in current exist in saved and are identical
    for (name, current_printer) in current {
        match saved.get(name) {
            Some(saved_printer) => {
                // Compare all relevant fields
                if current_printer.system_name != saved_printer.system_name
                    || current_printer.uri != saved_printer.uri
                    || current_printer.description != saved_printer.description
                    || current_printer.location != saved_printer.location
                    || current_printer.make_and_model != saved_printer.make_and_model
                    || current_printer.media_sizes != saved_printer.media_sizes
                    || current_printer.printer_id != saved_printer.printer_id
                {
                    return true;
                }
            }
            None => return true, // Printer exists in current but not in saved
        }
    }

    false // No differences found
}

/// Save printers to JSON file only if they have changed
pub fn save_printers_if_changed(
    printers: &HashMap<String, Printer>,
    saved_printers: &HashMap<String, Printer>,
) -> bool {
    if !printers_have_changed(printers, saved_printers) {
        return false; // No changes, no need to save
    }

    save_printers(printers);
    true // Changes were saved
}

/// Save printers to JSON file
pub fn save_printers(printers: &HashMap<String, Printer>) {
    let path = printers_file_path();
    let config_dir = crate::utils::config::config_dir();

    // create_dir_all is idempotent - no need to check existence first
    if let Err(e) = fs::create_dir_all(&config_dir) {
        warn!(error = %e, "Failed to create config directory");
        return;
    }

    match serde_json::to_string_pretty(printers) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                warn!(error = %e, "Failed to save printers file");
            } else {
                debug!(count = printers.len(), path = %path.display(), "Saved printers");
            }
        }
        Err(e) => warn!(error = %e, "Failed to serialize printers"),
    }
}
