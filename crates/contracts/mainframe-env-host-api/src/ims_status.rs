//! Exact IMS PCB status lookup and PCB-kind applicability contracts.

use crate::ImsPcbKind;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Status membership namespace; a recognized code is not valid in every namespace.
pub enum ImsStatusContext {
    /// Generated database-call status membership set.
    Database,
    /// Generated system-service status membership set.
    SystemService,
    /// Generated message-call status membership set.
    Message,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Generated classification of a status; category membership does not execute a call.
pub enum ImsStatusCategory {
    /// Descriptor classification for an exceptional but completed valid call.
    ExceptionalValidCompleted,
    /// Descriptor classification for completion with warning and data.
    WarningWithDataCompleted,
    /// Descriptor classification for completion with warning and no data.
    WarningNoDataCompleted,
    /// Descriptor classification for improper caller specification.
    ImproperUserSpecification,
    /// Descriptor classification for system, I/O or security error.
    SystemIoSecurityError,
    /// Descriptor classification for unavailable data.
    UnavailableData,
    /// Descriptor classification for a lock-timeout condition.
    LockTimeout,
}

impl ImsStatusCategory {
    #[must_use]
    /// Return whether this category is one of the three completed-call classifications.
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
/// Exact two-byte code and its generated classifications.
pub struct ImsStatusDescriptor {
    /// Exact display-code status bytes, including two blanks for success.
    pub code: [u8; 2],
    /// All generated category memberships associated with this status.
    pub categories: &'static [ImsStatusCategory],
}

impl ImsStatusDescriptor {
    #[must_use]
    /// Return whether both status bytes are blank.
    pub fn is_blank_success(self) -> bool {
        self.code == *b"  "
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Pinned-source status inventory and allowed PCB kinds for one context.
pub struct ImsStatusContextDescriptor {
    /// Namespace whose status memberships are described.
    pub context: ImsStatusContext,
    /// Pinned topic locator supporting the membership set.
    pub source_topic: &'static str,
    /// Expected source-body SHA-256 identity.
    pub source_sha256: &'static str,
    /// PCB categories admitted in this status namespace.
    pub pcb_kinds: &'static [ImsPcbKind],
    /// Sorted exact-code descriptors used by bounded lookup.
    pub statuses: &'static [ImsStatusDescriptor],
}

include!("generated/ims_status_codes.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Syntactically validated two-byte status; registry membership requires a separate lookup.
pub struct ImsStatusCode([u8; 2]);

impl ImsStatusCode {
    /// Exact two-blank success representation.
    pub const BLANK_SUCCESS: Self = Self(*b"  ");

    /// Accept two blanks or two uppercase ASCII letters/digits; reject other lengths and bytes.
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
    /// Return the exact pair of stored status bytes.
    pub const fn bytes(self) -> [u8; 2] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Malformed or inapplicable code rejected by status lookup.
pub enum ImsStatusProblem {
    /// Input is not an admitted two-byte status spelling.
    MalformedCode,
    /// No generated context contains the code.
    UnknownCode,
    /// The code exists but not in the requested context.
    ForbiddenStatusContext,
    /// The selected status context does not admit the PCB kind.
    ForbiddenPcbKind,
}

#[must_use]
/// Return all generated status-context descriptors.
pub fn ims_status_contexts() -> &'static [ImsStatusContextDescriptor] {
    IMS_STATUS_CONTEXTS
}

#[must_use]
/// Return the descriptor for a closed context; the generated inventory must be exhaustive.
pub fn ims_status_context(context: ImsStatusContext) -> &'static ImsStatusContextDescriptor {
    IMS_STATUS_CONTEXTS
        .iter()
        .find(|descriptor| descriptor.context == context)
        .expect("generated IMS status registry must cover every closed context")
}

/// Resolve an exact code within one context, distinguishing unknown from forbidden membership.
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

/// Resolve context membership and reject a PCB kind outside that context.
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
