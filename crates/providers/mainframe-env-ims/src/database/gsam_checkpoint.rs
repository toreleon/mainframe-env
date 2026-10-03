//! Bounded logical file position operations. Shared store publication stays in
//! the application recovery bridge; removed addresses are never reconstructed.
use super::*;
use crate::recovery::SavedGsamPosition;

impl DatabaseEngine {
    pub(crate) fn gsam_identity_only_since(&self, prior: &DatabaseEngineImage) -> bool {
        if self.definition.organization != DatabaseOrganization::Gsam {
            return false;
        }
        let mut current = self.image();
        let mut prior = prior.clone();
        for image in [&mut current, &mut prior] {
            image.revision = 0;
            for record in &mut image.records {
                record.gsam_address = None;
            }
        }
        current == prior
    }
    pub(crate) fn save_gsam_position(
        &mut self,
        position: &PcbPosition,
        output: bool,
        seed: [u8; 32],
    ) -> Result<(SavedGsamPosition, bool), EngineProblem> {
        if self.definition.organization != DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        if output {
            let mut changed = false;
            for id in self.roots.clone() {
                changed |= self.issue_gsam_address(id, seed)?.1;
            }
            return Ok((
                SavedGsamPosition::Output {
                    records: self.record_count(),
                    prefix_digest: self.gsam_prefix_digest(self.record_count())?,
                },
                changed,
            ));
        }
        if let Some(id) = position.current {
            let (address, changed) = self.issue_gsam_address(id, seed)?;
            Ok((SavedGsamPosition::Record(address), changed))
        } else if position.after_end {
            Ok((SavedGsamPosition::Eof, false))
        } else {
            Ok((SavedGsamPosition::Beginning, false))
        }
    }

    pub(crate) fn restore_gsam_position(
        &mut self,
        saved: &SavedGsamPosition,
    ) -> Result<(PcbPosition, bool), EngineProblem> {
        if self.definition.organization != DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let mut position = PcbPosition::default();
        match saved {
            SavedGsamPosition::Beginning => {}
            SavedGsamPosition::Eof => position.after_end = true,
            SavedGsamPosition::Record(address) => {
                self.read_gsam_address(&mut position, address)?;
            }
            SavedGsamPosition::Output {
                records,
                prefix_digest,
            } => {
                // Validate live records and identity before removing later output.
                // Empty/missing/replaced files cannot revive a stale saved address.
                if self.gsam_prefix_digest(*records)? != *prefix_digest {
                    return Err(EngineProblem::InvalidData);
                }
                if self.record_count() > *records {
                    let kept: BTreeSet<_> = self.roots.iter().take(*records).copied().collect();
                    self.records.retain(|id, _| kept.contains(id));
                    self.roots.truncate(*records);
                    self.revision = self
                        .revision
                        .checked_add(1)
                        .ok_or(EngineProblem::LimitExceeded)?;
                    // Keep next_id monotonic, and keep every surviving token.
                    return Ok((position, true));
                }
            }
        }
        Ok((position, false))
    }

    fn gsam_prefix_digest(&self, records: usize) -> Result<[u8; 32], EngineProblem> {
        if records > self.record_count() {
            return Err(EngineProblem::InvalidData);
        }
        let prefix: Vec<_> = self
            .roots
            .iter()
            .take(records)
            .map(|id| &self.records[id])
            .collect();
        let bytes = serde_json::to_vec(&(&self.definition, prefix))
            .map_err(|_| EngineProblem::InvalidData)?;
        let mut hash = Sha256::new();
        hash.update(b"mainframe-env.ims-gsam-checkpoint-prefix@1\0");
        hash.update(bytes);
        Ok(hash.finalize().into())
    }
}
