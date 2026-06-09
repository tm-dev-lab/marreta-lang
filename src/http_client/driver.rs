use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::value::Value;

// --- Error -------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum HttpClientDriverError {
    ConnectionFailed(String),
    Timeout(String),
    TlsError(String),
    InvalidUrl(String),
    RequestFailed(String),
}

impl std::fmt::Display for HttpClientDriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(m) => write!(f, "connection failed: {}", m),
            Self::Timeout(m) => write!(f, "request timeout: {}", m),
            Self::TlsError(m) => write!(f, "TLS error: {}", m),
            Self::InvalidUrl(m) => write!(f, "invalid URL: {}", m),
            Self::RequestFailed(m) => write!(f, "request failed: {}", m),
        }
    }
}

pub type HttpClientResult<T> = Result<T, HttpClientDriverError>;

// --- Types -------------------------------------------------------------------

/// HTTP method for outbound requests.
#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Patch => write!(f, "PATCH"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

/// Outbound HTTP request passed to the driver.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub body: Option<Value>,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub timeout: Option<Duration>,
}

/// HTTP response returned by the driver.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Value,
    pub headers: HashMap<String, String>,
}

// --- HttpClient trait --------------------------------------------------------

/// Abstraction over HTTP client implementations.
/// Single `execute()` method — all five verbs share the same signature.
/// Implementations: ReqwestDriver (v0.10). MockHttpClient (tests).
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> HttpClientResult<HttpResponse>;
}

// --- Mock driver for unit tests ----------------------------------------------

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Mutex;

    /// A mock HTTP client that returns pre-configured responses.
    /// Used in interpreter unit tests to avoid real network calls.
    #[derive(Debug)]
    pub struct MockHttpClient {
        /// Queue of responses to return (FIFO).
        pub responses: Mutex<Vec<HttpClientResult<HttpResponse>>>,
        /// Record of requests that were sent.
        pub requests: Mutex<Vec<HttpRequest>>,
    }

    impl MockHttpClient {
        pub fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                responses: Mutex::new(Vec::new()),
                requests: Mutex::new(Vec::new()),
            })
        }

        /// Enqueue a successful response to be returned on the next `execute()` call.
        pub fn enqueue_response(&self, response: HttpResponse) {
            self.responses.lock().unwrap().push(Ok(response));
        }

        /// Enqueue an error to be returned on the next `execute()` call.
        pub fn enqueue_error(&self, error: HttpClientDriverError) {
            self.responses.lock().unwrap().push(Err(error));
        }

        /// Returns the list of requests that were captured.
        pub fn captured_requests(&self) -> Vec<HttpRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl HttpClient for MockHttpClient {
        async fn execute(&self, request: HttpRequest) -> HttpClientResult<HttpResponse> {
            self.requests.lock().unwrap().push(request);
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(HttpClientDriverError::RequestFailed(
                    "mock: no responses enqueued".into(),
                ))
            } else {
                responses.remove(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_display() {
        assert_eq!(format!("{}", HttpMethod::Get), "GET");
        assert_eq!(format!("{}", HttpMethod::Post), "POST");
        assert_eq!(format!("{}", HttpMethod::Put), "PUT");
        assert_eq!(format!("{}", HttpMethod::Patch), "PATCH");
        assert_eq!(format!("{}", HttpMethod::Delete), "DELETE");
    }

    #[test]
    fn test_http_method_equality() {
        assert_eq!(HttpMethod::Get, HttpMethod::Get);
        assert_ne!(HttpMethod::Get, HttpMethod::Post);
    }

    #[test]
    fn test_driver_error_display() {
        let err = HttpClientDriverError::ConnectionFailed("refused".into());
        assert_eq!(format!("{}", err), "connection failed: refused");

        let err = HttpClientDriverError::Timeout("5s exceeded".into());
        assert_eq!(format!("{}", err), "request timeout: 5s exceeded");

        let err = HttpClientDriverError::TlsError("cert expired".into());
        assert_eq!(format!("{}", err), "TLS error: cert expired");

        let err = HttpClientDriverError::InvalidUrl("not a url".into());
        assert_eq!(format!("{}", err), "invalid URL: not a url");

        let err = HttpClientDriverError::RequestFailed("unknown".into());
        assert_eq!(format!("{}", err), "request failed: unknown");
    }

    #[tokio::test]
    async fn test_mock_returns_enqueued_response() {
        let mock = mock::MockHttpClient::new();
        mock.enqueue_response(HttpResponse {
            status: 200,
            body: Value::String("ok".into()),
            headers: HashMap::new(),
        });

        let req = HttpRequest {
            method: HttpMethod::Get,
            url: "https://example.com".into(),
            body: None,
            headers: HashMap::new(),
            query: HashMap::new(),
            timeout: None,
        };

        let resp = mock.execute(req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, Value::String("ok".into()));
    }

    #[tokio::test]
    async fn test_mock_captures_requests() {
        let mock = mock::MockHttpClient::new();
        mock.enqueue_response(HttpResponse {
            status: 201,
            body: Value::Null,
            headers: HashMap::new(),
        });

        let req = HttpRequest {
            method: HttpMethod::Post,
            url: "https://api.example.com/orders".into(),
            body: Some(Value::String("payload".into())),
            headers: HashMap::from([("Authorization".into(), "Bearer tok".into())]),
            query: HashMap::new(),
            timeout: Some(Duration::from_secs(5)),
        };

        let _ = mock.execute(req).await;
        let captured = mock.captured_requests();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, HttpMethod::Post);
        assert_eq!(captured[0].url, "https://api.example.com/orders");
    }

    #[tokio::test]
    async fn test_mock_returns_error_when_enqueued() {
        let mock = mock::MockHttpClient::new();
        mock.enqueue_error(HttpClientDriverError::Timeout("10s".into()));

        let req = HttpRequest {
            method: HttpMethod::Get,
            url: "https://slow.example.com".into(),
            body: None,
            headers: HashMap::new(),
            query: HashMap::new(),
            timeout: None,
        };

        let result = mock.execute(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_errors_when_no_responses_enqueued() {
        let mock = mock::MockHttpClient::new();

        let req = HttpRequest {
            method: HttpMethod::Get,
            url: "https://example.com".into(),
            body: None,
            headers: HashMap::new(),
            query: HashMap::new(),
            timeout: None,
        };

        let result = mock.execute(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_fifo_order() {
        let mock = mock::MockHttpClient::new();
        mock.enqueue_response(HttpResponse {
            status: 200,
            body: Value::String("first".into()),
            headers: HashMap::new(),
        });
        mock.enqueue_response(HttpResponse {
            status: 201,
            body: Value::String("second".into()),
            headers: HashMap::new(),
        });

        let req = HttpRequest {
            method: HttpMethod::Get,
            url: "https://example.com".into(),
            body: None,
            headers: HashMap::new(),
            query: HashMap::new(),
            timeout: None,
        };

        let r1 = mock.execute(req.clone()).await.unwrap();
        assert_eq!(r1.status, 200);

        let r2 = mock.execute(req).await.unwrap();
        assert_eq!(r2.status, 201);
    }
}
