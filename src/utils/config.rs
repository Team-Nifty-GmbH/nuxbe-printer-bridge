use crate::models::Config;
use std::fs;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};

/// Clone config from a shared RwLock
pub fn read_config(config: &Arc<RwLock<Config>>) -> Config {
    config
        .read()
        .expect("Failed to acquire config read lock")
        .clone()
}

/// Path to the config directory
pub fn config_dir() -> std::path::PathBuf {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    home_dir.join(".config/nuxbe-printer-bridge")
}

/// Path to the config file
fn config_path() -> std::path::PathBuf {
    config_dir().join("config.json")
}

/// Load configuration from file or create default if it doesn't exist
pub fn load_config() -> Config {
    let config_dir = config_dir();
    let config_path = config_path();

    // create_dir_all is idempotent - no need to check existence first
    fs::create_dir_all(&config_dir).expect("Failed to create config directory");

    match fs::read_to_string(&config_path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            warn!(error = %e, "Error parsing config file, using default configuration");
            let default_config = Config::default();
            save_config(&default_config);
            default_config
        }),
        Err(_) => {
            debug!("Config file not found, creating with default values");
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

    // create_dir_all is idempotent - no need to check existence first
    if let Err(e) = fs::create_dir_all(&config_dir) {
        warn!(error = %e, "Failed to create config directory");
        return;
    }

    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(e) = fs::write(&config_path, json) {
                warn!(error = %e, "Failed to save config file");
            }
        }
        Err(e) => warn!(error = %e, "Failed to serialize config"),
    }
}
