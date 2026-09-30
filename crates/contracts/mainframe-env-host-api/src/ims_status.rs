//! Exact IMS PCB status lookup and PCB-kind applicability contracts.

use crate::ImsPcbKind;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ImsStatusContext {
    Database,
    SystemService,
    Message,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ImsStatusCategory {
    ExceptionalValidCompleted,
    WarningWithDataCompleted,
    WarningNoDataCompleted,
    ImproperUserSpecification,
    SystemIoSecurityError,
    UnavailableData,
    LockTimeout,
}

impl ImsStatusCategory {
    #[must_use]
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
pub struct ImsStatusDescriptor {
    pub code: [u8; 2],
    pub categories: &'static [ImsStatusCategory],
}

impl ImsStatusDescriptor {
    #[must_use]
    pub fn is_blank_success(self) -> bool {
        self.code == *b"  "
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsStatusContextDescriptor {
    pub context: ImsStatusContext,
    pub source_topic: &'static str,
    pub source_sha256: &'static str,
    pub pcb_kinds: &'static [ImsPcbKind],
    pub statuses: &'static [ImsStatusDescriptor],
}

include!("generated/ims_status_codes.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsStatusCode([u8; 2]);

impl ImsStatusCode {
    pub const BLANK_SUCCESS: Self = Self(*b"  ");

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
    pub const fn bytes(self) -> [u8; 2] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsStatusProblem {
    MalformedCode,
    UnknownCode,
    ForbiddenStatusContext,
    ForbiddenPcbKind,
}

#[must_use]
pub fn ims_status_contexts() -> &'static [ImsStatusContextDescriptor] {
    IMS_STATUS_CONTEXTS
}

#[must_use]
pub fn ims_status_context(context: ImsStatusContext) -> &'static ImsStatusContextDescriptor {
    IMS_STATUS_CONTEXTS
        .iter()
        .find(|descriptor| descriptor.context == context)
        .expect("generated IMS status registry must cover every closed context")
}

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
