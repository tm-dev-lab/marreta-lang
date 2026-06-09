use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use super::driver::{
    HttpClient, HttpClientDriverError, HttpClientResult, HttpMethod, HttpRequest, HttpResponse,
};
use crate::value::{Value, json_to_value, value_to_json};

/// reqwest-based HTTP client driver.
/// Created once at startup — `reqwest::Client` internally uses `Arc` for connection pool sharing.
pub struct ReqwestDriver {
    client: reqwest::Client,
    default_timeout: Duration,
}

impl ReqwestDriver {
    /// Creates a new driver with the given default timeout.
    /// Max redirects hardcoded to 10 (API-to-API communication doesn't redirect).
    pub fn new(default_timeout: Duration) -> HttpClientResult<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| {
                HttpClientDriverError::RequestFailed(format!("failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            client,
            default_timeout,
        })
    }
}

#[async_trait]
impl HttpClient for ReqwestDriver {
    async fn execute(&self, request: HttpRequest) -> HttpClientResult<HttpResponse> {
        // Build the reqwest request
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };

        let timeout = request.timeout.unwrap_or(self.default_timeout);

        // Build URL with query params
        let url = if !request.query.is_empty() {
            let qs = serde_urlencoded::to_string(&request.query).unwrap_or_default();
            if request.url.contains('?') {
                format!("{}&{}", request.url, qs)
            } else {
                format!("{}?{}", request.url, qs)
            }
        } else {
            request.url.clone()
        };

        let mut builder = self.client.request(method, &url).timeout(timeout);

        // Add headers
        for (key, value) in &request.headers {
            builder = builder.header(key, value);
        }

        // Add body (serialize Value to JSON)
        if let Some(body) = &request.body {
            let json = value_to_json(body);
            builder = builder.json(&json);
        }

        // Execute
        let response = builder.send().await.map_err(|e| {
            if e.is_timeout() {
                HttpClientDriverError::Timeout(format!("request to {} timed out", request.url))
            } else if e.is_connect() {
                HttpClientDriverError::ConnectionFailed(format!(
                    "connection to {} failed: {}",
                    request.url, e
                ))
            } else if e.is_redirect() {
                HttpClientDriverError::RequestFailed(format!(
                    "too many redirects for {}",
                    request.url
                ))
            } else {
                HttpClientDriverError::RequestFailed(format!(
                    "request to {} failed: {}",
                    request.url, e
                ))
            }
        })?;

        let status = response.status().as_u16();

        // Collect response headers (lowercase keys)
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_lowercase(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();

        // Read body text
        let body_text = response.text().await.map_err(|e| {
            HttpClientDriverError::RequestFailed(format!("failed to read response body: {}", e))
        })?;

        // Auto-detect JSON: try parse, fallback to raw string
        let body = match serde_json::from_str::<serde_json::Value>(&body_text) {
            Ok(json) => json_to_value(&json),
            Err(_) => Value::String(body_text),
        };

        Ok(HttpResponse {
            status,
            body,
            headers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reqwest_driver_creation() {
        let driver = ReqwestDriver::new(Duration::from_secs(30));
        assert!(driver.is_ok());
    }

    #[test]
    fn test_reqwest_driver_custom_timeout() {
        let driver = ReqwestDriver::new(Duration::from_millis(500)).unwrap();
        assert_eq!(driver.default_timeout, Duration::from_millis(500));
    }
}
