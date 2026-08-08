//! Topic model (§7). MVP uses canonical topic + aliases + phrases, no semantic
//! model. User interest weights alter ranking only; they never delete events.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopicMatch {
    pub topic_id: String,
    pub canonical_name: String,
    pub matched_text: String,
    pub confidence: f32,
}

/// A topic entry loaded from `config/topics.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}
