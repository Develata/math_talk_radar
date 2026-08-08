//! Change-detection events (§23). Emitted by comparing the current scan's
//! canonical fingerprints against the previous state.
use chrono::{DateTime, Utc};
use radar_core::EventId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    EventAdded,
    EventUpdated,
    ScheduleAdded,
    SpeakerAdded,
    LivestreamAdded,
    MediaAdded,
    MediaRemoved,
    EventCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub kind: ChangeKind,
    pub event_id: EventId,
    pub detected_at: DateTime<Utc>,
    #[serde(default)]
    pub detail: Option<String>,
}
