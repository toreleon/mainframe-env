//! Generated identity contracts for pinned IBM MQ 9.4 MQI signatures.
//!
//! These descriptors freeze source-reviewed parameter and symbolic identities.
//! They do not register handlers, validate option legality, advertise runtime
//! support, or grant behavioral or licensed differential credit.

/// Recorded availability of a pinned MQI call topic during the catalog review.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MqMqiSourceStatus {
    /// The retained topic body matched its manifest byte count and SHA-256 pin.
    Verified,
    /// The exact pinned topic body was absent from the retained offline archive.
    Missing,
}

#[cfg(test)]
impl MqMqiSourceStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Missing => "missing",
        }
    }
}

/// Source-review state of an MQI parameter signature.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MqMqiSignatureStatus {
    /// The ordered signature was projected from a digest-verified call topic.
    SourceVerified,
    /// Projection is blocked until the exact pinned call topic is available.
    PendingSource,
}

#[cfg(test)]
impl MqMqiSignatureStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SourceVerified => "source-verified",
            Self::PendingSource => "pending-source",
        }
    }
}

/// Direction stated for one language-neutral MQI parameter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MqMqiParameterDirection {
    Input,
    Output,
    InputOutput,
}

#[cfg(test)]
impl MqMqiParameterDirection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::InputOutput => "input-output",
        }
    }
}

/// Identity dimension carried by one MQI parameter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MqMqiParameterRole {
    Scalar,
    Name,
    Length,
    Data,
    Handle,
    Structure,
    Options,
    Selector,
    CompletionCode,
    ReasonCode,
}

#[cfg(test)]
impl MqMqiParameterRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Name => "name",
            Self::Length => "length",
            Self::Data => "data",
            Self::Handle => "handle",
            Self::Structure => "structure",
            Self::Options => "options",
            Self::Selector => "selector",
            Self::CompletionCode => "completion",
            Self::ReasonCode => "reason",
        }
    }
}

/// Resource role assigned to a handle parameter independently of its wire type.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MqMqiHandleRole {
    Connection,
    Object,
    Subscription,
    Message,
}

#[cfg(test)]
impl MqMqiHandleRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Connection => "connection",
            Self::Object => "object",
            Self::Subscription => "subscription",
            Self::Message => "message",
        }
    }
}

/// Lifetime action represented by an MQI handle parameter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MqMqiHandleAction {
    Use,
    Create,
    Release,
    UseOrCreate,
}

#[cfg(test)]
impl MqMqiHandleAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Use => "use",
            Self::Create => "create",
            Self::Release => "release",
            Self::UseOrCreate => "use-or-create",
        }
    }
}

/// Reviewed normalization of one literal type spelling in a pinned call topic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiSourceSpellingAnomaly {
    /// Topic containing the literal anomalous spelling.
    pub source_topic_path: &'static str,
    pub source_topic_sha256: &'static str,
    pub source_line: u16,
    pub literal_data_type: &'static str,
    /// Corroborating topic that defines the canonical handle and its consumers.
    pub canonical_topic_path: &'static str,
    pub canonical_topic_sha256: &'static str,
    pub canonical_first_line: u16,
    pub canonical_last_line: u16,
}

/// One ordered parameter in a pinned language-neutral MQI signature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiParameterDescriptor {
    /// One-based position in the language-neutral signature.
    pub position: u16,
    /// Exact parameter spelling in the pinned syntax line.
    pub name: &'static str,
    /// Exact language-neutral type spelling in the pinned parameter table.
    pub data_type: &'static str,
    pub direction: MqMqiParameterDirection,
    /// Orthogonal identity dimensions carried by the parameter.
    pub roles: &'static [MqMqiParameterRole],
    /// Symbol families associated with a structure, option, selector, or status.
    pub symbolic_identities: &'static [&'static str],
    pub handle_role: Option<MqMqiHandleRole>,
    pub handle_action: Option<MqMqiHandleAction>,
    /// Symbolic root of the structure's version family; this is not a version
    /// legality matrix or a claim that every numeric version is supported.
    pub structure_version_identity: Option<&'static str>,
    /// Literal source spelling retained when another pinned topic establishes
    /// the canonical identity used by this descriptor.
    pub source_spelling_anomaly: Option<MqMqiSourceSpellingAnomaly>,
}

impl MqMqiParameterDescriptor {
    #[must_use]
    pub fn has_role(&self, role: MqMqiParameterRole) -> bool {
        self.roles.contains(&role)
    }
}

/// Source-bound identity contract for one unique MQI call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiContractDescriptor {
    pub official_row: &'static str,
    pub label: &'static str,
    pub topic_path: &'static str,
    pub topic_sha256: &'static str,
    pub source_status: MqMqiSourceStatus,
    pub signature_status: MqMqiSignatureStatus,
    pub parameters: &'static [MqMqiParameterDescriptor],
}

impl MqMqiContractDescriptor {
    /// Resolves one exact parameter name within this signature.
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<&'static MqMqiParameterDescriptor> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
    }
}

mod generated {
    use super::{
        MqMqiContractDescriptor, MqMqiHandleAction, MqMqiHandleRole, MqMqiParameterDescriptor,
        MqMqiParameterDirection, MqMqiParameterRole, MqMqiSignatureStatus,
        MqMqiSourceSpellingAnomaly, MqMqiSourceStatus,
    };

    include!("generated/mq_mqi_contracts.rs");
}

/// Number of unique MQI call contracts. The 27th source row remains duplicate
/// provenance in the separate call identity registry.
pub const MQ_MQI_CONTRACT_COUNT: usize = 26;
/// Number of source-verified parameter identities across all 26 call topics.
pub const MQ_MQI_PARAMETER_COUNT: usize = generated::MQ_MQI_PARAMETER_COUNT;
/// Number of call topics with source-verified signatures.
pub const MQ_MQI_VERIFIED_SIGNATURE_COUNT: usize = 26;
/// Number of pinned call topics whose exact body remains unavailable.
pub const MQ_MQI_PENDING_SIGNATURE_COUNT: usize = 0;
/// SHA-256 identity of the readable normative catalog bytes.
pub const MQ_MQI_CONTRACT_CATALOG_SHA256: &str = generated::MQ_MQI_CONTRACT_CATALOG_SHA256;
/// SHA-256 identity of every logical generated contract field.
pub const MQ_MQI_CONTRACT_SET_SHA256: &str = generated::MQ_MQI_CONTRACT_SET_SHA256;

/// Returns all unique MQI contracts in official-row and label order.
#[must_use]
pub const fn mq_mqi_contracts() -> &'static [MqMqiContractDescriptor] {
    generated::MQ_MQI_CONTRACTS
}

/// Resolves one MQI contract by official row identity.
#[must_use]
pub fn mq_mqi_contract(official_row: &str) -> Option<&'static MqMqiContractDescriptor> {
    let contracts = mq_mqi_contracts();
    contracts
        .binary_search_by_key(&official_row, |descriptor| descriptor.official_row)
        .ok()
        .map(|index| &contracts[index])
}

/// Resolves one MQI contract by exact IBM call symbol.
#[must_use]
pub fn mq_mqi_contract_by_label(label: &str) -> Option<&'static MqMqiContractDescriptor> {
    let contracts = mq_mqi_contracts();
    contracts
        .binary_search_by_key(&label, |descriptor| descriptor.label)
        .ok()
        .map(|index| &contracts[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mq_mqi_call_identity;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;

    fn digest_field(hasher: &mut Sha256, value: &[u8]) {
        hasher.update(u64::try_from(value.len()).unwrap().to_be_bytes());
        hasher.update(value);
    }

    fn digest_optional(hasher: &mut Sha256, value: Option<&str>) {
        digest_field(hasher, value.unwrap_or_default().as_bytes());
    }

    fn digest_anomaly(hasher: &mut Sha256, value: Option<MqMqiSourceSpellingAnomaly>) {
        let Some(value) = value else {
            hasher.update([0]);
            return;
        };
        hasher.update([1]);
        for field in [
            value.source_topic_path,
            value.source_topic_sha256,
            value.literal_data_type,
            value.canonical_topic_path,
            value.canonical_topic_sha256,
        ] {
            digest_field(hasher, field.as_bytes());
        }
        for line in [
            value.source_line,
            value.canonical_first_line,
            value.canonical_last_line,
        ] {
            hasher.update(line.to_be_bytes());
        }
    }

    #[test]
    fn contracts_are_exact_sorted_and_join_the_call_denominator() {
        let contracts = mq_mqi_contracts();
        assert_eq!(contracts.len(), MQ_MQI_CONTRACT_COUNT);
        assert_eq!(contracts.len(), 26);
        assert_eq!(
            contracts
                .iter()
                .map(|contract| contract.parameters.len())
                .sum::<usize>(),
            MQ_MQI_PARAMETER_COUNT
        );
        assert_eq!(MQ_MQI_PARAMETER_COUNT, 169);
        assert!(
            contracts
                .windows(2)
                .all(|pair| pair[0].official_row < pair[1].official_row)
        );
        assert!(
            contracts
                .windows(2)
                .all(|pair| pair[0].label < pair[1].label)
        );

        for contract in contracts {
            let identity = mq_mqi_call_identity(contract.official_row).unwrap();
            assert_eq!(identity.label, contract.label);
            assert_eq!(identity.topic_path, contract.topic_path);
            assert_eq!(identity.topic_sha256, contract.topic_sha256);
            assert_eq!(mq_mqi_contract(contract.official_row), Some(contract));
            assert_eq!(mq_mqi_contract_by_label(contract.label), Some(contract));
            assert!(
                contract
                    .parameters
                    .iter()
                    .enumerate()
                    .all(|(index, parameter)| parameter.position
                        == u16::try_from(index + 1).unwrap())
            );
        }
    }

    #[test]
    fn mqinq_has_source_verified_output_attributes() {
        let verified = mq_mqi_contracts()
            .iter()
            .filter(|contract| contract.source_status == MqMqiSourceStatus::Verified)
            .count();
        let pending = mq_mqi_contracts()
            .iter()
            .filter(|contract| contract.signature_status == MqMqiSignatureStatus::PendingSource)
            .collect::<Vec<_>>();
        assert_eq!(verified, MQ_MQI_VERIFIED_SIGNATURE_COUNT);
        assert_eq!(pending.len(), MQ_MQI_PENDING_SIGNATURE_COUNT);
        let mqinq = mq_mqi_contract_by_label("MQINQ").unwrap();
        assert_eq!(mqinq.signature_status, MqMqiSignatureStatus::SourceVerified);
        assert_eq!(mqinq.parameters.len(), 10);
        assert_eq!(
            mqinq.parameter("IntAttrs").unwrap().direction,
            MqMqiParameterDirection::Output
        );
        assert_eq!(
            mqinq.parameter("CharAttrs").unwrap().direction,
            MqMqiParameterDirection::Output
        );
    }

    #[test]
    fn representative_structure_status_selector_and_handle_identities_are_exact() {
        let begin_options = mq_mqi_contract_by_label("MQBEGIN")
            .unwrap()
            .parameter("BeginOptions")
            .unwrap();
        assert_eq!(begin_options.data_type, "MQBO");
        assert!(begin_options.has_role(MqMqiParameterRole::Structure));
        assert!(begin_options.has_role(MqMqiParameterRole::Options));
        assert_eq!(
            begin_options.structure_version_identity,
            Some("MQBO_VERSION")
        );

        let selectors = mq_mqi_contract_by_label("MQSET")
            .unwrap()
            .parameter("Selectors")
            .unwrap();
        assert_eq!(selectors.symbolic_identities, ["MQIA", "MQCA"]);
        assert!(selectors.has_role(MqMqiParameterRole::Selector));

        let commit = mq_mqi_contract_by_label("MQCMIT").unwrap();
        assert_eq!(
            commit.parameter("CompCode").unwrap().symbolic_identities,
            ["MQCC"]
        );
        assert_eq!(
            commit.parameter("Reason").unwrap().symbolic_identities,
            ["MQRC"]
        );

        let subscription = mq_mqi_contract_by_label("MQSUB")
            .unwrap()
            .parameter("Hsub")
            .unwrap();
        assert_eq!(
            subscription.handle_role,
            Some(MqMqiHandleRole::Subscription)
        );
        assert_eq!(subscription.handle_action, Some(MqMqiHandleAction::Create));

        let documented_message_handle = mq_mqi_contract_by_label("MQBUFMH")
            .unwrap()
            .parameter("Hmsg")
            .unwrap();
        assert_eq!(documented_message_handle.data_type, "MQHMSG");
        assert_eq!(documented_message_handle.symbolic_identities, ["MQHMSG"]);
        assert_eq!(
            documented_message_handle.handle_role,
            Some(MqMqiHandleRole::Message)
        );
        assert_eq!(
            documented_message_handle.source_spelling_anomaly,
            Some(MqMqiSourceSpellingAnomaly {
                source_topic_path: "SSFKSJ_9.4.0/refdev/q101710_.html",
                source_topic_sha256: "8a94879a9c9f2e18ddb2171b0dd5ea477ebaf26684a760c9e1e31931f2084156",
                source_line: 13,
                literal_data_type: "MQHMQSG",
                canonical_topic_path: "SSFKSJ_9.4.0/refdev/q101780_.html",
                canonical_topic_sha256: "66cf482573408e227aec23ec2219cd52bf7bf879591f33acb04dc3f33fb386db",
                canonical_first_line: 37,
                canonical_last_line: 49,
            })
        );
    }

    #[test]
    fn callback_has_no_status_pair_and_other_verified_calls_have_one() {
        for contract in mq_mqi_contracts() {
            let completions = contract
                .parameters
                .iter()
                .filter(|parameter| parameter.has_role(MqMqiParameterRole::CompletionCode))
                .count();
            let reasons = contract
                .parameters
                .iter()
                .filter(|parameter| parameter.has_role(MqMqiParameterRole::ReasonCode))
                .count();
            let expected = usize::from(
                contract.signature_status == MqMqiSignatureStatus::SourceVerified
                    && contract.label != "MQCB_FUNCTION",
            );
            assert_eq!(
                (completions, reasons),
                (expected, expected),
                "{}",
                contract.label
            );
        }
    }

    #[test]
    fn every_identity_dimension_is_present_without_advertising_handlers() {
        let roles = mq_mqi_contracts()
            .iter()
            .flat_map(|contract| contract.parameters)
            .flat_map(|parameter| parameter.roles)
            .copied()
            .collect::<BTreeSet<_>>();
        for role in [
            MqMqiParameterRole::Handle,
            MqMqiParameterRole::Structure,
            MqMqiParameterRole::Options,
            MqMqiParameterRole::Selector,
            MqMqiParameterRole::CompletionCode,
            MqMqiParameterRole::ReasonCode,
        ] {
            assert!(roles.contains(&role));
        }
        assert_eq!(mq_mqi_contract_by_label("MQXCNVC"), None);
        assert_eq!(mq_mqi_contract_by_label("mqput"), None);
    }

    #[test]
    fn contract_digest_covers_every_generated_field() {
        let contracts = mq_mqi_contracts();
        let mut hasher = Sha256::new();
        hasher.update(b"mainframe-env.mq-mqi-contract-identities@1\0");
        hasher.update(u64::try_from(contracts.len()).unwrap().to_be_bytes());
        for contract in contracts {
            for value in [
                contract.official_row,
                contract.label,
                contract.topic_path,
                contract.topic_sha256,
                contract.source_status.as_str(),
                contract.signature_status.as_str(),
            ] {
                digest_field(&mut hasher, value.as_bytes());
            }
            hasher.update(
                u64::try_from(contract.parameters.len())
                    .unwrap()
                    .to_be_bytes(),
            );
            for parameter in contract.parameters {
                hasher.update(parameter.position.to_be_bytes());
                for value in [
                    parameter.name,
                    parameter.data_type,
                    parameter.direction.as_str(),
                ] {
                    digest_field(&mut hasher, value.as_bytes());
                }
                hasher.update(u64::try_from(parameter.roles.len()).unwrap().to_be_bytes());
                for role in parameter.roles {
                    digest_field(&mut hasher, role.as_str().as_bytes());
                }
                hasher.update(
                    u64::try_from(parameter.symbolic_identities.len())
                        .unwrap()
                        .to_be_bytes(),
                );
                for identity in parameter.symbolic_identities {
                    digest_field(&mut hasher, identity.as_bytes());
                }
                digest_optional(
                    &mut hasher,
                    parameter.handle_role.map(MqMqiHandleRole::as_str),
                );
                digest_optional(
                    &mut hasher,
                    parameter.handle_action.map(MqMqiHandleAction::as_str),
                );
                digest_optional(&mut hasher, parameter.structure_version_identity);
                digest_anomaly(&mut hasher, parameter.source_spelling_anomaly);
            }
        }
        assert_eq!(
            MQ_MQI_CONTRACT_SET_SHA256,
            format!("sha256:{:x}", hasher.finalize())
        );
    }
}
