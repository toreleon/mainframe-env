use super::*;
use crate::{ImsGsamAddress, ImsGsamRequest, ImsGsamResult, ImsGsamSearchArgument};
impl Canonical for ImsGsamAddress {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsGsamAddress", 2)?;
        out.text("database")?;
        self.database.encode(out)?;
        out.text("token")?;
        self.token.encode(out)
    }
}
impl Canonical for ImsGsamSearchArgument {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Beginning => out.variant("ImsGsamSearchArgument", "Beginning", 0),
            Self::Record(address) => {
                out.variant("ImsGsamSearchArgument", "Record", 1)?;
                out.text("0")?;
                address.encode(out)
            }
        }
    }
}
impl Canonical for ImsGsamRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsGsamRequest", 4)?;
        out.text("context")?;
        self.context.encode(out)?;
        out.text("request")?;
        self.request.encode(out)?;
        out.text("save_address")?;
        self.save_address.encode(out)?;
        out.text("search")?;
        self.search.encode(out)
    }
}
impl Canonical for ImsGsamResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsGsamResult", 2)?;
        out.text("address")?;
        self.address.encode(out)?;
        out.text("result")?;
        self.result.encode(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImsExecutionContext, ImsOperation, ImsRequest, ImsResult, ImsSegment, Mutation};
    use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};

    fn sample() -> ImsGsamRequest {
        ImsGsamRequest {
            request: ImsRequest {
                operation: ImsOperation::GetUnique,
                psb: None,
                pcb: 1,
                segments: vec![],
                data: vec![],
                qualifiers: vec![],
                checkpoint_id: None,
                max_segments: 1,
                system: None,
                q_class: None,
                mutation: Some(Mutation {
                    sequence: 7,
                    idempotency_key: IdempotencyKey::new(
                        "gsam-golden",
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    transaction: None,
                }),
            },
            context: ImsExecutionContext::DbBatch,
            search: Some(ImsGsamSearchArgument::Record(ImsGsamAddress {
                database: "GENDB".into(),
                token: std::array::from_fn(|i| i as u8),
            })),
            save_address: false,
        }
    }
    #[test]
    fn gsam_canonical_request_and_result_match_independent_binary_goldens() {
        let value = sample();
        let request = HostRequest::ImsGsam(value.clone());
        let digest = canonical_request_digest(&request).unwrap();
        assert_eq!(
            canonical_request_size(&request, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            829
        );
        assert_eq!(
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "ccc805d540fca887179e009db58f91780ea84c29921729688a36919698c9b980"
        );
        let address = match value.search.clone().unwrap() {
            ImsGsamSearchArgument::Record(a) => a,
            _ => unreachable!(),
        };
        let result = Ok(HostResult::ImsGsam(ImsGsamResult {
            result: ImsResult {
                status: "  ".into(),
                segments: vec![ImsSegment {
                    name: "ROOT".into(),
                    parent_key: None,
                    data: vec![b'A', 0, 0xff],
                }],
                affected_segments: 0,
                checkpoint_id: None,
                system: None,
            },
            address: Some(address),
        }));
        assert_eq!(
            canonical_result_size(&result, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            502
        );
        assert_eq!(
            canonical_result_digest(&result)
                .unwrap()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "ee1eea1457dcca61a8318540879282c91e93d54a9bf5022583754b0095801821"
        );
        let mut changed = value.clone();
        changed.search = Some(ImsGsamSearchArgument::Beginning);
        assert_ne!(
            digest,
            canonical_request_digest(&HostRequest::ImsGsam(changed)).unwrap()
        );
        let mut changed = value.clone();
        changed.context = ImsExecutionContext::Dbctl;
        assert_ne!(
            digest,
            canonical_request_digest(&HostRequest::ImsGsam(changed)).unwrap()
        );
        let mut changed = value;
        changed.save_address = true;
        assert_ne!(
            digest,
            canonical_request_digest(&HostRequest::ImsGsam(changed)).unwrap()
        );
    }
    #[test]
    fn gsam_host_validation_rejects_ambiguous_shapes_and_binds_effect_identity() {
        let limits = HostLimits::default();
        let valid = sample();
        assert_eq!(valid.validate(limits), Ok(()));
        let host = HostRequest::ImsGsam(valid.clone());
        assert!(host.is_mutating());
        assert_eq!(
            host.required_capability(InvocationLimits::default())
                .as_str(),
            "host.ims.write"
        );
        assert_eq!(host.mutation().unwrap().sequence, 7);
        let mut req = valid.clone();
        req.request.mutation = None;
        assert_eq!(req.validate(limits), Err(HostProblem::MissingIdempotency));
        let mut req = valid.clone();
        req.save_address = true;
        assert_eq!(req.validate(limits), Err(HostProblem::Malformed));
        let mut req = valid.clone();
        req.request.qualifiers.push(crate::ImsQualifier {
            segment: "ROOT".into(),
            field: "KEY".into(),
            value: vec![1],
        });
        assert_eq!(req.validate(limits), Err(HostProblem::Malformed));
        let mut req = valid;
        req.search = Some(ImsGsamSearchArgument::Record(ImsGsamAddress {
            database: "GENDB".into(),
            token: [0; 32],
        }));
        assert_eq!(req.validate(limits), Err(HostProblem::Malformed));
    }
}
