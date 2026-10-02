//! Source-reviewed call-return vocabulary, without runtime outcome calculation.
//!
//! Only the pinned call page's explicit Reason/CompCode groups admit a pair.
//! Host applicability remains the trusted context authority's responsibility;
//! context locators here record review provenance, not dispatch permission.
//! Callback notification has no ordinary call-return pair. Contradictory or
//! unnumbered declarations remain visible as pending and cannot be constructed.
//! Completion classes are symbolic: these call pages do not number MQCC values.

use crate::mq_mqi::MqMqiCall;

mod encoding;
mod generated;
#[cfg(test)]
mod tests;

pub use generated::{MQ_STATUS_CATALOG_SHA256, MQ_STATUS_PAIR_COUNT, MQ_STATUS_PENDING_COUNT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqCompletion {
    Ok,
    Warning,
    Failed,
}

impl MqCompletion {
    pub fn from_symbol(symbol: &str) -> Result<Self, MqStatusProblem> {
        generated::COMPLETIONS
            .iter()
            .find(|(_, name)| *name == symbol)
            .map(|(completion, _)| *completion)
            .ok_or(MqStatusProblem::UnknownCompletionSymbol)
    }

    pub fn symbol(self) -> &'static str {
        generated::completion_symbol(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqStatusReview {
    Admitted,
    PendingNumber,
    PendingNumericConflict,
    PendingSymbolConflict,
    PendingSymbolSpelling,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqStatusSourceLocation {
    pub completion_line: u16,
    pub reason_line: u16,
    pub number_line: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqStatusContextNote {
    pub kind: &'static str,
    pub first_line: u16,
    pub last_line: u16,
    pub fragment_sha256: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqStatusPairDescriptor {
    pub completion: MqCompletion,
    /// Exact source spelling, including a malformed spelling retained as pending.
    pub reason_symbol: &'static str,
    pub declared_decimal: Option<i32>,
    pub declared_hex: Option<&'static str>,
    pub review: MqStatusReview,
    pub source_locations: &'static [MqStatusSourceLocation],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqStatusCallDescriptor {
    pub call: MqMqiCall,
    pub has_call_return: bool,
    pub reason_section: Option<(u16, u16)>,
    pub context_notes: &'static [MqStatusContextNote],
    pub reviewed_projection_sha256: &'static str,
    pair_blocks: &'static [&'static [MqStatusPairDescriptor]],
}

impl MqStatusCallDescriptor {
    /// Includes pending declarations; inspect `review` before using identities.
    pub fn pairs(&self) -> impl Iterator<Item = &'static MqStatusPairDescriptor> {
        self.pair_blocks.iter().flat_map(|block| block.iter())
    }
}

pub fn mq_status_calls() -> &'static [MqStatusCallDescriptor] {
    &generated::CALLS
}

pub fn mq_status_call(call: MqMqiCall) -> &'static MqStatusCallDescriptor {
    generated::descriptor(call)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqStatusProblem {
    NoCallReturn,
    UnknownCompletionSymbol,
    UnknownPair,
    PendingSource(MqStatusReview),
    NumericMismatch,
    AmbiguousReasonIdentity,
}

/// An observed return identity admitted by the reviewed call-specific table.
/// Private fields prevent arbitrary integer bags and forged descriptor admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqReviewedStatus {
    call: MqMqiCall,
    pair: &'static MqStatusPairDescriptor,
}

impl MqReviewedStatus {
    pub fn from_symbols(
        call: MqMqiCall,
        completion: &str,
        reason: &str,
    ) -> Result<Self, MqStatusProblem> {
        let descriptor = mq_status_call(call);
        if !descriptor.has_call_return {
            return Err(MqStatusProblem::NoCallReturn);
        }
        let completion = MqCompletion::from_symbol(completion)?;
        let pair = descriptor
            .pairs()
            .find(|pair| pair.completion == completion && pair.reason_symbol == reason)
            .ok_or(MqStatusProblem::UnknownPair)?;
        if pair.review != MqStatusReview::Admitted {
            return Err(MqStatusProblem::PendingSource(pair.review));
        }
        Ok(Self { call, pair })
    }

    /// Both numeric spellings must agree with the admitted symbolic identity.
    pub fn from_identity(
        call: MqMqiCall,
        completion: &str,
        reason: &str,
        decimal: i32,
        hexadecimal: u32,
    ) -> Result<Self, MqStatusProblem> {
        let status = Self::from_symbols(call, completion, reason)?;
        if status.reason_decimal() != decimal || decimal < 0 || decimal as u32 != hexadecimal {
            return Err(MqStatusProblem::NumericMismatch);
        }
        Ok(status)
    }

    /// Aliases sharing a number require the explicit symbolic constructor.
    /// A number mentioned by an unresolved declaration also fails closed.
    pub fn from_reason_number(
        call: MqMqiCall,
        completion: MqCompletion,
        number: i32,
    ) -> Result<Self, MqStatusProblem> {
        let descriptor = mq_status_call(call);
        if !descriptor.has_call_return {
            return Err(MqStatusProblem::NoCallReturn);
        }
        let mut matched = None;
        for pair in descriptor
            .pairs()
            .filter(|pair| pair.completion == completion)
        {
            let matches = pair.declared_decimal == Some(number)
                || pair.declared_hex.is_some_and(|hex| {
                    u32::from_str_radix(hex, 16).ok() == u32::try_from(number).ok()
                });
            if !matches {
                continue;
            }
            if pair.review != MqStatusReview::Admitted {
                return Err(MqStatusProblem::PendingSource(pair.review));
            }
            if matched.replace(pair).is_some() {
                return Err(MqStatusProblem::AmbiguousReasonIdentity);
            }
        }
        matched
            .map(|pair| Self { call, pair })
            .ok_or(MqStatusProblem::UnknownPair)
    }

    pub fn call(self) -> MqMqiCall {
        self.call
    }

    pub fn identity(self) -> &'static MqStatusPairDescriptor {
        self.pair
    }

    pub fn completion(self) -> MqCompletion {
        self.pair.completion
    }

    pub fn reason_symbol(self) -> &'static str {
        self.pair.reason_symbol
    }

    pub fn reason_decimal(self) -> i32 {
        self.pair
            .declared_decimal
            .expect("admitted reason is numbered")
    }

    pub fn reason_hex(self) -> &'static str {
        self.pair
            .declared_hex
            .expect("admitted reason has verified hex")
    }
}
