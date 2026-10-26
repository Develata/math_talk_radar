//! CLI subcommands. Each is a thin wrapper over the runtime pipeline or
//! lifecycle manager.
pub mod doctor;
pub mod scan;
pub mod schema;
pub mod sources;
pub mod state;
pub mod uninstall;
pub mod update;
