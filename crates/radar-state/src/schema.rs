//! redb table definitions (§22, ADR-0002). Keys are `&str` (event id, source
//! id); values are `&[u8]` serde_json-serialized domain types. The schema
//! version table stores the persisted `STATE_SCHEMA_VERSION`.

use redb::TableDefinition;

/// Current state DB schema version. Independent of the public JSON
/// `schema_version`. Bumped on any destructive or semantic change to the
/// persisted shape; migrations live in [`super::migrations`].
pub const STATE_SCHEMA_VERSION: u32 = 1;

/// Events keyed by their stable `EventId` string. Value is a serde_json-serialized
/// [`radar_core::Event`] (which carries `first_seen_at` / `last_seen_at`).
pub const EVENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("events");

/// Source health keyed by source id. Value is a serde_json-serialized
/// [`radar_core::SourceHealth`].
pub const SOURCE_HEALTH: TableDefinition<&str, &[u8]> = TableDefinition::new("source_health");

/// Schema version. Single row keyed by `"version"`.
pub const SCHEMA_VERSION: TableDefinition<&str, u32> = TableDefinition::new("schema_version");
