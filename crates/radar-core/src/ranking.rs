//! Score composition (§26). Pure function of event evidence; no I/O.
//!
//! Default score is 0–100. Title-only scholar mentions (e.g. "Gross-Zagier
//! formula", "Deligne periods") must NOT receive the people component. The
//! composition function lands in M1/M4; this module fixes the component shape
//! and per-signal caps.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScoreComponents {
    pub topic: u8,
    pub media: u8,
    pub access: u8,
    pub source_tier: u8,
    pub people: u8,
    pub completeness: u8,
}

// Per-signal caps from §26. Sum = 100.
pub const MAX_TOPIC: u8 = 30;
pub const MAX_MEDIA: u8 = 25;
pub const MAX_ACCESS: u8 = 15;
pub const MAX_SOURCE_TIER: u8 = 10;
pub const MAX_PEOPLE: u8 = 10;
pub const MAX_COMPLETENESS: u8 = 10;
