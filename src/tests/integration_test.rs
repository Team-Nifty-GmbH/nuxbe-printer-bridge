use crate::utils::config::load_config;

// This test verifies the config loading logic
#[tokio::test]
async fn test_config_integration() {
    // Just load the config without modifying it
    let config = load_config();
    println!("Loaded config with instance name: {}", config.instance_name);
    assert!(!config.instance_name.is_empty());
}
