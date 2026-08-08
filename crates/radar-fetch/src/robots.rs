//! Robots policy (§16). `respect_robots = true` is the only mode; no bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RobotsPolicy {
    #[default]
    Respect,
}

impl RobotsPolicy {
    pub fn is_respected(self) -> bool {
        matches!(self, RobotsPolicy::Respect)
    }
}
