use crate::model::{AuditFieldValue, DecisionOutcome, SafStatus, SecurityAuditRecord};
use mainframe_env_host_api::HostProblem;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SmfType80Record {
    pub record_type: u16,
    pub sequence: u64,
    pub correlation: String,
    pub actor: String,
    pub action: String,
    pub class: Option<String>,
    pub resource_digest: Option<String>,
    pub decision: DecisionOutcome,
    pub status: SafStatus,
    pub fields: BTreeMap<String, AuditFieldValue>,
    pub tick: u64,
}

pub(crate) fn project_type80(
    records: &[SecurityAuditRecord],
    start: usize,
    max_items: usize,
) -> Result<Vec<SmfType80Record>, HostProblem> {
    if max_items == 0 || max_items > 65_536 || start > records.len() {
        return Err(HostProblem::ResourceExhausted);
    }
    records
        .iter()
        .enumerate()
        .skip(start)
        .take(max_items)
        .map(|(index, record)| {
            Ok(SmfType80Record {
                record_type: 80,
                sequence: u64::try_from(index)
                    .map_err(|_| HostProblem::ResourceExhausted)?
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?,
                correlation: record.correlation.clone(),
                actor: record.actor.clone(),
                action: record.action.clone(),
                class: record.class.clone(),
                resource_digest: record.resource_digest.clone(),
                decision: record.decision,
                status: record.status,
                fields: redact_fields(record.fields.clone()),
                tick: record.tick,
            })
        })
        .collect()
}

pub(crate) fn redact_fields(
    mut fields: BTreeMap<String, AuditFieldValue>,
) -> BTreeMap<String, AuditFieldValue> {
    for (name, value) in &mut fields {
        let redact = sensitive_name(name)
            || match value {
                AuditFieldValue::Text(value) | AuditFieldValue::Reference(value) => {
                    sensitive_value(value)
                }
                AuditFieldValue::Digest(_) | AuditFieldValue::Redacted => false,
            };
        if redact {
            *value = AuditFieldValue::Redacted;
        }
    }
    fields
}

pub(crate) fn sensitive_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "PASSWORD",
        "PHRASE",
        "CREDENTIAL",
        "SECRET",
        "TOKEN",
        "KEY",
        "PRIVATE_KEY",
        "CERTIFICATE",
        "PASSCODE",
        "MFA",
        "ASSERTION",
        "CARD",
        "QUEUE_PAYLOAD",
        "PROTECTED",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

pub(crate) fn sensitive_value(value: &str) -> bool {
    let trimmed = value.trim();
    let upper = trimmed.to_ascii_uppercase();
    upper.starts_with("SECRET:")
        || upper.starts_with("VAULT:")
        || upper.starts_with("KEYRING:")
        || upper.starts_with("$ARGON2")
        || upper.contains("BEGIN PRIVATE KEY")
        || upper.contains("BEGIN CERTIFICATE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_secret_shaped_values_are_redacted_before_projection() {
        let fields = redact_fields(BTreeMap::from([
            (
                "PHRASE".into(),
                AuditFieldValue::Text("never-retain-this".into()),
            ),
            (
                "NOTE".into(),
                AuditFieldValue::Reference("SeCrEt:opaque-reference".into()),
            ),
            (
                "LOCATION".into(),
                AuditFieldValue::Text(" VaUlT:credential-reference ".into()),
            ),
            (
                "RING".into(),
                AuditFieldValue::Reference("KeYrInG:signing-key".into()),
            ),
            (
                "VERIFIER".into(),
                AuditFieldValue::Text("$ArGoN2id$v=19$unsafe".into()),
            ),
            (
                "REQUEST_DIGEST".into(),
                AuditFieldValue::Digest(format!("sha256:{}", "a".repeat(64))),
            ),
        ]));
        assert_eq!(fields["PHRASE"], AuditFieldValue::Redacted);
        assert_eq!(fields["NOTE"], AuditFieldValue::Redacted);
        assert_eq!(fields["LOCATION"], AuditFieldValue::Redacted);
        assert_eq!(fields["RING"], AuditFieldValue::Redacted);
        assert_eq!(fields["VERIFIER"], AuditFieldValue::Redacted);
        assert!(matches!(
            fields["REQUEST_DIGEST"],
            AuditFieldValue::Digest(_)
        ));
    }
}
