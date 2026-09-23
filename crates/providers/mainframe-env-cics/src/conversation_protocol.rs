//! Transport-neutral conversation lifecycle shared by CIC-905 command families.
//!
//! A transport adapter may carry frames, but only this contract changes CICS
//! ownership and protocol state. The caller persists each validated transition
//! with the enclosing provider effect and audit record.

use serde::{Deserialize, Serialize};

mod ledger;
pub use ledger::{
    CONVERSATION_STATE_NAMESPACE, ConversationAttachHeader, ConversationLedger,
    ConversationSystemDefinition,
};

/// Durable encoding version for one allocated conversation.
pub const CONVERSATION_RECORD_VERSION: u16 = 1;
/// Maximum length of a partner process name defined by APPC.
pub const MAX_PROCESS_BYTES: usize = 64;
/// Maximum APPC PIP list length, including each record's four-byte header.
pub const MAX_PIP_BYTES: usize = 763;

/// The session protocol selected at allocation, independent of its carrier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationKind {
    AppcMapped,
    AppcBasic,
    Mro,
}

/// Source-visible conversation state. Future sibling commands can add guarded
/// transitions without redefining the durable identity or owner.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationState {
    Allocated,
    Send,
    Receive,
    Free,
    PendFree,
    ConfFree,
    ConfReceive,
    ConfSend,
    SyncFree,
    SyncReceive,
    SyncSend,
    Rollback,
}

/// The invocation that owns a conversation. Its lease epoch fences resumed
/// workers and prevents a stale task from reusing a released session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationOwner {
    pub execution: String,
    pub run_unit: String,
    pub lease_epoch: u64,
}

impl ConversationOwner {
    fn valid(&self) -> bool {
        !self.execution.is_empty()
            && self.execution.len() <= 128
            && !self.execution.contains('\0')
            && !self.run_unit.is_empty()
            && self.run_unit.len() <= 128
            && !self.run_unit.contains('\0')
            && self.lease_epoch != 0
    }
}

/// No transport or transaction/program name appears in this authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationRecord {
    pub version: u16,
    pub token: [u8; 4],
    pub system: String,
    pub kind: ConversationKind,
    pub owner: ConversationOwner,
    pub principal_facility: bool,
    /// A FREE command has returned this allocation to the session pool.
    pub released: bool,
    pub state: ConversationState,
    pub sync_level: Option<u8>,
    pub process: Option<Vec<u8>>,
    pub pip: Vec<u8>,
    pub sequence: u64,
}

/// Local/DPL invocation context supplied by the trusted host boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationContext {
    Local,
    DplServer,
}

/// Fail-closed protocol errors; command handlers map these to the source's
/// mapped conditions or GDS six-byte return codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationProblem {
    Malformed,
    NotOwned,
    StaleOwner,
    DplPrincipal,
    WrongKind,
    WrongState,
    Length,
    Exhausted,
}

impl ConversationRecord {
    pub fn allocate(
        token: [u8; 4],
        system: &str,
        kind: ConversationKind,
        owner: ConversationOwner,
        principal_facility: bool,
    ) -> Result<Self, ConversationProblem> {
        let record = Self {
            version: CONVERSATION_RECORD_VERSION,
            token,
            system: system.into(),
            kind,
            owner,
            principal_facility,
            released: false,
            state: ConversationState::Allocated,
            sync_level: None,
            process: None,
            pip: Vec::new(),
            sequence: 0,
        };
        record.validate()?;
        Ok(record)
    }

    /// Decode an existing provider row. Unknown schema versions, malformed
    /// lengths and impossible state combinations cannot become live sessions.
    pub fn decode(bytes: &[u8]) -> Result<Self, ConversationProblem> {
        if bytes.len() > 2048 {
            return Err(ConversationProblem::Length);
        }
        let record: Self =
            serde_json::from_slice(bytes).map_err(|_| ConversationProblem::Malformed)?;
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>, ConversationProblem> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| ConversationProblem::Malformed)?;
        if encoded.len() > 2048 {
            return Err(ConversationProblem::Length);
        }
        Ok(encoded)
    }

    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.version != CONVERSATION_RECORD_VERSION
            || self.token == [0; 4]
            || self.system.is_empty()
            || self.system.len() > 4
            || !self.system.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
            })
            || !self.owner.valid()
            || self
                .process
                .as_ref()
                .is_some_and(|name| name.is_empty() || name.len() > MAX_PROCESS_BYTES)
            || self.pip.len() > MAX_PIP_BYTES
            || self.sync_level.is_some_and(|level| level > 2)
            || (self.released && self.state != ConversationState::Free)
            || (self.process.is_none() && self.sync_level.is_some())
            || (self.kind != ConversationKind::Mro
                && self.state != ConversationState::Allocated
                && self.process.is_none()
                && self.state != ConversationState::Free)
        {
            return Err(ConversationProblem::Malformed);
        }
        validate_pip(&self.pip)?;
        Ok(())
    }

    /// Check this task's authority before any operation or state mutation.
    /// The DPL server's function-shipping principal facility is restricted
    /// even though other task-owned alternate facilities may be usable.
    pub fn check_owner(
        &self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        if self.owner.execution != owner.execution || self.owner.run_unit != owner.run_unit {
            return Err(ConversationProblem::NotOwned);
        }
        if self.owner.lease_epoch != owner.lease_epoch {
            return Err(ConversationProblem::StaleOwner);
        }
        if self.principal_facility && context == ConversationContext::DplServer {
            return Err(ConversationProblem::DplPrincipal);
        }
        if self.released {
            return Err(ConversationProblem::WrongState);
        }
        Ok(())
    }

    /// CONNECT PROCESS / GDS CONNECT PROCESS move only an allocated APPC
    /// conversation into SEND. The complete PIP record boundaries are checked
    /// before any durable write or transport dispatch.
    pub fn connect(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
        process: Vec<u8>,
        pip: Vec<u8>,
        sync_level: u8,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        let expected = if basic {
            ConversationKind::AppcBasic
        } else {
            ConversationKind::AppcMapped
        };
        if self.kind != expected {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Allocated {
            return Err(ConversationProblem::WrongState);
        }
        if process.is_empty() || process.len() > MAX_PROCESS_BYTES || sync_level > 2 {
            return Err(ConversationProblem::Length);
        }
        validate_pip(&pip)?;
        self.next_sequence()?;
        self.process = Some(process);
        self.pip = pip;
        self.sync_level = Some(sync_level);
        self.state = ConversationState::Send;
        Ok(())
    }

    /// Record the result of an actual mapped APPC or MRO exchange. A transport
    /// result is an input, never inferred from a successful broker delivery.
    pub fn complete_converse(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        next: ConversationState,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.kind == ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Send
            && !(self.kind == ConversationKind::Mro && self.state == ConversationState::Allocated)
        {
            return Err(ConversationProblem::WrongState);
        }
        if !matches!(
            next,
            ConversationState::Send
                | ConversationState::Receive
                | ConversationState::Free
                | ConversationState::PendFree
                | ConversationState::ConfReceive
                | ConversationState::SyncReceive
        ) {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        self.state = next;
        Ok(())
    }

    /// Apply a peer protocol completion, independently of message delivery.
    /// The basic conversation's GDS FREE becomes legal only after this event.
    pub fn peer_finished(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.kind != ConversationKind::AppcBasic || self.state != ConversationState::Send {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        self.state = ConversationState::Free;
        Ok(())
    }

    /// FREE and GDS FREE release this task's allocation. APPC basic requires
    /// the source-defined FREE state; releasing it earlier is a state check.
    pub fn release(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if basic != (self.kind == ConversationKind::AppcBasic) {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Free
            && (basic
                || matches!(
                    self.state,
                    ConversationState::PendFree | ConversationState::Rollback
                ))
        {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        self.state = ConversationState::Free;
        self.released = true;
        Ok(())
    }

    fn next_sequence(&mut self) -> Result<(), ConversationProblem> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ConversationProblem::Exhausted)?;
        Ok(())
    }
}

fn validate_pip(pip: &[u8]) -> Result<(), ConversationProblem> {
    if pip.is_empty() {
        return Ok(());
    }
    if !(4..=MAX_PIP_BYTES).contains(&pip.len()) {
        return Err(ConversationProblem::Length);
    }
    let mut cursor = 0usize;
    while cursor < pip.len() {
        if pip.len() - cursor < 4 {
            return Err(ConversationProblem::Length);
        }
        let length = usize::from(u16::from_be_bytes([pip[cursor], pip[cursor + 1]]));
        if length < 4 || length > pip.len() - cursor || pip[cursor + 2..cursor + 4] != [0, 0] {
            return Err(ConversationProblem::Length);
        }
        cursor += length;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(epoch: u64) -> ConversationOwner {
        ConversationOwner {
            execution: "execution".into(),
            run_unit: "run".into(),
            lease_epoch: epoch,
        }
    }

    #[test]
    fn mapped_lifecycle_and_restart_preserve_owner_and_state() {
        let mut record = ConversationRecord::allocate(
            *b"C001",
            "SYS1",
            ConversationKind::AppcMapped,
            owner(3),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner(3),
                ConversationContext::Local,
                false,
                b"TRAN".to_vec(),
                vec![0, 4, 0, 0],
                2,
            )
            .unwrap();
        let bytes = record.encode().unwrap();
        let mut reopened = ConversationRecord::decode(&bytes).unwrap();
        assert_eq!(reopened.sequence, 1);
        assert_eq!(
            reopened.check_owner(&owner(2), ConversationContext::Local),
            Err(ConversationProblem::StaleOwner)
        );
        assert_eq!(
            reopened.complete_converse(
                &owner(3),
                ConversationContext::Local,
                ConversationState::Allocated
            ),
            Err(ConversationProblem::WrongState)
        );
        reopened
            .complete_converse(
                &owner(3),
                ConversationContext::Local,
                ConversationState::Receive,
            )
            .unwrap();
        reopened
            .release(&owner(3), ConversationContext::Local, false)
            .unwrap();
        assert_eq!(reopened.state, ConversationState::Free);
        assert!(reopened.released);
        assert_eq!(reopened.sequence, 3);
        assert_eq!(
            reopened.release(&owner(3), ConversationContext::Local, false),
            Err(ConversationProblem::WrongState)
        );
    }

    #[test]
    fn dpl_principal_and_wrong_owner_cannot_mutate() {
        let mut record = ConversationRecord::allocate(
            *b"C002",
            "SYS1",
            ConversationKind::AppcMapped,
            owner(3),
            true,
        )
        .unwrap();
        let before = record.clone();
        assert_eq!(
            record.connect(
                &owner(3),
                ConversationContext::DplServer,
                false,
                b"TRAN".to_vec(),
                vec![],
                0
            ),
            Err(ConversationProblem::DplPrincipal)
        );
        let alien = ConversationOwner {
            execution: "other".into(),
            ..owner(3)
        };
        assert_eq!(
            record.release(&alien, ConversationContext::Local, false),
            Err(ConversationProblem::NotOwned)
        );
        assert_eq!(record, before);
    }

    #[test]
    fn basic_release_requires_free_and_connect_validates_pip() {
        let mut record = ConversationRecord::allocate(
            *b"C003",
            "SYS1",
            ConversationKind::AppcBasic,
            owner(3),
            false,
        )
        .unwrap();
        assert_eq!(
            record.release(&owner(3), ConversationContext::Local, true),
            Err(ConversationProblem::WrongState)
        );
        assert_eq!(
            record.connect(
                &owner(3),
                ConversationContext::Local,
                true,
                b"TRAN".to_vec(),
                vec![0, 3, 0, 0],
                0
            ),
            Err(ConversationProblem::Length)
        );
        assert_eq!(record.state, ConversationState::Allocated);
        record
            .connect(
                &owner(3),
                ConversationContext::Local,
                true,
                b"TRAN".to_vec(),
                vec![0, 4, 0, 0],
                0,
            )
            .unwrap();
        assert_eq!(
            record.complete_converse(
                &owner(3),
                ConversationContext::Local,
                ConversationState::Free
            ),
            Err(ConversationProblem::WrongKind)
        );
        record
            .peer_finished(&owner(3), ConversationContext::Local)
            .unwrap();
        record
            .release(&owner(3), ConversationContext::Local, true)
            .unwrap();
    }

    #[test]
    fn corrupt_rows_and_invalid_owners_fail_closed() {
        let record =
            ConversationRecord::allocate(*b"C004", "SYS1", ConversationKind::Mro, owner(3), false)
                .unwrap();
        let mut encoded = record.encode().unwrap();
        encoded.extend_from_slice(b"{}{}");
        assert_eq!(
            ConversationRecord::decode(&encoded),
            Err(ConversationProblem::Malformed)
        );
        let mut corrupt = record;
        corrupt.version = 2;
        assert_eq!(corrupt.encode(), Err(ConversationProblem::Malformed));
        let mut unknown: serde_json::Value = serde_json::from_slice(
            &ConversationRecord::allocate(*b"C006", "SYS1", ConversationKind::Mro, owner(3), false)
                .unwrap()
                .encode()
                .unwrap(),
        )
        .unwrap();
        unknown["unreviewed_field"] = serde_json::Value::Bool(true);
        assert_eq!(
            ConversationRecord::decode(&serde_json::to_vec(&unknown).unwrap()),
            Err(ConversationProblem::Malformed)
        );
        assert_eq!(
            ConversationRecord::allocate(*b"C005", "SYS1", ConversationKind::Mro, owner(0), false),
            Err(ConversationProblem::Malformed)
        );
    }
}
