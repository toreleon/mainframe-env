//! Exact DFHCDBLK indicator layout used by GDS EXTRACT ATTRIBUTES.

use serde::{Deserialize, Serialize};

/// Typed fields of the 24-byte basic-conversation CONVDATA area.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationIndicators {
    pub complete: bool,
    pub sync_required: bool,
    pub free_required: bool,
    pub receive_required: bool,
    pub signal_received: bool,
    pub confirm_received: bool,
    pub error_received: bool,
    pub error_code: [u8; 4],
    pub rollback_required: bool,
}

impl ConversationIndicators {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// CDBCOMPL through CDBSYNRB; the final twelve reserved bytes are zero.
    #[must_use]
    pub fn convdata(self) -> [u8; 24] {
        let mut bytes = [0; 24];
        for (index, flag) in [
            self.complete,
            self.sync_required,
            self.free_required,
            self.receive_required,
            self.signal_received,
            self.confirm_received,
            self.error_received,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index] = if flag { 0xff } else { 0 };
        }
        bytes[7..11].copy_from_slice(&self.error_code);
        bytes[11] = if self.rollback_required { 0xff } else { 0 };
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convdata_is_exactly_24_bytes_with_reserved_zero_tail() {
        let indicators = ConversationIndicators {
            complete: true,
            free_required: true,
            error_received: true,
            error_code: [0x08, 0x89, 0x00, 0x00],
            rollback_required: true,
            ..Default::default()
        };
        assert_eq!(
            indicators.convdata(),
            [
                0xff, 0, 0xff, 0, 0, 0, 0xff, 0x08, 0x89, 0x00, 0x00, 0xff, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ]
        );
    }
}
