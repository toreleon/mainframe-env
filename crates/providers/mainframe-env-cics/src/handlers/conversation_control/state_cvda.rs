//! Fullword CICS conversation STATE output from the pinned dfha80c table.

use super::ConversationState;
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{CicsResponse, HostProblem};

const STATE_SCHEMA: &str = "mainframe-env.cics.cvda@1";

pub(super) fn bytes(state: ConversationState) -> Vec<u8> {
    let value: i32 = match state {
        ConversationState::Allocated => 82,
        ConversationState::ConfFree => 83,
        ConversationState::ConfReceive => 84,
        ConversationState::ConfSend => 85,
        ConversationState::Free => 86,
        ConversationState::PendFree => 87,
        ConversationState::PendReceive => 88,
        ConversationState::Receive => 89,
        ConversationState::Rollback => 90,
        ConversationState::Send => 91,
        ConversationState::SyncFree => 92,
        ConversationState::SyncReceive => 93,
        ConversationState::SyncSend => 94,
    };
    value.to_be_bytes().to_vec()
}

pub(super) fn insert_output(
    response: &mut CicsResponse,
    bytes: Option<&Vec<u8>>,
) -> Result<(), HostProblem> {
    if let Some(bytes) = bytes {
        if bytes.len() != 4 {
            return Err(HostProblem::InfrastructureFailure);
        }
        response.outputs.insert(
            "STATE".into(),
            BoundedPayload::new(STATE_SCHEMA, bytes.clone(), InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ConversationState, bytes};

    #[test]
    fn pinned_dfha80c_conversation_state_values() {
        for (state, value) in [
            (ConversationState::Allocated, 82_i32),
            (ConversationState::ConfFree, 83),
            (ConversationState::ConfReceive, 84),
            (ConversationState::ConfSend, 85),
            (ConversationState::Free, 86),
            (ConversationState::PendFree, 87),
            (ConversationState::PendReceive, 88),
            (ConversationState::Receive, 89),
            (ConversationState::Rollback, 90),
            (ConversationState::Send, 91),
            (ConversationState::SyncFree, 92),
            (ConversationState::SyncReceive, 93),
            (ConversationState::SyncSend, 94),
        ] {
            assert_eq!(bytes(state), value.to_be_bytes());
        }
    }
}
