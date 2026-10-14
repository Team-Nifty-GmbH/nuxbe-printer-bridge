use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

use crate::utils::config::load_config;

// Helper to create a test config directory
fn setup_test_config_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".config/nuxbe-printer-bridge");
    fs::create_dir_all(&config_path).unwrap();
    (dir, config_path)
}

#[test]
fn test_load_config_default() {
    // Just test that we can load the config
    let config = load_config();
    println!("Loaded config with instance name: {}", config.instance_name);
    assert!(!config.instance_name.is_empty());
    assert!(config.api_port > 0);
    assert!(config.printer_check_interval > 0);
    assert!(config.job_check_interval > 0);
}

#[test]
fn test_temp_dir_creation() {
    // Test that we can create a temporary directory
    // This is useful for future tests that need isolated storage
    let (dir, config_path) = setup_test_config_dir();

    println!(
        "Created temporary config directory: {}",
        config_path.display()
    );
    assert!(config_path.exists());

    // Directory will be automatically cleaned up when 'dir' goes out of scope
    // Just verify it exists now
    assert!(dir.path().exists());
}

#[test]
fn a_config_written_before_mercure_still_loads_with_mercure_off() {
    let config: crate::models::Config = serde_json::from_str(
        r#"{"instance_name":"x","printer_check_interval":5,"job_check_interval":2,
        "flux_url":"https://flux.test","flux_api_token":"t","api_port":8080,
        "reverb_disabled":false,"reverb_app_id":"a","reverb_app_key":"k",
        "reverb_app_secret":"s","reverb_use_tls":true,"reverb_host":"flux.test",
        "reverb_auth_endpoint":"https://flux.test/broadcasting/auth"}"#,
    )
    .unwrap();

    assert!(!config.mercure_enabled);
    assert!(config.mercure_hub_url.is_none());
    assert!(!config.reverb_disabled);
}
