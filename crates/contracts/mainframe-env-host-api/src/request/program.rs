use super::RuntimeServiceSelector;
use crate::{ClassName, MethodName, ProgramName};
use mainframe_env_execution_api::{ArtifactRef, BoundedPayload};

#[derive(Clone, Debug, Eq, PartialEq)]
/// Exact immutable selection attached to a LINK request.
/// Validation requires a nonzero generation and a lowercase `sha256:` content identity.
pub struct ProgramLinkSelection {
    /// Exact immutable executable artifact selected by the caller.
    pub artifact: ArtifactRef,
    /// Exact installed program generation selected by the caller.
    pub generation: u64,
    /// Exact immutable program-definition or application-entry identity selected by the caller.
    pub content_identity: String,
}

impl ProgramLinkSelection {
    pub(super) fn is_valid(&self) -> bool {
        self.generation != 0
            && self
                .content_identity
                .strip_prefix("sha256:")
                .is_some_and(|digest| {
                    digest.len() == 64
                        && digest
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned program-control operations and explicitly bounded argument payloads.
/// State-changing variants use the outer effect replay key; no payload-level `Mutation` is carried.
pub enum ProgramRequest {
    /// Query installed program metadata.
    Inquire {
        /// Validated program name to resolve or invoke.
        program: ProgramName,
    },
    /// Call a program, optionally selecting an exact runtime-service ABI.
    Call {
        /// Validated program name to resolve or invoke.
        program: ProgramName,
        /// Owned bounded argument bytes tagged with a payload schema identity.
        payload: BoundedPayload,
        /// Optional exact namespace/name/ABI selection for a runtime service.
        service: Option<RuntimeServiceSelector>,
    },
    /// Invoke a method on an schema-described receiver.
    Invoke {
        /// Validated class name used for method resolution.
        class: ClassName,
        /// Validated method name selected on the receiver.
        method: MethodName,
        /// Owned bounded receiver bytes tagged with a payload schema identity.
        receiver: BoundedPayload,
        /// Owned bounded argument bytes tagged with a payload schema identity.
        payload: BoundedPayload,
    },
    /// Link to a program, optionally pinning its immutable installed selection.
    Link {
        /// Validated program name to resolve or invoke.
        program: ProgramName,
        /// Owned bounded argument bytes tagged with a payload schema identity.
        payload: BoundedPayload,
        /// Optional immutable artifact/generation/content selection; `None` leaves resolution to
        /// the provider.
        selection: Option<ProgramLinkSelection>,
    },
    /// Transfer control to the selected program.
    Xctl {
        /// Validated program name to resolve or invoke.
        program: ProgramName,
        /// Owned bounded argument bytes tagged with a payload schema identity.
        payload: BoundedPayload,
    },
    /// Return with an optional next-transaction selection.
    Return {
        /// Optional next transaction identity interpreted by the program-control provider.
        next_transaction: Option<String>,
        /// Owned bounded argument bytes tagged with a payload schema identity.
        payload: BoundedPayload,
    },
    /// Cancel a nonempty bounded set of program selections.
    Cancel {
        /// Program names to cancel; host validation requires 1 through `max_fields` entries.
        programs: Vec<ProgramName>,
    },
    /// Request abnormal termination with an owned code.
    Abend {
        /// Owned abnormal-termination code interpreted by the provider.
        code: String,
    },
}
