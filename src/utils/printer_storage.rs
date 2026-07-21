use crate::models::Printer;
use crate::utils::config::save_json;
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

/// Save printers to JSON file only if they have changed
pub fn save_printers_if_changed(
    printers: &HashMap<String, Printer>,
    saved_printers: &HashMap<String, Printer>,
) -> bool {
    if printers == saved_printers {
        return false; // No changes, no need to save
    }

    save_printers(printers);
    true // Changes were saved
}

/// Save printers to JSON file
pub fn save_printers(printers: &HashMap<String, Printer>) {
    let path = printers_file_path();
    save_json(&path, printers, "printers");
    debug!(count = printers.len(), path = %path.display(), "Saved printers");
}
