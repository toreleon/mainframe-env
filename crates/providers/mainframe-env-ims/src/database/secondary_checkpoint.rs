//! Retained occurrence witnesses belong to the existing engine image.
use super::*;
use crate::recovery::{SavedSecondaryOccurrence, SavedSecondaryPosition};

impl DatabaseEngineImage {
    /// Undo restores data, never a pointer identity invalidated by later work.
    /// Data-only replacements retain a witness present on both actual images.
    pub(crate) fn reconcile_secondary_backout(&mut self, current: &Self) {
        let live = current
            .records
            .iter()
            .map(|record| (record.id, record.secondary_checkpoint_identity))
            .collect::<BTreeMap<_, _>>();
        for record in &mut self.records {
            if record.secondary_checkpoint_identity.is_some()
                && live.get(&record.id).copied().flatten() != record.secondary_checkpoint_identity
            {
                record.secondary_checkpoint_identity = None;
            }
        }
    }
}

impl RecordId {
    pub(crate) fn is_nonzero(self) -> bool {
        self.0 != 0
    }
}

impl DatabaseEngine {
    pub(super) fn validate_secondary_checkpoint_identities(&self) -> Result<(), EngineProblem> {
        let mut seen = BTreeSet::new();
        for record in self.records.values() {
            if let Some(identity) = record.secondary_checkpoint_identity
                && (identity == [0; 32]
                    || !seen.insert(identity)
                    || self.definition.organization == DatabaseOrganization::Gsam)
            {
                return Err(EngineProblem::InvalidData);
            }
        }
        Ok(())
    }

    fn checkpoint_occurrence(
        &mut self,
        id: RecordId,
        seed: [u8; 32],
    ) -> Result<(SavedSecondaryOccurrence, bool), EngineProblem> {
        let record = self.records.get(&id).ok_or(EngineProblem::InvalidData)?;
        let changed = record.secondary_checkpoint_identity.is_none();
        if changed {
            let mut hash = Sha256::new();
            hash.update(b"mainframe-env.ims-secondary-checkpoint-occurrence@1\0");
            hash.update(seed);
            hash.update(self.state_digest());
            hash.update(id.0.to_le_bytes());
            let identity: [u8; 32] = hash.finalize().into();
            if identity == [0; 32]
                || self
                    .records
                    .values()
                    .any(|r| r.secondary_checkpoint_identity == Some(identity))
            {
                return Err(EngineProblem::InvalidData);
            }
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or(EngineProblem::LimitExceeded)?;
            self.records
                .get_mut(&id)
                .ok_or(EngineProblem::InvalidData)?
                .secondary_checkpoint_identity = Some(identity);
        }
        let record = &self.records[&id];
        Ok((
            SavedSecondaryOccurrence {
                id,
                identity: record
                    .secondary_checkpoint_identity
                    .ok_or(EngineProblem::InvalidData)?,
                version: record.version,
            },
            changed,
        ))
    }

    pub(crate) fn save_secondary_position(
        &mut self,
        name: &str,
        position: &PcbPosition,
        metadata_digest: [u8; 32],
        seed: [u8; 32],
    ) -> Result<(SavedSecondaryPosition, bool), EngineProblem> {
        self.validate_secondary_navigation(name)?;
        self.validate_secondary_position(position)?;
        let source = position
            .secondary
            .as_ref()
            .filter(|p| p.index == name)
            .ok_or(EngineProblem::InvalidData)?
            .source;
        let current = position.current.ok_or(EngineProblem::InvalidData)?;
        let target = self.index_target(name, source)?;
        let source_path = self.checkpoint_key_path(source)?;
        let current_path = self.checkpoint_key_path(current)?;
        let search_key = self.selected_secondary_key(name, position)?.to_vec();
        let (source, a) = self.checkpoint_occurrence(source, seed)?;
        let (target, b) = self.checkpoint_occurrence(target, seed)?;
        let (current, c) = self.checkpoint_occurrence(current, seed)?;
        Ok((
            SavedSecondaryPosition {
                index: name.into(),
                metadata_digest,
                search_key,
                source,
                target,
                current,
                source_path,
                current_path,
            },
            a || b || c,
        ))
    }

    fn checkpoint_key_path(&self, id: RecordId) -> Result<Vec<(String, Vec<u8>)>, EngineProblem> {
        self.record_path(id)?
            .into_iter()
            .map(|id| {
                let record = &self.records[&id];
                let segment = self.segment(&record.segment)?;
                let key = self
                    .primary_key(segment, &record.data)?
                    .ok_or(EngineProblem::Unsupported)?;
                Ok((record.segment.clone(), key))
            })
            .collect()
    }

    fn live_occurrence(&self, saved: &SavedSecondaryOccurrence) -> bool {
        self.records
            .get(&saved.id)
            .is_some_and(|r| r.secondary_checkpoint_identity == Some(saved.identity))
    }

    pub(crate) fn restore_secondary_position(
        &self,
        saved: &SavedSecondaryPosition,
    ) -> Result<(PcbPosition, bool), EngineProblem> {
        self.validate_secondary_navigation(&saved.index)?;
        let mut position = PcbPosition::default();
        let live = [&saved.source, &saved.target, &saved.current]
            .iter()
            .all(|p| self.live_occurrence(p))
            && self.index_target(&saved.index, saved.source.id).ok() == Some(saved.target.id)
            && self.checkpoint_key_path(saved.source.id).ok().as_ref() == Some(&saved.source_path)
            && self.checkpoint_key_path(saved.current.id).ok().as_ref()
                == Some(&saved.current_path);
        if live {
            let request = ReadRequest {
                kind: ReadKind::Unique,
                target: None,
                path: vec![],
                hold: false,
            };
            match self.read_secondary_occurrence(
                &saved.index,
                &mut position,
                &request,
                Some(saved.source.id),
                |id, key| id == saved.current.id && key == saved.search_key,
            ) {
                Ok(_) => return Ok((position, true)),
                Err(EngineProblem::NotFound) => {}
                Err(problem) => return Err(problem),
            }
        }
        // Real selected GN supplies the predecessor. No synthetic record or
        // primary-order cursor is introduced for a removed/replaced pointer.
        if !keyed_root_order(self.definition.organization) {
            return Err(EngineProblem::Unsupported);
        }
        let boundary = (
            saved.search_key.clone(),
            self.checkpoint_order(&saved.source_path)?,
            self.checkpoint_order(&saved.current_path)?,
        );
        let mut scan = PcbPosition::default();
        let request = ReadRequest {
            kind: ReadKind::Next,
            target: None,
            path: vec![],
            hold: false,
        };
        loop {
            let record =
                match self.read_secondary_visible(&saved.index, &mut scan, &request, |_| true) {
                    Ok(record) => record,
                    Err(EngineProblem::EndOfDatabase) => break,
                    Err(problem) => return Err(problem),
                };
            let source = scan
                .secondary
                .as_ref()
                .ok_or(EngineProblem::InvalidData)?
                .source;
            let order = (
                self.selected_secondary_key(&saved.index, &scan)?.to_vec(),
                self.checkpoint_order(&self.checkpoint_key_path(source)?)?,
                self.checkpoint_order(&self.checkpoint_key_path(record.id)?)?,
            );
            if order > boundary {
                break;
            }
            position = scan.clone();
        }
        position.held = None;
        position.parentage = None;
        Ok((position, false))
    }

    fn checkpoint_order(
        &self,
        path: &[(String, Vec<u8>)],
    ) -> Result<Vec<(usize, Vec<u8>)>, EngineProblem> {
        path.iter()
            .map(|(name, key)| {
                let ordinal = self
                    .definition
                    .segments
                    .iter()
                    .position(|s| &s.name == name)
                    .ok_or(EngineProblem::InvalidData)?;
                Ok((ordinal, key.clone()))
            })
            .collect()
    }
}
