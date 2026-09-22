/// Posting mechanism used to complete a durable CICS event wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsEventPostMode {
    /// Standard MVS-compatible posting, including timer-event completion.
    Standard,
    /// Directly modifying an ECB rather than using the standard post service.
    Hand,
}
