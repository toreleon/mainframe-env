use super::contracts::{TmDefinitionSet, TmInputMessage, TmPcbStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmInstallReceipt {
    pub transactions: usize,
    pub identity: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmEnqueueReceipt {
    pub message_id: String,
    pub sequence: u64,
    pub work_id: String,
    pub conversation_id: Option<String>,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmScheduleReceipt {
    pub message_id: String,
    pub transaction: String,
    pub source: String,
    pub conversation_id: Option<String>,
    pub spa: Vec<u8>,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmCallResult {
    pub status: TmPcbStatus,
    pub segment: Option<Vec<u8>>,
    pub destination: Option<String>,
    pub pcb: Option<TmPcbView>,
    pub output_message_ids: Vec<String>,
    pub conversation_id: Option<String>,
    pub replayed: bool,
}

impl TmCallResult {
    pub(crate) fn status(status: TmPcbStatus) -> Self {
        Self {
            status,
            segment: None,
            destination: None,
            pcb: None,
            output_message_ids: Vec::new(),
            conversation_id: None,
            replayed: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmPcbView {
    Io {
        logical_terminal: String,
        status: TmPcbStatus,
        message_sequence: u64,
        user_id: Option<String>,
        group_name: Option<String>,
    },
    Alternate {
        name: String,
        destination: Option<String>,
        status: TmPcbStatus,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmCancelReceipt {
    pub message_id: String,
    pub state: TmMessageState,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmOutboundMessage {
    pub message_id: String,
    pub destination: String,
    pub segments: Vec<Vec<u8>>,
    pub express: bool,
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmConversationView {
    pub conversation_id: String,
    pub next_transaction: String,
    pub spa: Vec<u8>,
    pub step: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmMessageState {
    AdmissionPending,
    Scheduled,
    InFlight,
    Completed,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogRow {
    pub definitions: TmDefinitionSet,
    pub next_sequence: u64,
    #[serde(default)]
    pub package_binding: Option<String>,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default = "catalog_active")]
    pub active: bool,
}

const fn catalog_active() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageDefinitionsRow {
    pub application: String,
    pub generation: u64,
    pub package_identity: String,
    pub definitions: TmDefinitionSet,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageRow {
    pub message: TmInputMessage,
    pub sequence: u64,
    pub enqueue_tick: u64,
    pub deadline_tick: u64,
    pub principal: String,
    pub state: TmMessageState,
    pub work_id: String,
    pub request_digest: [u8; 32],
    pub new_conversation: bool,
    pub run_unit: Option<String>,
    #[serde(default)]
    pub package_binding: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionRow {
    pub run_unit: String,
    pub principal: String,
    pub message_id: String,
    pub transaction: String,
    pub io_destination: String,
    pub message_sequence: u64,
    pub user_id: Option<String>,
    pub group_name: Option<String>,
    pub input_index: Option<usize>,
    pub io_status: TmPcbStatus,
    pub alternate_destinations: BTreeMap<String, Option<String>>,
    pub output_buffers: BTreeMap<String, OutputBuffer>,
    pub pending_output_ids: Vec<String>,
    pub conversation_id: Option<String>,
    pub work_id: String,
    pub lease_id: String,
    pub lease_epoch: u64,
    #[serde(default)]
    pub package_binding: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputBuffer {
    pub destination: String,
    pub express: bool,
    pub segments: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationRow {
    pub conversation_id: String,
    pub principal: String,
    pub next_transaction: String,
    pub spa: Vec<u8>,
    pub step: u64,
    #[serde(default)]
    pub package_binding: Option<String>,
}

pub(crate) struct ConversationStart {
    pub current_version: Option<u64>,
    pub row: ConversationRow,
    pub spa: Vec<u8>,
}

impl ConversationRow {
    pub(crate) fn view(&self) -> TmConversationView {
        TmConversationView {
            conversation_id: self.conversation_id.clone(),
            next_transaction: self.next_transaction.clone(),
            spa: self.spa.clone(),
            step: self.step,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutboundRow {
    pub message: TmOutboundMessage,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ReplayResult {
    Enqueue(TmEnqueueReceipt),
    Schedule(TmScheduleReceipt),
    Call(TmCallResult),
    Cancel(TmCancelReceipt),
}

impl ReplayResult {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Call(result) => result.status.valid(),
            Self::Enqueue(_) | Self::Schedule(_) | Self::Cancel(_) => true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayRow {
    pub request_digest: [u8; 32],
    pub result: ReplayResult,
    pub work: Option<ReplayWork>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayWork {
    pub work_id: String,
    pub lease_id: String,
    pub lease_epoch: u64,
    pub disposition: WorkDisposition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WorkDisposition {
    Complete,
    Release,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkPayload {
    pub schema_version: String,
    pub message_id: String,
    pub transaction: String,
}
