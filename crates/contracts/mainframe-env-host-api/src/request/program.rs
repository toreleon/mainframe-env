use super::RuntimeServiceSelector;
use crate::{ClassName, MethodName, ProgramName};
use mainframe_env_execution_api::{ArtifactRef, BoundedPayload};

#[derive(Clone, Debug, Eq, PartialEq)]
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
pub enum ProgramRequest {
    Inquire {
        program: ProgramName,
    },
    Call {
        program: ProgramName,
        payload: BoundedPayload,
        service: Option<RuntimeServiceSelector>,
    },
    Invoke {
        class: ClassName,
        method: MethodName,
        receiver: BoundedPayload,
        payload: BoundedPayload,
    },
    Link {
        program: ProgramName,
        payload: BoundedPayload,
        selection: Option<ProgramLinkSelection>,
    },
    Xctl {
        program: ProgramName,
        payload: BoundedPayload,
    },
    Return {
        next_transaction: Option<String>,
        payload: BoundedPayload,
    },
    Cancel {
        programs: Vec<ProgramName>,
    },
    Abend {
        code: String,
    },
}
