//! Exact IMS PCB status lookup and PCB-kind applicability contracts.

use crate::ImsPcbKind;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Status lookup domain; the same bytes can have different applicability across contexts.
pub enum ImsStatusContext {
    /// Database call status vocabulary.
    Database,
    /// System service call status vocabulary.
    SystemService,
    /// Message call status vocabulary.
    Message,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Reviewed classification allowing overlapping categories; completion does not always mean returned data.
pub enum ImsStatusCategory {
    /// Exceptional but valid completed call.
    ExceptionalValidCompleted,
    /// Completed warning with data.
    WarningWithDataCompleted,
    /// Completed warning without data.
    WarningNoDataCompleted,
    /// Application operand/specification failure.
    ImproperUserSpecification,
    /// System, I/O or security failure classification.
    SystemIoSecurityError,
    /// Requested data is unavailable.
    UnavailableData,
    /// Lock timeout classification, distinct from generic no-data.
    LockTimeout,
}

impl ImsStatusCategory {
    #[must_use]
    /// Report completion for the three completed categories, including warnings with or without data; no generic success is inferred.
    pub const fn call_completed(self) -> bool {
        matches!(
            self,
            Self::ExceptionalValidCompleted
                | Self::WarningWithDataCompleted
                | Self::WarningNoDataCompleted
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Exact two-byte status and all retained categories, including warnings and exceptional completion.
pub struct ImsStatusDescriptor {
    /// Exact two-byte display status, including two blanks for success.
    pub code: [u8; 2],
    /// All applicable categories; a warning can also classify unavailable data.
    pub categories: &'static [ImsStatusCategory],
}

impl ImsStatusDescriptor {
    #[must_use]
    /// Test exact two-space success bytes without collapsing other completed statuses.
    pub fn is_blank_success(self) -> bool {
        self.code == *b"  "
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source-pinned context vocabulary and permitted PCB kinds, independent of runtime outcome calculation.
pub struct ImsStatusContextDescriptor {
    /// Closed status lookup domain.
    pub context: ImsStatusContext,
    /// Pinned publication topic locator; metadata presence earns no execution credit.
    pub source_topic: &'static str,
    /// Expected source-body SHA-256, binding the descriptor to retained source bytes.
    pub source_sha256: &'static str,
    /// Exact PCB families allowed for this domain.
    pub pcb_kinds: &'static [ImsPcbKind],
    /// Sorted unique status descriptors for this domain.
    pub statuses: &'static [ImsStatusDescriptor],
}

include!("generated/ims_status_codes.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Syntactically valid two-byte status; construction does not establish catalog membership.
pub struct ImsStatusCode([u8; 2]);

impl ImsStatusCode {
    /// Exact two-space status identity; distinct from other completed/warning codes.
    pub const BLANK_SUCCESS: Self = Self(*b"  ");

    /// Admit exactly two blanks or two uppercase ASCII alphanumeric bytes; catalog/context membership is checked separately.
    pub fn parse(input: &[u8]) -> Result<Self, ImsStatusProblem> {
        let [left, right] = input else {
            return Err(ImsStatusProblem::MalformedCode);
        };
        let bytes = [*left, *right];
        if bytes == *b"  "
            || bytes
                .iter()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            Ok(Self(bytes))
        } else {
            Err(ImsStatusProblem::MalformedCode)
        }
    }

    #[must_use]
    /// Return the exact admitted status bytes, including blank padding.
    pub const fn bytes(self) -> [u8; 2] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Status syntax, vocabulary or context/PCB refusal, kept distinct from application completion.
pub enum ImsStatusProblem {
    /// Bytes are not an admitted two-byte status syntax.
    MalformedCode,
    /// Syntactically valid code is absent from all generated contexts.
    UnknownCode,
    /// Known code exists only in a different context.
    ForbiddenStatusContext,
    /// The context does not permit this PCB family.
    ForbiddenPcbKind,
}

#[must_use]
/// Borrow the complete generated context registry, not runtime outcomes.
pub fn ims_status_contexts() -> &'static [ImsStatusContextDescriptor] {
    IMS_STATUS_CONTEXTS
}

#[must_use]
/// Select the generated closed context; panics only if generated coverage is incomplete.
pub fn ims_status_context(context: ImsStatusContext) -> &'static ImsStatusContextDescriptor {
    IMS_STATUS_CONTEXTS
        .iter()
        .find(|descriptor| descriptor.context == context)
        .expect("generated IMS status registry must cover every closed context")
}

/// Resolve exact status bytes in a context; distinguish malformed, unknown and known-in-another-context codes.
pub fn ims_status(
    input: &[u8],
    context: ImsStatusContext,
) -> Result<&'static ImsStatusDescriptor, ImsStatusProblem> {
    let code = ImsStatusCode::parse(input)?.bytes();
    let statuses = ims_status_context(context).statuses;
    if let Ok(index) = statuses.binary_search_by_key(&code, |status| status.code) {
        return Ok(&statuses[index]);
    }
    if IMS_STATUS_CONTEXTS.iter().any(|descriptor| {
        descriptor
            .statuses
            .binary_search_by_key(&code, |status| status.code)
            .is_ok()
    }) {
        Err(ImsStatusProblem::ForbiddenStatusContext)
    } else {
        Err(ImsStatusProblem::UnknownCode)
    }
}

/// Resolve exact context status and then require permitted PCB-kind membership; no runtime outcome is computed.
pub fn resolve_ims_status(
    input: &[u8],
    context: ImsStatusContext,
    pcb_kind: ImsPcbKind,
) -> Result<&'static ImsStatusDescriptor, ImsStatusProblem> {
    let status = ims_status(input, context)?;
    if !ims_status_context(context).pcb_kinds.contains(&pcb_kind) {
        return Err(ImsStatusProblem::ForbiddenPcbKind);
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn blank_success_and_documented_status_sets_are_exact() {
        assert_eq!(IMS_STATUS_CONTEXTS.len(), 3);
        assert_eq!(IMS_STATUS_CONTEXT_MEMBERSHIPS, 205);
        assert_eq!(IMS_STATUS_DISTINCT_CODE_COUNT, 162);
        assert_eq!(
            IMS_STATUS_CONTEXTS
                .iter()
                .map(|context| context.statuses.len())
                .collect::<Vec<_>>(),
            [87, 50, 68]
        );

        for context in [
            ImsStatusContext::Database,
            ImsStatusContext::SystemService,
            ImsStatusContext::Message,
        ] {
            let success = ims_status(b"  ", context).unwrap();
            assert!(success.is_blank_success());
            assert_eq!(
                success.categories,
                &[ImsStatusCategory::ExceptionalValidCompleted]
            );
        }

        assert_eq!(
            ims_status(b"GB", ImsStatusContext::Database)
                .unwrap()
                .categories,
            &[ImsStatusCategory::ExceptionalValidCompleted]
        );
        assert_eq!(
            ims_status(b"FD", ImsStatusContext::Database)
                .unwrap()
                .categories,
            &[
                ImsStatusCategory::WarningNoDataCompleted,
                ImsStatusCategory::UnavailableData,
            ]
        );
        assert_eq!(
            ims_status(b"BD", ImsStatusContext::Database)
                .unwrap()
                .categories,
            &[ImsStatusCategory::LockTimeout]
        );
        assert_eq!(
            ims_status(b"QC", ImsStatusContext::SystemService)
                .unwrap()
                .categories,
            &[ImsStatusCategory::WarningNoDataCompleted]
        );
        assert_eq!(
            ims_status(b"CC", ImsStatusContext::Message)
                .unwrap()
                .categories,
            &[ImsStatusCategory::WarningWithDataCompleted]
        );

        let distinct = IMS_STATUS_CONTEXTS
            .iter()
            .flat_map(|context| context.statuses.iter().map(|status| status.code))
            .collect::<BTreeSet<_>>();
        assert_eq!(distinct.len(), IMS_STATUS_DISTINCT_CODE_COUNT);
    }

    #[test]
    fn status_lookup_rejects_malformed_unknown_and_forbidden_inputs() {
        for malformed in [b"G".as_slice(), b"gb", b"G ", b"!A", b"AAA"] {
            assert_eq!(
                ims_status(malformed, ImsStatusContext::Database),
                Err(ImsStatusProblem::MalformedCode)
            );
        }
        assert_eq!(
            ims_status(b"ZZ", ImsStatusContext::Database),
            Err(ImsStatusProblem::UnknownCode)
        );
        assert_eq!(
            ims_status(b"GB", ImsStatusContext::SystemService),
            Err(ImsStatusProblem::ForbiddenStatusContext)
        );
        assert_eq!(
            ims_status(b"AA", ImsStatusContext::Database),
            Err(ImsStatusProblem::ForbiddenStatusContext)
        );
    }

    #[test]
    fn status_contexts_accept_only_their_documented_pcb_kinds() {
        assert!(
            resolve_ims_status(b"GB", ImsStatusContext::Database, ImsPcbKind::Database).is_ok()
        );
        assert!(resolve_ims_status(b"GB", ImsStatusContext::Database, ImsPcbKind::Gsam).is_ok());
        assert!(resolve_ims_status(b"QC", ImsStatusContext::SystemService, ImsPcbKind::Io).is_ok());
        assert!(resolve_ims_status(b"CC", ImsStatusContext::Message, ImsPcbKind::Io).is_ok());
        assert!(
            resolve_ims_status(b"CC", ImsStatusContext::Message, ImsPcbKind::Alternate,).is_ok()
        );
        assert_eq!(
            resolve_ims_status(b"GB", ImsStatusContext::Database, ImsPcbKind::Io),
            Err(ImsStatusProblem::ForbiddenPcbKind)
        );
        assert_eq!(
            resolve_ims_status(
                b"QC",
                ImsStatusContext::SystemService,
                ImsPcbKind::Alternate,
            ),
            Err(ImsStatusProblem::ForbiddenPcbKind)
        );
    }

    #[test]
    fn generated_status_rows_are_sorted_unique_and_well_formed() {
        for context in IMS_STATUS_CONTEXTS {
            assert!(
                context
                    .statuses
                    .windows(2)
                    .all(|pair| pair[0].code < pair[1].code)
            );
            for status in context.statuses {
                assert!(ImsStatusCode::parse(&status.code).is_ok());
                assert!(status.categories.len() <= 2);
            }
        }
    }
}
