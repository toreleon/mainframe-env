//! Legacy MQ records and validation, mechanically retained.

use super::{HostLimits, HostProblem, Mutation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqOperation {
    Open,
    Get,
    Put,
    PutOne,
    Close,
    Commit,
    Rollback,
}

impl MqOperation {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRequest {
    pub operation: MqOperation,
    pub queue: Option<String>,
    pub handle: Option<u32>,
    pub options: i32,
    pub message: Vec<u8>,
    pub message_id: Option<Vec<u8>>,
    pub correlation_id: Option<Vec<u8>>,
    pub wait_ticks: u64,
    pub max_message_bytes: u32,
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqResult {
    pub completion_code: i32,
    pub reason_code: i32,
    pub handle: Option<u32>,
    pub message: Vec<u8>,
    pub message_id: Option<Vec<u8>>,
    pub correlation_id: Option<Vec<u8>>,
    pub trigger_program: Option<String>,
}

impl MqRequest {
    pub(super) fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        let request = self;
        if request
            .queue
            .as_ref()
            .is_some_and(|queue| queue.is_empty() || queue.len() > limits.max_name_bytes)
            || request.message.len() > limits.max_record_bytes
            || request
                .message_id
                .as_ref()
                .is_some_and(|value| value.len() != 24)
            || request
                .correlation_id
                .as_ref()
                .is_some_and(|value| value.len() != 24)
            || request.max_message_bytes == 0
            || request.max_message_bytes as usize > limits.max_record_bytes
        {
            return Err(HostProblem::ResourceExhausted);
        }
        request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?
            .validate(limits)
    }
}

impl MqResult {
    pub(super) fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        let result = self;
        if result.message.len() > limits.max_record_bytes
            || result
                .message_id
                .as_ref()
                .is_some_and(|value| value.len() != 24)
            || result
                .correlation_id
                .as_ref()
                .is_some_and(|value| value.len() != 24)
            || result
                .trigger_program
                .as_ref()
                .is_some_and(|program| program.is_empty() || program.len() > limits.max_name_bytes)
        {
            Err(HostProblem::ResourceExhausted)
        } else {
            Ok(())
        }
    }
}
