//! Host logical identity and positioning over the existing sequential engine.
use super::*;
use mainframe_env_host_api::ImsGsamAddress;

impl DatabaseEngine {
    pub(crate) fn validate_gsam_addresses(&self) -> Result<(), EngineProblem> {
        let mut addresses = BTreeSet::new();
        for record in self.records.values() {
            if let Some(token) = record.gsam_address
                && (self.definition.organization != DatabaseOrganization::Gsam
                    || token == [0; 32]
                    || !addresses.insert(token))
            {
                return Err(EngineProblem::InvalidData);
            }
        }
        Ok(())
    }

    /// GN-issued identity is stable across append and reopen. Materializing an
    /// old record is an atomic host metadata mutation, not a physical IBM RSA.
    pub(crate) fn issue_gsam_address(
        &mut self,
        id: RecordId,
        seed: [u8; 32],
    ) -> Result<(ImsGsamAddress, bool), EngineProblem> {
        if self.definition.organization != DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let record = self.records.get(&id).ok_or(EngineProblem::NotFound)?;
        let (token, changed) = match record.gsam_address {
            Some(token) => (token, false),
            None => {
                let mut hash = Sha256::new();
                hash.update(b"mainframe-env.ims-gsam-logical-address@1\0");
                hash.update(seed);
                hash.update(self.state_digest());
                hash.update(id.0.to_le_bytes());
                let token: [u8; 32] = hash.finalize().into();
                if token == [0; 32] || self.records.values().any(|r| r.gsam_address == Some(token))
                {
                    return Err(EngineProblem::InvalidData);
                }
                self.revision = self
                    .revision
                    .checked_add(1)
                    .ok_or(EngineProblem::LimitExceeded)?;
                self.records
                    .get_mut(&id)
                    .ok_or(EngineProblem::NotFound)?
                    .gsam_address = Some(token);
                (token, true)
            }
        };
        Ok((
            ImsGsamAddress {
                database: self.definition.name.clone(),
                token,
            },
            changed,
        ))
    }

    pub(crate) fn read_gsam_address(
        &self,
        position: &mut PcbPosition,
        address: &ImsGsamAddress,
    ) -> Result<RecordView, EngineProblem> {
        if self.definition.organization != DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        if address.database != self.definition.name {
            return Err(EngineProblem::InvalidRequest);
        }
        let id = self
            .records
            .values()
            .find(|r| r.gsam_address == Some(address.token))
            .map(|r| r.id)
            .ok_or(EngineProblem::InvalidRequest)?;
        *position = PcbPosition {
            current: Some(id),
            ..PcbPosition::default()
        };
        Ok(self.view(id))
    }

    pub(crate) fn read_gsam_next(
        &self,
        position: &mut PcbPosition,
    ) -> Result<RecordView, EngineProblem> {
        if self.definition.organization != DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let result = self.read(
            position,
            &ReadRequest {
                kind: ReadKind::Next,
                target: None,
                path: vec![],
                hold: false,
            },
        );
        position.parentage = None;
        result
    }
}
