//! State schema version (§65). Bumped on any destructive or semantic change to
//! the persisted shape. Migrations live in [`super::migrations`].

/// Current state DB schema version. Independent of the public JSON
/// `schema_version`.
pub const STATE_SCHEMA_VERSION: u32 = 1;
