//! HTTP fetch layer (§15, §16). Produces `FetchedDocument` for adapters.
//!
//! The real async client (concurrency, per-host limits, retry with backoff,
//! robots check, global deadline, response-body cap) lands in M2. This module
//! establishes the policy shape, retry decision logic, and client builder.
#![forbid(unsafe_code)]

pub mod budget;
pub mod client;
pub mod engine;
pub mod error;
pub mod fetch_policy;
pub mod policy;
pub mod retry;
pub mod robots;

pub use budget::RequestBudget;
pub use client::{FetchBuildError, FetchClient};
pub use engine::{SourceFetchResult, fetch_all, fetch_source, past_deadline};
pub use error::FetchError;
pub use fetch_policy::FetchPolicy;
pub use policy::HttpPolicy;
pub use retry::{RetryDecision, is_transient_network_error, retry_for_status};
pub use robots::{RobotsCache, RobotsPolicy};
