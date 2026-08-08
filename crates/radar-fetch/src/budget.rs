//! Per-source request budget (§14 crawl boundary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestBudget {
    pub max_depth: u8,
    pub request_budget: u32,
    pub remaining: u32,
}

impl Default for RequestBudget {
    fn default() -> Self {
        Self {
            max_depth: 2,
            request_budget: 20,
            remaining: 20,
        }
    }
}

impl RequestBudget {
    pub fn try_consume(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}
