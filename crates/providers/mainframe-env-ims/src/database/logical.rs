use super::*;

impl DatabaseEngine {
    pub(crate) fn logical_links(&self) -> &BTreeSet<LogicalLink> {
        &self.logical_links
    }

    pub(crate) fn add_logical_link(&mut self, link: LogicalLink) -> Result<(), EngineProblem> {
        if !self.records.contains_key(&link.child)
            || link.parent_database.is_empty()
            || link.parent_segment.is_empty()
            || self.logical_links.len()
                >= self
                    .limits
                    .max_records
                    .saturating_mul(self.limits.max_segments)
            || self.logical_links.iter().any(|existing| {
                existing.child == link.child
                    && existing.parent_database == link.parent_database
                    && existing.parent_segment == link.parent_segment
            })
        {
            return Err(EngineProblem::InvalidData);
        }
        self.logical_links.insert(link);
        Ok(())
    }

    pub(super) fn validate_logical_links(&self) -> Result<(), EngineProblem> {
        if self.logical_links.len()
            > self
                .limits
                .max_records
                .saturating_mul(self.limits.max_segments)
        {
            return Err(EngineProblem::LimitExceeded);
        }
        let mut unique = BTreeSet::new();
        for link in &self.logical_links {
            if link.parent_database.is_empty()
                || link.parent_segment.is_empty()
                || !self.records.contains_key(&link.child)
                || !unique.insert((link.child, &link.parent_database, &link.parent_segment))
            {
                return Err(EngineProblem::InvalidData);
            }
        }
        Ok(())
    }

    pub(crate) fn subtree_ids(&self, id: RecordId) -> Result<BTreeSet<RecordId>, EngineProblem> {
        let mut found = BTreeSet::new();
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            let record = self.records.get(&current).ok_or(EngineProblem::NotFound)?;
            if found.insert(current) {
                pending.extend(record.children.iter().copied());
            }
            if found.len() > self.limits.max_records {
                return Err(EngineProblem::LimitExceeded);
            }
        }
        Ok(found)
    }

    pub(crate) fn delete_by_id(&mut self, id: RecordId) -> Result<usize, EngineProblem> {
        let record = self.records.get(&id).ok_or(EngineProblem::NotFound)?;
        let mut position = PcbPosition {
            current: Some(id),
            parentage: Some(id),
            held: Some(HeldRecord {
                id,
                version: record.version,
            }),
            after_end: false,
            secondary: None,
        };
        self.delete(&mut position)
    }
}
