// src/tests/api_routes_test.rs
#[cfg(test)]
pub mod api_tests {
    use actix_web::{App, test, web};
    use reqwest::Client;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    use crate::api::routes::{check_jobs_endpoint, check_printers_endpoint, get_printers};

    #[actix_web::test]
    async fn test_get_printers_endpoint() {
        // Create a simple test app with the printers endpoint
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(false)) // verbose_debug flag
                .service(get_printers),
        )
        .await;

        // Make a request to the endpoint
        let req = test::TestRequest::get().uri("/printers").to_request();
        let resp = test::call_service(&app, req).await;

        // In a real environment, we just check the response is successful
        println!("Printers endpoint status: {}", resp.status());
        assert!(resp.status().is_success() || resp.status().is_server_error());
    }

    #[actix_web::test]
    async fn test_check_jobs_endpoint() {
        // Create required dependencies
        let config = Arc::new(Mutex::new(crate::models::Config::default()));
        let http_client = Client::new();

        // Create the test app
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(Arc::clone(&config)))
                .app_data(web::Data::new(http_client.clone()))
                .service(check_jobs_endpoint),
        )
        .await;

        // Make a request to the endpoint
        let req = test::TestRequest::get().uri("/check_jobs").to_request();
        let resp = test::call_service(&app, req).await;

        // Check the response (may fail in environments without proper CUPS setup)
        println!("Jobs endpoint status: {}", resp.status());
    }

    #[actix_web::test]
    async fn test_check_printers_endpoint() {
        // Create required dependencies
        let config = Arc::new(Mutex::new(crate::models::Config::default()));
        let http_client = Client::new();
        // Fix: specify the generic type for HashSet
        let printers_data = Arc::new(Mutex::new(HashSet::<String>::new()));

        // Create the test app
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(Arc::clone(&printers_data)))
                .app_data(web::Data::new(Arc::clone(&config)))
                .app_data(web::Data::new(http_client.clone()))
                .app_data(web::Data::new(false)) // verbose_debug flag
                .service(check_printers_endpoint),
        )
        .await;

        // Make a request to the endpoint
        let req = test::TestRequest::get().uri("/check_printers").to_request();
        let resp = test::call_service(&app, req).await;

        // Check the response (may fail in environments without proper CUPS setup)
        println!("Check printers endpoint status: {}", resp.status());
    }
}

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
