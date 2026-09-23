use super::*;

impl ReferenceMachine {
    pub(super) fn new_storage64_arena(&self) -> Storage64Arena {
        Storage64Arena::new(Storage64Limits {
            max_allocations: self.invocation.limits.max_frames,
            max_bytes: self.invocation.limits.max_storage_bytes,
        })
    }

    pub(super) fn install_storage64_snapshot(
        &mut self,
        arena: Storage64Arena,
        area_bindings: BTreeMap<String, u64>,
    ) {
        self.storage64 = arena;
        self.storage64_area_bindings = area_bindings;
    }

    pub(super) fn restore_storage64_snapshot(
        &self,
        restored_storage64: &mut Storage64Arena,
        snapshot: &MachineSnapshot,
    ) -> Result<(), MachineProblem> {
        if snapshot.schema_version >= 11 {
            restored_storage64
                .restore(snapshot.storage64.clone())
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        }
        Ok(())
    }

    pub(super) fn restore_area64_bindings(
        &self,
        snapshot: &MachineSnapshot,
        restored_storage64: &Storage64Arena,
    ) -> Result<BTreeMap<String, u64>, MachineProblem> {
        let area_bindings = if snapshot.schema_version >= 12 {
            snapshot.storage64_area_bindings.clone()
        } else {
            BTreeMap::new()
        };
        if area_bindings.len() > self.invocation.limits.max_frames as usize {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        if !area_bindings.is_empty() {
            let caller_key = self
                .storage64_caller_key()
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            if area_bindings.iter().any(|(name, address)| {
                self.layouts.get(name).is_none_or(|layout| {
                    layout.length == 0
                        || matches!(
                            layout.category,
                            LayoutCategory::Pointer | LayoutCategory::Pointer32
                        )
                }) || restored_storage64
                    .can_release(*address, self.invocation.run_unit_id.as_str(), caller_key)
                    .is_err()
            }) {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        Ok(area_bindings)
    }

    /// Bind an assembler-style DATA area to the first byte of a live 64-bit
    /// allocation. The area name, rather than its stored bytes, identifies
    /// the allocation for FREEMAIN64 DATA.
    pub fn bind_storage64_area(
        &mut self,
        name: &str,
        address: u64,
    ) -> Result<(), Storage64Problem> {
        let caller_key = self.storage64_caller_key()?;
        let name = normalize(name);
        let layout = self
            .layouts
            .get(&name)
            .ok_or(Storage64Problem::InvalidPointer)?;
        if layout.length == 0
            || matches!(
                layout.category,
                LayoutCategory::Pointer | LayoutCategory::Pointer32
            )
        {
            return Err(Storage64Problem::InvalidPointer);
        }
        self.storage64
            .can_release(address, self.invocation.run_unit_id.as_str(), caller_key)?;
        self.storage64_area_bindings.insert(name, address);
        Ok(())
    }

    pub(super) fn release_storage64_task(&mut self) {
        self.storage64
            .end_task(self.invocation.run_unit_id.as_str());
        self.storage64_area_bindings
            .retain(|_, address| self.storage64.contains(*address));
    }

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

    pub(super) fn storage64_caller_key(&self) -> Result<Storage64Key, Storage64Problem> {
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
