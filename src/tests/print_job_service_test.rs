use crate::models::Config;

#[tokio::test]
async fn test_config_setup() {
    // Create test configuration
    let config = Config {
        instance_name: "test-instance".to_string(),
        flux_api_token: Some("test-token".to_string()),
        ..Config::default()
    };

    // Test setup is correct
    assert_eq!(config.instance_name, "test-instance");
    assert!(config.flux_api_token.is_some());
}

#[tokio::test]
async fn test_fetch_jobs_setup() {
    // Create test configuration
    let config = Config {
        instance_name: "test-instance".to_string(),
        flux_url: "http://test-server".to_string(),
        flux_api_token: Some("test-token".to_string()),
        ..Config::default()
    };

    // Test that the config is set up correctly
    assert_eq!(config.instance_name, "test-instance");
    assert_eq!(config.flux_url, "http://test-server");
    assert!(config.flux_api_token.is_some());
}
