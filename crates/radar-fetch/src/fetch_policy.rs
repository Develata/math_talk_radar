//! Per-source fetch policy (Oracle #5: allowlist only).
use radar_core::config::SourceSpec;

#[derive(Debug, Clone)]
pub struct FetchPolicy {
    pub allowed_hosts: Vec<String>,
}

impl From<&SourceSpec> for FetchPolicy {
    fn from(source: &SourceSpec) -> Self {
        if !source.allowed_hosts.is_empty() {
            Self {
                allowed_hosts: source.allowed_hosts.clone(),
            }
        } else if let Some(url) = &source.entrypoint {
            Self {
                allowed_hosts: vec![url.host_str().unwrap_or("").to_string()],
            }
        } else {
            Self {
                allowed_hosts: vec![],
            }
        }
    }
}

impl FetchPolicy {
    pub fn is_host_allowed(&self, url: &url::Url) -> bool {
        if self.allowed_hosts.is_empty() {
            return true;
        }
        url.host_str()
            .map(|h| self.allowed_hosts.iter().any(|a| a == h))
            .unwrap_or(false)
    }
}
