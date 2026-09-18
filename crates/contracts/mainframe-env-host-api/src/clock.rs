#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockRequest {
    UtcTimestamp,
    Date,
    Time,
}
