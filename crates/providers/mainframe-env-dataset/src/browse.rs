//! STARTBR end-of-data-set positioning for a full-length all-`X'FF'` key.
//!
//! IBM topic `SSJL4D_6.x/reference-applications/commands-api/dfhp4_startbr.html`
//! (RIDFLD option): a full record id of
//! all `X'FF'` bytes positions a VSAM browse at the end of the data set,
//! ready for READPREV. `#191` (introduced by `0df4cdb`): STARTBR's bounds
//! check rejected that key with NOTFND instead of registering the cursor.

use crate::service::{State, condition, entry};
use mainframe_env_host_api::{DatasetName, HostProblem, KeyRelation};

impl State {
    /// Called once a STARTBR key is past every record. Fails NOTFND, except
    /// a full-length all-`X'FF'` key under GTEQ on a non-empty data set,
    /// which is left to register the browse at the end instead. A shorter
    /// GENERIC all-`X'FF'` key, any key on an empty data set, or any other
    /// relation still fails.
    pub(crate) fn require_eof_browse(
        &self,
        dataset: &DatasetName,
        key: &[u8],
        relation: KeyRelation,
        has_records: bool,
    ) -> Result<(), HostProblem> {
        if relation == KeyRelation::GreaterOrEqual && has_records && !key.is_empty() {
            let key_length = match self.alternate_indexes.get(dataset.as_str()) {
                Some(index) => Some(index.key_length),
                None => entry(self, dataset)?.attributes.key_length,
            };
            if key_length.is_some_and(|length| length as usize == key.len())
                && key.iter().all(|byte| *byte == 0xFF)
            {
                return Ok(());
            }
        }
        Err(condition("NOTFND", 13))
    }
}
