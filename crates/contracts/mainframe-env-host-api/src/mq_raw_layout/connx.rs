//! Source-reviewed default-only CONNX input translation. No connection execution.

use super::{MqRawCapture, MqRawFieldValue, MqRawLayoutKind, MqRawProblem, generated};
use crate::{
    MqHandleSharing,
    mq_mqi::{MqMqiConnect, MqMqiOptions},
    mq_object_route::MqRouteName,
};

#[cfg(test)]
mod tests;

/// An independently selected embedding profile, never decoded from application
/// options. It remains an assertion until compared with trusted host admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqConnxProfile {
    /// Ordinary owned connection: default binding, non-MTS, no client fallback,
    /// implicit CICS connection, or external binding/security configuration.
    OrdinaryOwnedNonshared,
    MtsSharingDefaultPending,
    ClientOrFallbackPending,
    ImplicitConnectionPending,
    BindingOrSecurityPending,
    UnknownPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqConnxProblem {
    Raw(MqRawProblem),
    WrongLayout,
    NumericRange,
    UnknownBits,
    UnsupportedOptions,
    ProfilePending,
}

/// Includes rejected identities and zero aliases; this list grants no support.
pub fn mq_connx_numeric_identities() -> &'static [(&'static str, i32)] {
    generated::MQCNO_NUMERIC_IDENTITIES
}

impl MqRawCapture {
    /// Produces only existing default/nonshared semantics, retaining the raw
    /// capture separately. A caller must still supply real host/service admission.
    /// This cannot choose topology, authorize connection or create a handle.
    pub fn decode_connx_default(
        &self,
        profile: MqConnxProfile,
        manager: Option<MqRouteName>,
    ) -> Result<MqMqiConnect, MqConnxProblem> {
        if self.layout().kind != MqRawLayoutKind::Cno1 {
            return Err(MqConnxProblem::WrongLayout);
        }
        let MqRawFieldValue::Long(options) = self.field("Options").map_err(MqConnxProblem::Raw)?
        else {
            return Err(MqConnxProblem::WrongLayout);
        };
        checked_options(i64::from(options))?;
        if profile != MqConnxProfile::OrdinaryOwnedNonshared {
            return Err(MqConnxProblem::ProfilePending);
        }
        Ok(MqMqiConnect {
            manager,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        })
    }
}

fn checked_options(number: i64) -> Result<(), MqConnxProblem> {
    let options = super::mq_raw_cobol_long(number).map_err(|_| MqConnxProblem::NumericRange)?;
    if options < 0 {
        return Err(MqConnxProblem::NumericRange);
    }
    if options & !generated::MQCNO_KNOWN != 0 {
        return Err(MqConnxProblem::UnknownBits);
    }
    if !generated::MQCNO_ADMITTED.contains(&options) {
        return Err(MqConnxProblem::UnsupportedOptions);
    }
    Ok(())
}
