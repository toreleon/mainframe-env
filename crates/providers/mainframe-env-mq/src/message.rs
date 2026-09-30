use mainframe_env_host_api::{HostProblem, MqRequest, canonical_mq_request_digest};
use sha2::{Digest, Sha256};

pub(crate) fn canonical_message_id(run: &str, request: &MqRequest) -> Result<Vec<u8>, HostProblem> {
    let request_digest = canonical_mq_request_digest(request)?;
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.mq-message-id@1\0");
    digest.update(u64::try_from(run.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(run.as_bytes());
    digest.update(request_digest);
    Ok(digest.finalize()[..24].to_vec())
}
