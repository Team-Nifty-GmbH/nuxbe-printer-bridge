use std::fs;
use crate::models::Config;

/// Path to the config directory
fn config_dir() -> std::path::PathBuf {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    home_dir.join(".config/flux-spooler")
}

/// Path to the config file
fn config_path() -> std::path::PathBuf {
    config_dir().join("config.json")
}

/// Load configuration from file or create default if it doesn't exist
pub fn load_config() -> Config {
    let config_dir = config_dir();
    let config_path = config_path();

    // Create the directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    match fs::read_to_string(&config_path) {
        Ok(contents) => {
            serde_json::from_str(&contents).unwrap_or_else(|e| {
                eprintln!(
                    "Error parsing config file: {}. Using default configuration.",
                    e
                );
                let default_config = Config::default();
                // Save the default config
                save_config(&default_config);
                default_config
            })
        }
        Err(_) => {
            println!("Config file not found. Creating with default values.");
            let default_config = Config::default();
            save_config(&default_config);
            default_config
        }
    }
}

/// Save configuration to file
pub fn save_config(config: &Config) {
    let config_dir = config_dir();
    let config_path = config_path();

    // Create the directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(e) = fs::write(&config_path, json) {
                eprintln!("Failed to save config file: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to serialize config: {}", e),
    }
}