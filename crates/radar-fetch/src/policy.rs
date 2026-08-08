//! HTTP policy defaults (§15).
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct HttpPolicy {
    pub global_concurrency: usize,
    pub per_host_concurrency: usize,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub global_scan_deadline: Duration,
    pub redirect_limit: usize,
    pub max_retry: u32,
    pub max_response_body: usize,
}

impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            global_concurrency: 8,
            per_host_concurrency: 2,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(15),
            global_scan_deadline: Duration::from_secs(30),
            redirect_limit: 5,
            max_retry: 1,
            max_response_body: 4 * 1024 * 1024,
        }
    }
}
