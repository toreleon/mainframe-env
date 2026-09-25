//! Extraction-specific APPC basic return-code projections.
//!
//! The six-byte return-code type and the other GDS command tables are owned
//! by the shared conversation-control handler.

use crate::service::GdsReturnCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsExtractAttributesFailure {
    NotAppc,
    DplPrincipal,
    NotBasic,
    NotOwned,
}

impl GdsExtractAttributesFailure {
    #[must_use]
    pub const fn retcode(self) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::NotAppc => [0x03, 0, 0, 0, 0, 0],
            Self::DplPrincipal => [0x03, 0x01, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::NotOwned => [0x04, 0, 0, 0, 0, 0],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsExtractProcessFailure {
    NotAppcOrPrincipal,
    NotBasic,
    NotOwned,
    ProcessTooLong,
}

impl GdsExtractProcessFailure {
    #[must_use]
    pub const fn retcode(self) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::NotAppcOrPrincipal => [0x03, 0, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::NotOwned => [0x04, 0, 0, 0, 0, 0],
            Self::ProcessTooLong => [0x05, 0, 0, 0, 0, 0x20],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_returns_exact_six_byte_codes() {
        assert_eq!(
            GdsExtractAttributesFailure::DplPrincipal.retcode().0,
            [3, 1, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsExtractProcessFailure::ProcessTooLong.retcode().0,
            [5, 0, 0, 0, 0, 32]
        );
        assert_eq!(GdsReturnCode::NORMAL.0, [0; 6]);
    }
}
