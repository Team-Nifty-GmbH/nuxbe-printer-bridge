use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::models::Printer;
use crate::config;

/// Path to the printers JSON file
pub fn printers_file_path() -> PathBuf {
    let config_dir = crate::config::config_dir();
    config_dir.join("printers.json")
}

/// Load printers from JSON file
pub fn load_printers() -> HashMap<String, Printer> {
    let path = printers_file_path();

    if !path.exists() {
        return HashMap::new();
    }

    match fs::read_to_string(&path) {
        Ok(contents) => {
            serde_json::from_str(&contents).unwrap_or_else(|e| {
                eprintln!("Error parsing printers file: {}. Using empty list.", e);
                HashMap::new()
            })
        }
        Err(_) => {
            println!("Printers file not found. Starting with empty list.");
            HashMap::new()
        }
    }
}

/// Save printers to JSON file
pub fn save_printers(printers: &HashMap<String, Printer>) {
    let path = printers_file_path();

    // Ensure the config directory exists
    let config_dir = crate::config::config_dir();
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    match serde_json::to_string_pretty(printers) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("Failed to save printers file: {}", e);
            } else {
                println!("Successfully saved {} printers to {}", printers.len(), path.display());
            }
        }
        Err(e) => eprintln!("Failed to serialize printers: {}", e),
    }
}