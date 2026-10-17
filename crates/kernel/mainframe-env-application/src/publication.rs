//! Existing publication recovery bytes shared by composition and selected admission.
use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};

pub const APPLICATION_PUBLICATION_NAMESPACE: &str = "application-publication-v2";
pub const APPLICATION_PUBLICATION_CONTRACT: &str = "mainframe-env.application-publication@1";
pub const MAX_APPLICATION_PUBLICATION_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationSectionState {
    NotApplicable,
    Pending,
    Applying,
    Applied,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationAction {
    Install,
    Rollback,
}

/// Publication data is not a permit: admission must join the actual selected owner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationPublicationState {
    pub schema_version: String,
    pub package: String,
    pub generation: u64,
    pub identity: String,
    pub action: PublicationAction,
    pub controllers: PublicationSectionState,
    pub db2: PublicationSectionState,
    #[serde(default = "publication_not_applicable")]
    pub ims: PublicationSectionState,
    pub complete: bool,
}
const fn publication_not_applicable() -> PublicationSectionState {
    PublicationSectionState::NotApplicable
}

impl ApplicationPublicationState {
    /// Check before decoding, after the store has materialized its bounded row.
    pub fn from_payload(payload: &[u8]) -> Result<Self, HostProblem> {
        if payload.len() > MAX_APPLICATION_PUBLICATION_BYTES {
            return Err(HostProblem::ResourceExhausted);
        }
        let state: Self =
            serde_json::from_slice(payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        if state.schema_version != APPLICATION_PUBLICATION_CONTRACT
            || state.generation == 0
            || state.package.is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(state)
    }

    /// Join a known selected owner. No store access or executable authority is created.
    pub fn matches_complete(&self, package: &str, generation: u64, identity: &str) -> bool {
        let terminal = |section| {
            matches!(
                section,
                PublicationSectionState::Applied | PublicationSectionState::NotApplicable
            )
        };
        self.schema_version == APPLICATION_PUBLICATION_CONTRACT
            && self.package.eq_ignore_ascii_case(package)
            && self.generation == generation
            && self.identity == identity
            && self.complete
            && terminal(self.controllers)
            && terminal(self.db2)
            && terminal(self.ims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HISTORICAL: &[u8] = br#"{"schema_version":"mainframe-env.application-publication@1","package":"APP","generation":1,"identity":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","action":"install","controllers":"applied","db2":"not-applicable","complete":true}"#;

    #[test]
    fn historical_missing_ims_and_unknown_fields_keep_existing_serde_behavior() {
        let state = ApplicationPublicationState::from_payload(HISTORICAL).unwrap();
        assert_eq!(state.ims, PublicationSectionState::NotApplicable);
        let mut extended: serde_json::Value = serde_json::from_slice(HISTORICAL).unwrap();
        extended["future"] = serde_json::Value::Bool(true);
        assert_eq!(
            ApplicationPublicationState::from_payload(&serde_json::to_vec(&extended).unwrap())
                .unwrap(),
            state
        );
        assert_eq!(
            serde_json::to_string(&state).unwrap(),
            "{\"schema_version\":\"mainframe-env.application-publication@1\",\"package\":\"APP\",\"generation\":1,\"identity\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"action\":\"install\",\"controllers\":\"applied\",\"db2\":\"not-applicable\",\"ims\":\"not-applicable\",\"complete\":true}"
        );
    }

    #[test]
    fn complete_tuple_and_terminal_sections_are_independently_required() {
        let valid = ApplicationPublicationState::from_payload(HISTORICAL).unwrap();
        assert!(valid.matches_complete("APP", 1, &valid.identity));
        assert!(!valid.matches_complete("OTHER", 1, &valid.identity));
        assert!(!valid.matches_complete("APP", 2, &valid.identity));
        assert!(!valid.matches_complete("APP", 1, "wrong"));
        for section in [
            PublicationSectionState::Pending,
            PublicationSectionState::Applying,
            PublicationSectionState::Failed,
        ] {
            for field in [0, 1, 2] {
                let mut state = valid.clone();
                match field {
                    0 => state.controllers = section,
                    1 => state.db2 = section,
                    _ => state.ims = section,
                }
                assert!(!state.matches_complete("APP", 1, &valid.identity));
            }
        }
        let mut state = valid.clone();
        state.complete = false;
        assert!(!state.matches_complete("APP", 1, &valid.identity));
    }

    #[test]
    fn malformed_null_and_oversized_publication_payloads_refuse() {
        assert_eq!(
            ApplicationPublicationState::from_payload(b"{"),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(
            ApplicationPublicationState::from_payload(&vec![
                b' ';
                MAX_APPLICATION_PUBLICATION_BYTES + 1
            ]),
            Err(HostProblem::ResourceExhausted)
        );
        let mut value: serde_json::Value = serde_json::from_slice(HISTORICAL).unwrap();
        value["ims"] = serde_json::Value::Null;
        assert_eq!(
            ApplicationPublicationState::from_payload(&serde_json::to_vec(&value).unwrap()),
            Err(HostProblem::InfrastructureFailure)
        );
    }
}
