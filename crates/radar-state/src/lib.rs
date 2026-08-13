//! Embedded local state (§22, §23, §65). The `redb`-backed repository, schema,
//! and migrations land in M3. This module establishes the change-event model
//! and the schema-version constant.
#![forbid(unsafe_code)]

pub mod changes;
pub mod migrations;
pub mod repository;
pub mod schema;

pub use changes::{ChangeKind, ChangeRecord, detect_changes};
pub use repository::{CancelledEventTombstone, Repository, StateError};
pub use schema::STATE_SCHEMA_VERSION;
