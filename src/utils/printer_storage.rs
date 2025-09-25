use crate::models::Printer;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
            eprintln!("Error parsing printers file: {}. Using empty list.", e);
            HashMap::new()
        }),
        Err(_) => {
            println!("Printers file not found. Starting with empty list.");
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
                if current_printer.description != saved_printer.description
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

    // Ensure the config directory exists
    let config_dir = crate::utils::config::config_dir();
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    match serde_json::to_string_pretty(printers) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("Failed to save printers file: {}", e);
            } else {
                eprintln!(
                    "Successfully saved {} printers to {}",
                    printers.len(),
                    path.display()
                );
            }
        }
        Err(e) => eprintln!("Failed to serialize printers: {}", e),
    }
}
