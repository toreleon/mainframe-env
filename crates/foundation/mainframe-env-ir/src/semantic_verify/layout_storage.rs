//! Storage-view consistency for executable COBOL layout metadata.

use super::LayoutIndex;
use crate::cobol_layout::CobolLayoutAbi;
use crate::{OperationIdentity, StorageId, StorageRegion};

#[derive(Clone, Copy)]
struct DeclaredStorageView {
    base: StorageId,
    offset: u64,
    length: u64,
}

impl<'a> LayoutIndex<'a> {
    pub(super) fn runtime_storage(
        &self,
        layout: &CobolLayoutAbi<'_>,
    ) -> Result<&'a StorageRegion, &'static str> {
        let expected = if layout.dynamic {
            layout.dynamic_limit
        } else {
            layout.length
        };
        match self.storage.get(&layout.name.to_ascii_uppercase()) {
            Some(regions) if matches!(regions.as_slice(), [region] if region.size == expected) => {
                Ok(regions[0])
            }
            _ => Err("COBOL layout has no unique exact-extent storage binding"),
        }
    }

    fn storage_view(
        &self,
        storage: &'a StorageRegion,
    ) -> Result<DeclaredStorageView, &'static str> {
        let mut current = storage;
        let mut offset = 0u64;
        for _ in 0..=self.storage_by_id.len() {
            let Some(alias) = &current.alias_of else {
                return Ok(DeclaredStorageView {
                    base: current.id,
                    offset,
                    length: storage.size,
                });
            };
            offset = offset
                .checked_add(alias.offset)
                .ok_or("COBOL storage alias offset overflows")?;
            current = self
                .storage_by_id
                .get(alias.storage.get() as usize)
                .ok_or("COBOL storage alias target is outside the module")?;
        }
        Err("COBOL storage alias chain is cyclic")
    }

    pub(super) fn validate_storage_topology(
        &self,
        identity: &OperationIdentity,
        layout: &CobolLayoutAbi<'_>,
    ) -> Result<(), &'static str> {
        let extent = if layout.dynamic {
            layout.dynamic_limit
        } else {
            layout.length
        };
        if extent == 0 {
            return if self.storage.contains_key(&layout.name.to_ascii_uppercase()) {
                Err("zero-extent COBOL layout unexpectedly declares storage")
            } else {
                Ok(())
            };
        }
        let view = self.storage_view(self.runtime_storage(layout)?)?;
        if !layout.dynamic && !layout.unbounded && !layout.parent.is_empty() {
            let Some([parent]) = self.get(identity, layout.parent) else {
                return Err("COBOL layout storage parent is missing or ambiguous");
            };
            let parent = self.abi(parent)?;
            if parent.dynamic {
                return Err("static COBOL layout cannot use dynamic parent storage");
            }
            if parent.length > 0 {
                let parent_view = self.storage_view(self.runtime_storage(&parent)?)?;
                let relative = layout
                    .offset
                    .checked_sub(parent.offset)
                    .ok_or("COBOL child layout starts before its parent")?;
                let expected_offset = parent_view
                    .offset
                    .checked_add(relative)
                    .ok_or("COBOL child storage offset overflows")?;
                let child_end = relative
                    .checked_add(view.length)
                    .ok_or("COBOL child storage extent overflows")?;
                if view.base != parent_view.base
                    || view.offset != expected_offset
                    || child_end > parent_view.length
                {
                    return Err("COBOL child storage view disagrees with its parent layout");
                }
            }
        }
        if !layout.alias_of.is_empty() && layout.category != "condition" {
            let Some([target]) = self.get(identity, layout.alias_of) else {
                return Err("COBOL storage alias target is missing or ambiguous");
            };
            let target = self.abi(target)?;
            let target_view = self.storage_view(self.runtime_storage(&target)?)?;
            if view.base != target_view.base || view.offset != target_view.offset {
                return Err("COBOL alias metadata and declared storage view disagree");
            }
        }
        Ok(())
    }
}
