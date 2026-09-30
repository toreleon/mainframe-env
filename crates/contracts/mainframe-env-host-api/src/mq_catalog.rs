//! Identity-only registry for the pinned IBM MQ 9.4 MQI denominator.

/// One normalized call identity in the pinned IBM MQ 9.4 MQI catalog.
///
/// Registry presence preserves denominator and source provenance. It does not
/// register a handler, advertise execution, or grant conformance credit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiCallIdentityDescriptor {
    /// Stable official row identity from the normalized 0.2 MQ catalog.
    pub official_row: &'static str,
    /// IBM MQ call symbol.
    pub label: &'static str,
    /// Pinned IBM MQ 9.4 topic used for later semantic review.
    pub topic_path: &'static str,
    /// Bare lowercase SHA-256 pin for `topic_path`.
    pub topic_sha256: &'static str,
    /// One-based positions in the 27-row call-list topic.
    ///
    /// `MQMHBUF` intentionally has two positions; every other call has one.
    pub source_positions: &'static [u16],
}

mod generated {
    use super::MqMqiCallIdentityDescriptor;

    include!("generated/mq_mqi_calls.rs");
}

/// Number of unique mandatory MQI calls in the pinned denominator.
pub const MQ_MQI_CALL_COUNT: usize = 26;
/// Number of displayed call rows retained from the pinned source list.
pub const MQ_MQI_SOURCE_ROW_COUNT: usize = 27;

/// SHA-256 identity of the generated MQI call identity/provenance set.
///
/// This is not a handler-registry, semantic-completion, coverage, or licensed
/// differential receipt.
pub const MQ_MQI_CALL_IDENTITY_SET_SHA256: &str = generated::MQ_MQI_CALL_IDENTITY_SET_SHA256;

/// Returns every unique pinned MQI call in official-row order.
#[must_use]
pub const fn mq_mqi_call_identities() -> &'static [MqMqiCallIdentityDescriptor] {
    generated::MQ_MQI_CALL_IDENTITIES
}

/// Resolves one MQI call by its official row identity.
#[must_use]
pub fn mq_mqi_call_identity(official_row: &str) -> Option<&'static MqMqiCallIdentityDescriptor> {
    let identities = mq_mqi_call_identities();
    identities
        .binary_search_by_key(&official_row, |descriptor| descriptor.official_row)
        .ok()
        .map(|index| &identities[index])
}

/// Resolves one MQI call by its exact IBM call symbol.
#[must_use]
pub fn mq_mqi_call_identity_by_label(label: &str) -> Option<&'static MqMqiCallIdentityDescriptor> {
    let identities = mq_mqi_call_identities();
    identities
        .binary_search_by_key(&label, |descriptor| descriptor.label)
        .ok()
        .map(|index| &identities[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::official_semantic_identity;
    use sha2::{Digest, Sha256};

    fn digest_field(hasher: &mut Sha256, value: &[u8]) {
        hasher.update(u64::try_from(value.len()).unwrap().to_be_bytes());
        hasher.update(value);
    }

    #[test]
    fn call_denominator_is_exact_sorted_and_round_trips() {
        let identities = mq_mqi_call_identities();
        assert_eq!(identities.len(), MQ_MQI_CALL_COUNT);
        assert_eq!(MQ_MQI_CALL_COUNT, 26);
        assert_eq!(
            identities
                .iter()
                .map(|descriptor| descriptor.source_positions.len())
                .sum::<usize>(),
            MQ_MQI_SOURCE_ROW_COUNT
        );
        assert!(
            identities
                .windows(2)
                .all(|pair| pair[0].official_row < pair[1].official_row)
        );
        assert!(
            identities
                .windows(2)
                .all(|pair| pair[0].label < pair[1].label)
        );

        for descriptor in identities {
            let official = official_semantic_identity(descriptor.official_row)
                .expect("generated MQI row must be an official identity");
            assert_eq!(official.baseline, "ibm-mq-9.4-mqi-2026-08-31");
            assert_eq!(official.subsystem, "mq");
            assert_eq!(official.unit, "mqi-calls-unique");
            assert_eq!(official.label, descriptor.label);
            assert_eq!(
                mq_mqi_call_identity(descriptor.official_row),
                Some(descriptor)
            );
            assert_eq!(
                mq_mqi_call_identity_by_label(descriptor.label),
                Some(descriptor)
            );
            assert_eq!(descriptor.topic_sha256.len(), 64);
        }
    }

    #[test]
    fn duplicate_source_row_is_provenance_not_denominator_credit() {
        let descriptor = mq_mqi_call_identity_by_label("MQMHBUF").unwrap();
        assert_eq!(descriptor.source_positions, [18, 25]);
        assert_eq!(
            mq_mqi_call_identities()
                .iter()
                .filter(|candidate| candidate.label == "MQMHBUF")
                .count(),
            1
        );
    }

    #[test]
    fn unknown_and_unpinned_call_identities_do_not_resolve() {
        assert_eq!(mq_mqi_call_identity("custom:mq.call@1"), None);
        assert_eq!(mq_mqi_call_identity_by_label("MQXCNVC"), None);
        assert_eq!(mq_mqi_call_identity_by_label("mqput"), None);
    }

    #[test]
    fn identity_digest_covers_every_logical_field() {
        let identities = mq_mqi_call_identities();
        let mut hasher = Sha256::new();
        hasher.update(b"mainframe-env.mq-mqi-call-identities@1\0");
        hasher.update(u64::try_from(identities.len()).unwrap().to_be_bytes());
        for descriptor in identities {
            digest_field(&mut hasher, descriptor.official_row.as_bytes());
            digest_field(&mut hasher, descriptor.label.as_bytes());
            digest_field(&mut hasher, descriptor.topic_path.as_bytes());
            digest_field(&mut hasher, descriptor.topic_sha256.as_bytes());
            hasher.update(
                u64::try_from(descriptor.source_positions.len())
                    .unwrap()
                    .to_be_bytes(),
            );
            for position in descriptor.source_positions {
                hasher.update(position.to_be_bytes());
            }
        }
        assert_eq!(
            MQ_MQI_CALL_IDENTITY_SET_SHA256,
            format!("sha256:{:x}", hasher.finalize())
        );
    }
}
