//! Wall-clock time as Java's `System.currentTimeMillis()` reports it.

/// Milliseconds since the Unix epoch (`System.currentTimeMillis()`), the
/// time base of lease expiries and other timestamps stored in documents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EpochMillis(pub i64);

impl EpochMillis {
    /// `System.currentTimeMillis()`.
    pub fn now() -> EpochMillis {
        EpochMillis(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0))
    }
}

impl From<EpochMillis> for crate::json::Value {
    fn from(value: EpochMillis) -> crate::json::Value {
        crate::json::Value::from(value.0)
    }
}
