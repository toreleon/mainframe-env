//! Task-local presentation facts for extraction. Protocol state remains in
//! `ConversationLedger`, owned by conversation-open.

use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const NAMESPACE: &str = "cics-conversation-extract-v1";
pub(super) const MAX_ROW_BYTES: usize = 8192;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LuName {
    pub token: [u8; 4],
    pub sysid: String,
    pub termid: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MutationReplay {
    pub key: String,
    pub request_sha256: String,
    pub outputs: BTreeMap<String, Vec<u8>>,
    pub output_schemas: BTreeMap<String, String>,
}

/// Facts supplied by terminal/session attach, RECEIVE, and POINT. This row
/// does not contain protocol kind, state, process, PIP, or ownership.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtractMetadata {
    pub schema_version: u8,
    pub run_unit: String,
    pub selected_token: Option<[u8; 4]>,
    pub session_names: BTreeMap<String, [u8; 4]>,
    pub netnames: BTreeMap<String, LuName>,
    pub received_attach: Option<String>,
    /// The task was attached by inbound APPC terminal data.
    pub network_attached: bool,
    pub logon_message: Option<Vec<u8>>,
    pub logon_consumed: bool,
    pub(super) last_mutation: Option<MutationReplay>,
}

impl ExtractMetadata {
    pub(crate) fn for_run_unit(run_unit: String) -> Self {
        Self {
            schema_version: 1,
            run_unit,
            selected_token: None,
            session_names: BTreeMap::new(),
            netnames: BTreeMap::new(),
            received_attach: None,
            network_attached: false,
            logon_message: None,
            logon_consumed: false,
            last_mutation: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.run_unit.is_empty()
            || self.run_unit.len() > 128
            || self.session_names.len() > 16
            || self.netnames.len() > 16
            || self
                .logon_message
                .as_ref()
                .is_some_and(|bytes| bytes.len() > 256)
            || self
                .received_attach
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 8)
            || self
                .session_names
                .iter()
                .any(|(name, token)| name.is_empty() || name.len() > 4 || *token == [0; 4])
            || self.netnames.iter().any(|(name, entry)| {
                name.len() != 8
                    || entry.token == [0; 4]
                    || entry.sysid.len() != 4
                    || entry.termid.len() != 4
            })
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if let Some(replay) = &self.last_mutation {
            if replay.key.is_empty()
                || replay.key.len() > 256
                || replay.request_sha256.len() != 64
                || replay.outputs.len() > 16
                || replay.outputs.keys().collect::<Vec<_>>()
                    != replay.output_schemas.keys().collect::<Vec<_>>()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(())
    }
}
