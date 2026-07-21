use crate::models::Config;
use reqwest::{Client, RequestBuilder};
use std::time::Duration;

/// Build the shared HTTP client with connect/request timeouts so a hung
/// connection to the API cannot stall a background loop indefinitely.
pub fn build_http_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Add authorization header to a request using the API token from config
pub fn with_auth_header(request: RequestBuilder, config: &Config) -> RequestBuilder {
    match &config.flux_api_token {
        Some(token) => request.header("Authorization", format!("Bearer {}", token)),
        None => request,
    }
}
