use serde::Serialize;

/// A known publication is never rolled back after its visibility boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Committed,
    DurabilityUncertain,
}
