#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Clock value requested from the selected host clock provider.
pub enum ClockRequest {
    /// Request a UTC timestamp string.
    UtcTimestamp,
    /// Request the provider's date string.
    Date,
    /// Request the provider's time string.
    Time,
}
