//! Floating storage profile selection and append-only checkpoint migration.
use super::*;

fn is_floating(layout: &LayoutMetadata) -> bool {
    matches!(
        layout.category,
        LayoutCategory::FloatShort | LayoutCategory::FloatLong
    )
}

impl ReferenceMachine {
    /// Reproduce an existing receipt's checkpoint representation for attestation.
    /// Schema 12 is available only for IEEE layouts; HEX state cannot be relabeled.
    /// Normal checkpoint emission continues to select the current schema.
    pub fn checkpoint_for_schema(&self, schema: &str) -> Option<BoundedPayload> {
        let current = self.checkpoint()?;
        if current.schema() == schema {
            return Some(current);
        }
        if schema != "mainframe-env.reference-machine-checkpoint@12"
            || current.schema() != "mainframe-env.reference-machine-checkpoint@13"
        {
            return None;
        }
        let mut snapshot = self.snapshot();
        if snapshot
            .float_profiles
            .values()
            .any(|profile| profile != mainframe_env_ir::COBOL_FLOAT_IEEE)
        {
            return None;
        }
        snapshot.schema_version = 12;
        snapshot.float_profiles.clear();
        let bytes = snapshot_codec::encode_snapshot(&snapshot)?;
        BoundedPayload::new(
            schema,
            bytes,
            InvocationLimits {
                max_payload_bytes: current.bytes().len(),
                ..InvocationLimits::default()
            },
        )
        .ok()
    }

    pub(super) fn snapshot_version(&self) -> u32 {
        if self.mqi.is_some() {
            0
        } else if self.layouts.values().any(is_floating) {
            13
        } else {
            12
        }
    }

    pub(super) fn checkpoint_schema(&self) -> &'static str {
        if self.layouts.values().any(is_floating) {
            "mainframe-env.reference-machine-checkpoint@13"
        } else {
            "mainframe-env.reference-machine-checkpoint@12"
        }
    }

    pub(super) fn snapshot_float_profiles(&self) -> BTreeMap<String, String> {
        self.layouts
            .iter()
            .filter(|(_, layout)| is_floating(layout))
            .map(|(name, layout)| {
                (
                    name.clone(),
                    match layout.float_profile {
                        float_codec::FloatProfile::Hex => mainframe_env_ir::COBOL_FLOAT_HEX,
                        float_codec::FloatProfile::Ieee => mainframe_env_ir::COBOL_FLOAT_IEEE,
                    }
                    .into(),
                )
            })
            .collect()
    }

    pub(super) fn validate_profiles(
        &self,
        schema_version: u32,
        profiles: &BTreeMap<String, String>,
    ) -> Result<BTreeSet<String>, MachineProblem> {
        let names = self
            .layouts
            .iter()
            .filter(|(_, layout)| is_floating(layout))
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        if schema_version >= 13
            && (profiles.keys().cloned().collect::<BTreeSet<_>>() != names
                || profiles.values().any(|profile| {
                    !matches!(
                        profile.as_str(),
                        mainframe_env_ir::COBOL_FLOAT_HEX | mainframe_env_ir::COBOL_FLOAT_IEEE
                    )
                }))
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        Ok(names)
    }

    pub(super) fn restore_checkpoint_float_profiles(
        &mut self,
        schema_version: u32,
        profiles: &BTreeMap<String, String>,
        names: BTreeSet<String>,
    ) {
        for name in names {
            self.layouts
                .get_mut(&name)
                .expect("validated floating name")
                .float_profile =
                if schema_version >= 13 && profiles[&name] == mainframe_env_ir::COBOL_FLOAT_HEX {
                    float_codec::FloatProfile::Hex
                } else {
                    // Legacy checkpoints contain IEEE bytes, even with a newly compiled module.
                    float_codec::FloatProfile::Ieee
                };
        }
    }
}

pub(super) fn snapshot_header(schema_version: u32) -> &'static [u8; 8] {
    if schema_version == 13 {
        b"MECP0013"
    } else {
        b"MECP0012"
    }
}

pub(super) fn decode(
    input: &mut SnapshotInput<'_>,
    header_version: u32,
    max_storage: usize,
) -> Result<BTreeMap<String, String>, MachineProblem> {
    let mut profiles = BTreeMap::new();
    if header_version >= 13 {
        let count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if count > max_storage {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..count {
            let name = snapshot_string(input, 4096)?;
            let profile = match input.take(1)?.first() {
                Some(0) => mainframe_env_ir::COBOL_FLOAT_IEEE,
                Some(1) => mainframe_env_ir::COBOL_FLOAT_HEX,
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            if profiles.insert(name, profile.into()).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
    }
    Ok(profiles)
}
