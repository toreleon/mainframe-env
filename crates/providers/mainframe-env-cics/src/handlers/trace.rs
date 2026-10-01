use mainframe_env_host_api::CicsOperation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTraceEntry {
    pub operation: CicsOperation,
    pub outcome: String,
    pub response: i32,
    pub response2: i32,
    pub payload_bytes: usize,
}
