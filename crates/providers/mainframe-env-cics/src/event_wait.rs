/// Posting mechanism used to complete a durable CICS event wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsEventPostMode {
    /// Standard MVS-compatible posting, including timer-event completion.
    Standard,
    /// Directly modifying an ECB rather than using the standard post service.
    Hand,
}

/// Task-purge cause applied while a durable CICS event wait is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsEventPurgeMode {
    /// The task's deadlock timeout expired.
    DeadlockTimeout,
    /// An ordinary task purge was requested.
    Purge,
    /// A force-purge was requested and cannot be suppressed by the wait.
    ForcePurge,
}
