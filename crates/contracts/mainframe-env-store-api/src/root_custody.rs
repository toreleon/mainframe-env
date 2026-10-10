//! Storage-only observation of one bounded synchronous root custody cutoff.
//! This is neither a manager permit nor source-known unit disposition.
use crate::{ProviderStateRecord, RootClosureSnapshot, StoreError};
use sha2::{Digest, Sha256};

/// Core-owned persistent proof that all old ordinary writers must stay fenced.
pub const ROOT_CUSTODY_NAMESPACE: &str = "durable-root-custody-v1";

/// Complete old graph captured while Open, and the exact atomic fence result.
/// The graph's legacy `closing` field holds its actual Open ownership row here;
/// it cannot be submitted as a Closing terminal decision. Structural data alone
/// supplies no host/provider custody. Backends and genuine drivers recheck it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootCustodyCutoff {
    /// Complete original storage capture, including audits and ownership indexes.
    pub preserved: RootClosureSnapshot,
    /// Actual Uncertain ownership row; it selects neither commit nor backout.
    pub fenced: ProviderStateRecord,
    /// Core-owned first-cutoff marker, version one, binding the complete capture.
    pub marker: ProviderStateRecord,
    /// Physical epoch immediately after the fence and marker transaction.
    pub fenced_epoch: u64,
}
impl RootCustodyCutoff {
    /// Aggregate all captured and new rows against the original shared budget.
    pub fn validate_bounds(&self) -> Result<(), StoreError> {
        self.captured_usage().map(|_| ())
    }
    /// Complete capture plus the exact retained fence and marker rows.
    /// Successor preparation must start from this aggregate, not only preserved.
    pub fn captured_usage(&self) -> Result<(usize, usize), StoreError> {
        let (mut rows, mut bytes) = self.preserved.captured_usage()?;
        for row in [&self.fenced, &self.marker] {
            row.validate_write(crate::MAX_ROOT_PAYLOAD_BYTES)?;
            rows = rows.checked_add(1).ok_or(StoreError::CapacityExceeded)?;
            bytes = bytes
                .checked_add(row.namespace.len() + row.key.len() + row.payload.len())
                .ok_or(StoreError::CapacityExceeded)?;
        }
        if rows > crate::MAX_ROOT_OPERATIONS || bytes > crate::MAX_ROOT_PAYLOAD_BYTES {
            return Err(StoreError::CapacityExceeded);
        }
        Ok((rows, bytes))
    }
    /// Digest of exact captured storage identities, bytes and versions. This
    /// does not replace canonical HostRequest/audit identities or grant SAF.
    pub fn captured_storage_digest(&self) -> Result<[u8; 32], StoreError> {
        self.validate_bounds()?;
        let mut hash = Sha256::new();
        hash.update(b"mainframe-env.root-custody-capture@1\0");
        {
            let mut row = |r: &ProviderStateRecord| {
                for bytes in [
                    r.namespace.as_bytes(),
                    r.key.as_bytes(),
                    r.payload.as_slice(),
                ] {
                    hash.update((bytes.len() as u64).to_be_bytes());
                    hash.update(bytes);
                }
                hash.update(r.version.to_be_bytes());
            };
            row(self.preserved.claim.inserted_row());
            row(&self.preserved.closing);
            for r in &self.preserved.core_records {
                row(r);
            }
            for d in &self.preserved.provider_dependencies {
                match d {
                    crate::TerminalRowDependency::Exact(r) => row(r),
                    crate::TerminalRowDependency::Absent { namespace, key } => {
                        row(&ProviderStateRecord {
                            namespace: namespace.clone(),
                            key: key.clone(),
                            version: 0,
                            payload: Vec::new(),
                        })
                    }
                }
            }
        }
        hash.update(self.preserved.provider_epoch.to_be_bytes());
        hash.update(self.preserved.observed_tick.to_be_bytes());
        Ok(hash.finalize().into())
    }
}
