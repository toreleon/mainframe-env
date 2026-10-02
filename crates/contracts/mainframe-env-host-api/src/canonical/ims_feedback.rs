use super::*;
use crate::{
    ImsPcbFeedbackRequestV1, ImsPcbFeedbackResultV1, ImsPcbFeedbackUnsupportedV1, ImsPcbFeedbackV1,
    ImsPcbKeyFeedbackV1,
};

impl Canonical for ImsPcbFeedbackRequestV1 {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsPcbFeedbackRequestV1", 4)?;
        out.text("context")?;
        self.context.encode(out)?;
        out.text("key_capacity")?;
        self.key_capacity.encode(out)?;
        out.text("request")?;
        self.request.encode(out)?;
        out.text("ssas")?;
        self.ssas.encode(out)
    }
}
impl Canonical for ImsPcbFeedbackUnsupportedV1 {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let name = match self {
            Self::FailedCallWitness => "FailedCallWitness",
            Self::SecondarySequence => "SecondarySequence",
            Self::NonKeyOperation => "NonKeyOperation",
            Self::MissingSequenceField => "MissingSequenceField",
            Self::LogicalRelationship => "LogicalRelationship",
        };
        out.variant("ImsPcbFeedbackUnsupportedV1", name, 0)
    }
}
impl Canonical for ImsPcbKeyFeedbackV1 {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Valid {
                segment_name,
                segment_level,
                bytes,
            } => {
                out.variant("ImsPcbKeyFeedbackV1", "Valid", 3)?;
                out.text("bytes")?;
                bytes.encode(out)?;
                out.text("segment_level")?;
                segment_level.encode(out)?;
                out.text("segment_name")?;
                segment_name.encode(out)
            }
            Self::InvalidatedSecondaryReplace => {
                out.variant("ImsPcbKeyFeedbackV1", "InvalidatedSecondaryReplace", 0)
            }
            Self::Unsupported(reason) => {
                out.variant("ImsPcbKeyFeedbackV1", "Unsupported", 1)?;
                out.text("0")?;
                reason.encode(out)
            }
        }
    }
}
impl Canonical for ImsPcbFeedbackV1 {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsPcbFeedbackV1", 6)?;
        out.text("database")?;
        self.database.encode(out)?;
        out.text("key")?;
        self.key.encode(out)?;
        out.text("pcb")?;
        self.pcb.encode(out)?;
        out.text("processing_options")?;
        self.processing_options.encode(out)?;
        out.text("sensitive_segment_count")?;
        self.sensitive_segment_count.encode(out)?;
        out.text("transferred_data_length")?;
        self.transferred_data_length.encode(out)
    }
}
impl Canonical for ImsPcbFeedbackResultV1 {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsPcbFeedbackResultV1", 2)?;
        out.text("feedback")?;
        self.feedback.encode(out)?;
        out.text("result")?;
        self.result.encode(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImsExecutionContext, ImsOperation, ImsRequest, ImsResult, ImsSegment, Mutation};
    use mainframe_env_execution_api::InvocationLimits;

    fn sample() -> ImsPcbFeedbackRequestV1 {
        ImsPcbFeedbackRequestV1 {
            context: ImsExecutionContext::DbBatch,
            key_capacity: 8,
            ssas: None,
            request: ImsRequest {
                operation: ImsOperation::GetUnique,
                psb: None,
                pcb: 2,
                segments: vec!["CHILD".into()],
                data: vec![],
                qualifiers: vec![],
                checkpoint_id: None,
                max_segments: 1,
                system: None,
                q_class: None,
                mutation: Some(Mutation {
                    sequence: 7,
                    idempotency_key: IdempotencyKey::new(
                        "feedback-golden",
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    transaction: None,
                }),
            },
        }
    }
    fn response() -> ImsPcbFeedbackResultV1 {
        ImsPcbFeedbackResultV1 {
            result: ImsResult {
                status: "  ".into(),
                segments: vec![ImsSegment {
                    name: "CHILD".into(),
                    parent_key: Some(vec![0xc1, 0]),
                    data: vec![0xc1, 0, 0xff],
                }],
                checkpoint_id: None,
                affected_segments: 0,
                system: None,
            },
            feedback: ImsPcbFeedbackV1 {
                pcb: 2,
                database: "GENDB".into(),
                processing_options: "AP".into(),
                sensitive_segment_count: 2,
                transferred_data_length: 3,
                key: ImsPcbKeyFeedbackV1::Valid {
                    segment_name: "CHILD".into(),
                    segment_level: 2,
                    bytes: vec![0xc1, 0, 0xff, 0x40],
                },
            },
        }
    }
    fn hex(digest: [u8; 32]) -> String {
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }
    #[test]
    fn feedback_v1_canonical_matches_independent_binary_schema_vectors() {
        // Independent Python encoder of EFFECT-CANONICAL-V1's documented tags,
        // framing and field order: external feedback_goldens.py receipt.
        let request = HostRequest::ImsPcbFeedbackV1(sample());
        assert_eq!(
            canonical_request_size(&request, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            685
        );
        let digest = canonical_request_digest(&request).unwrap();
        assert_eq!(
            hex(digest),
            "47f158c3d7ab302b61a07f9032149a79896348ab41a65c1a68b153af46b7ee06"
        );
        assert_ne!(
            digest,
            canonical_ims_request_digest(&sample().request).unwrap()
        );
        for edit in 0..4 {
            let mut changed = sample();
            match edit {
                0 => changed.key_capacity += 1,
                1 => changed.request.pcb = 1,
                2 => changed.context = ImsExecutionContext::Dbctl,
                _ => changed.ssas = Some(vec![]),
            }
            assert_ne!(
                digest,
                canonical_request_digest(&HostRequest::ImsPcbFeedbackV1(changed)).unwrap()
            );
        }
        let result = Ok(HostResult::ImsPcbFeedbackV1(response()));
        assert_eq!(
            canonical_result_size(&result, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            760
        );
        assert_eq!(
            hex(canonical_result_digest(&result).unwrap()),
            "d9c22d43f47036fc6da003ea6d9bda6d61c634caba09c6072c909d905e617cc6"
        );
        let mut unsupported = response();
        unsupported.feedback.key =
            ImsPcbKeyFeedbackV1::Unsupported(ImsPcbFeedbackUnsupportedV1::FailedCallWitness);
        assert_ne!(
            canonical_result_digest(&result).unwrap(),
            canonical_result_digest(&Ok(HostResult::ImsPcbFeedbackV1(unsupported))).unwrap()
        );
    }
    #[test]
    fn feedback_v1_validates_bounds_status_length_and_unknown_fields() {
        let limits = HostLimits::default();
        let host = HostRequest::ImsPcbFeedbackV1(sample());
        assert!(host.is_mutating());
        assert_eq!(
            host.required_capability(InvocationLimits::default())
                .as_str(),
            "host.ims.write"
        );
        assert_eq!(host.mutation().unwrap().sequence, 7);
        assert_eq!(host.validate(limits), Ok(()));
        let mut request = sample();
        request.request.mutation = None;
        assert_eq!(
            request.validate(limits),
            Err(HostProblem::MissingIdempotency)
        );
        let mut request = sample();
        request.key_capacity = u32::MAX;
        assert_eq!(
            request.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
        let mut request = sample();
        request.ssas = Some(vec![]);
        assert_eq!(request.validate(limits), Err(HostProblem::Malformed));
        assert_eq!(response().validate(limits), Ok(()));
        let mut result = response();
        result.feedback.transferred_data_length += 1;
        assert_eq!(result.validate(limits), Err(HostProblem::Malformed));
        let mut result = response();
        result.result.status = "GE".into();
        assert_eq!(result.validate(limits), Err(HostProblem::Malformed));
        result.feedback.key =
            ImsPcbKeyFeedbackV1::Unsupported(ImsPcbFeedbackUnsupportedV1::FailedCallWitness);
        assert_eq!(result.validate(limits), Ok(()));
        let mut json = serde_json::to_value(response().feedback).unwrap();
        json["raw_mask"] = serde_json::json!([]);
        assert!(serde_json::from_value::<ImsPcbFeedbackV1>(json).is_err());
    }
}
