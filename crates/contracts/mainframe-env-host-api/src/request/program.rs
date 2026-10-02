use super::RuntimeServiceSelector;
use crate::{ClassName, MethodName, ProgramName};
use mainframe_env_execution_api::{ArtifactRef, BoundedPayload};

#[derive(Clone, Debug, Eq, PartialEq)]
/// Exact immutable program selection; validation requires positive generation and lowercase SHA-256 content identity.
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
/// Owned program routing/control request. A name or selection does not install code or grant invocation permission.
pub enum ProgramRequest {
    /// Observe installed program metadata without invoking it.
    Inquire {
        /// Validated target program name; installation and resource admission remain external.
        program: ProgramName,
    },
    /// Call a program or explicitly selected runtime service.
    Call {
        /// Validated target program name; installation and resource admission remain external.
        program: ProgramName,
        /// Owned schema-qualified call/control data, not a raw caller pointer.
        payload: BoundedPayload,
        /// Optional explicit runtime-service route with positive ABI version.
        service: Option<RuntimeServiceSelector>,
    },
    /// Invoke a method with owned receiver and argument data.
    Invoke {
        /// Validated class identity for method dispatch.
        class: ClassName,
        /// Validated method identity within the class.
        method: MethodName,
        /// Owned schema-qualified receiver state.
        receiver: BoundedPayload,
        /// Owned schema-qualified call/control data, not a raw caller pointer.
        payload: BoundedPayload,
    },
    /// Invoke a nested program level with optional immutable selection.
    Link {
        /// Validated target program name; installation and resource admission remain external.
        program: ProgramName,
        /// Owned schema-qualified call/control data, not a raw caller pointer.
        payload: BoundedPayload,
        /// Optional exact immutable generation/content binding checked before LINK dispatch.
        selection: Option<ProgramLinkSelection>,
    },
    /// Transfer program control without inventing a continuation.
    Xctl {
        /// Validated target program name; installation and resource admission remain external.
        program: ProgramName,
        /// Owned schema-qualified call/control data, not a raw caller pointer.
        payload: BoundedPayload,
    },
    /// Return owned output and optional next transaction selection.
    Return {
        /// Optional continuation transaction label; scheduling remains router-owned.
        next_transaction: Option<String>,
        /// Owned schema-qualified call/control data, not a raw caller pointer.
        payload: BoundedPayload,
    },
    /// Request cancellation/reset of the listed program activations.
    Cancel {
        /// Nonempty bounded list of programs whose activation state is to be cancelled.
        programs: Vec<ProgramName>,
    },
    /// Request application abnormal termination, distinct from infrastructure failure.
    Abend {
        /// Application abend code retained for router handling.
        code: String,
    },
}
