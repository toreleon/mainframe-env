//! Additive display-code SSA operands; legacy IMS request encodings are unchanged.
use crate::{HostLimits, HostProblem, HostRequest, ImsExecutionContext, ImsOperation, ImsRequest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsNavigationRequest {
    pub request: ImsRequest,
    pub context: ImsExecutionContext,
    /// Invariant syntax is display-code; comparative values are exact binary bytes.
    pub ssas: Vec<Vec<u8>>,
}

impl ImsNavigationRequest {
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if !matches!(
            self.request.operation,
            ImsOperation::GetUnique
                | ImsOperation::GetNext
                | ImsOperation::GetNextParent
                | ImsOperation::GetHoldUnique
                | ImsOperation::GetHoldNext
                | ImsOperation::GetHoldNextParent
        ) || !self.request.segments.is_empty()
            || !self.request.qualifiers.is_empty()
            || !self.request.data.is_empty()
            || self.request.psb.is_some()
            || self.request.checkpoint_id.is_some()
            || self.request.system.is_some()
            || self.request.q_class.is_some()
        {
            return Err(HostProblem::Malformed);
        }
        if self.ssas.len() > 15
            || self.ssas.len() > limits.max_fields
            || self.ssas.iter().any(|ssa| ssa.len() > 32 * 1024)
            || self
                .ssas
                .iter()
                .try_fold(0usize, |n, s| n.checked_add(s.len()))
                .is_none_or(|n| n > limits.max_record_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        HostRequest::Ims(self.request.clone()).validate(limits)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Mutation;
    use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};

    pub(crate) fn sample() -> ImsNavigationRequest {
        ImsNavigationRequest {
            context: ImsExecutionContext::DbBatch,
            request: ImsRequest {
                operation: ImsOperation::GetHoldUnique,
                psb: None,
                pcb: 1,
                segments: vec![],
                data: vec![],
                qualifiers: vec![],
                checkpoint_id: None,
                max_segments: 15,
                system: None,
                q_class: None,
                mutation: Some(Mutation {
                    sequence: 7,
                    idempotency_key: IdempotencyKey::new("ssa-golden", InvocationLimits::default())
                        .unwrap(),
                    transaction: None,
                }),
            },
            ssas: vec![b"ROOT    *O(00010001EQ\0)".to_vec()],
        }
    }

    #[test]
    fn navigation_requires_bounded_unambiguous_operands_and_mutation_identity() {
        let limits = HostLimits::default();
        let valid = sample();
        assert_eq!(valid.validate(limits), Ok(()));
        let mut no_identity = valid.clone();
        no_identity.request.mutation = None;
        assert_eq!(
            no_identity.validate(limits),
            Err(HostProblem::MissingIdempotency)
        );
        let mut legacy_operands = valid.clone();
        legacy_operands.request.segments.push("ROOT".into());
        assert_eq!(
            legacy_operands.validate(limits),
            Err(HostProblem::Malformed)
        );
        let mut wrong_call = valid.clone();
        wrong_call.request.operation = ImsOperation::Insert;
        assert_eq!(wrong_call.validate(limits), Err(HostProblem::Malformed));
        let mut too_many = valid.clone();
        too_many.ssas = vec![b"ROOT     ".to_vec(); 15];
        assert_eq!(too_many.validate(limits), Ok(()));
        too_many.ssas = vec![b"ROOT     ".to_vec(); 16];
        assert_eq!(
            too_many.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
        let mut too_large = valid.clone();
        too_large.ssas = vec![vec![b' '; 32 * 1024 + 1]];
        assert_eq!(
            too_large.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            valid.validate(HostLimits {
                max_record_bytes: 8,
                ..limits
            }),
            Err(HostProblem::ResourceExhausted)
        );
        let host = HostRequest::ImsNavigation(valid);
        assert!(host.is_mutating());
        assert_eq!(
            host.required_capability(InvocationLimits::default())
                .as_str(),
            "host.ims.write"
        );
        assert_eq!(host.mutation().unwrap().sequence, 7);
    }
}
