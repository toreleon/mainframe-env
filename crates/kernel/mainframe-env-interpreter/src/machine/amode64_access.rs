use super::*;

impl ReferenceMachine {
    /// Read a checked virtual AMODE(64) allocation without exposing a native pointer.
    pub fn read_storage64(
        &self,
        address: u64,
        offset: usize,
        length: usize,
    ) -> Result<Vec<u8>, Storage64Problem> {
        let key = self.storage64_caller_key()?;
        self.storage64.read(
            address,
            offset,
            length,
            self.invocation.run_unit_id.as_str(),
            key,
        )
    }

    /// Write a checked virtual AMODE(64) allocation and include it in checkpoints.
    pub fn write_storage64(
        &mut self,
        address: u64,
        offset: usize,
        value: &[u8],
    ) -> Result<(), Storage64Problem> {
        let key = self.storage64_caller_key()?;
        self.storage64.write(
            address,
            offset,
            value,
            self.invocation.run_unit_id.as_str(),
            key,
        )
    }

    fn storage64_caller_key(&self) -> Result<Storage64Key, Storage64Problem> {
        let marker = self
            .invocation
            .bindings
            .get("cics.amode64.caller")
            .ok_or(Storage64Problem::InvalidAbi)?;
        if marker.schema() != "mainframe-env.cics.amode64-caller@1"
            || marker.bytes() != b"non-le-amode64"
        {
            return Err(Storage64Problem::InvalidAbi);
        }
        let key = self
            .invocation
            .bindings
            .get("cics.amode64.taskdatakey")
            .ok_or(Storage64Problem::InvalidAbi)?;
        if key.schema() != "mainframe-env.cics.taskdatakey@1" {
            return Err(Storage64Problem::InvalidAbi);
        }
        match key.bytes() {
            b"USER" => Ok(Storage64Key::User),
            b"CICS" => Ok(Storage64Key::Cics),
            _ => Err(Storage64Problem::InvalidAbi),
        }
    }
}
