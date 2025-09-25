use crate::models::Config;
use reqwest::RequestBuilder;

/// Add authorization header to a request using the API token from config
pub fn with_auth_header(request: RequestBuilder, config: &Config) -> RequestBuilder {
    request.header(
        "Authorization",
        format!(
            "Bearer {}",
            config.flux_api_token.as_ref().unwrap_or(&String::new())
        ),
    )
}
