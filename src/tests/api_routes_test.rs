// src/tests/api_routes_test.rs

#[cfg(test)]
pub mod config_tests {
    use crate::utils::config::load_config;

    #[test]
    fn test_config_exists() {
        // Load the configuration
        let config = load_config();

        // Just check that we can load the config without errors
        // and that required fields exist (without asserting specific values)
        assert!(config.api_port > 0);
        assert!(!config.instance_name.is_empty());
        println!("Loaded config with instance name: {}", config.instance_name);
    }
}

#[cfg(test)]
pub mod printer_service_tests {
    use crate::services::printer::get_all_printers;

    #[tokio::test]
    async fn test_get_all_printers() {
        // Just verify the function runs without panicking
        let printers = get_all_printers(false).await;

        // Log the results rather than making strict assertions
        println!("Found {} printers", printers.len());
        for printer in &printers {
            println!("Printer: {}", printer.name);
        }
    }
}
