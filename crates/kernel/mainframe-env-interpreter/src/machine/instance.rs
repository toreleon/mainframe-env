//! Last-used program state, separate from a suspended machine checkpoint.
use super::*;

// Explicit protocol tuple: version, persistent storage, ALTER targets, persistent
// indexes, last file status, dataset cursors, deterministic random state.
type RetainedState = (
    u32,
    BTreeMap<String, Vec<u8>>,
    BTreeMap<String, String>,
    BTreeMap<String, (i128, u32)>,
    String,
    BTreeMap<String, String>,
    Option<u64>,
);

impl ReferenceMachine {
    /// Only this explicitly described lifecycle is safe to retain. Older MIR
    /// lacks INITIAL metadata and must be recompiled, never guessed ordinary.
    pub fn installed_call_is_initial(&self) -> Result<bool, MachineProblem> {
        let initial = match self
            .operations
            .iter()
            .find(|op| op.identity.name() == "config")
            .and_then(|op| optional_text_attribute(op, "program_lifecycle"))
        {
            Some("retained@1") => false,
            Some("initial@1") => true,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        if self.layouts.values().any(|layout| layout.dynamic || layout.unbounded || matches!(layout.category,
            LayoutCategory::Pointer | LayoutCategory::Pointer32 | LayoutCategory::ProcedurePointer |
            LayoutCategory::FunctionPointer | LayoutCategory::ObjectReference))
            || self.operations.iter().any(|op| matches!(op.identity.name(),
                "entry" | "allocate" | "free" | "invoke"))
            // INITIAL implies implicit file closure; do not fake that protocol.
            || initial && !self.files.is_empty()
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        Ok(initial)
    }

    fn retained_roots(&self) -> Result<BTreeMap<String, StorageId>, MachineProblem> {
        let persistent: BTreeSet<_> = self
            .operations
            .iter()
            .filter(|op| {
                op.identity.name() == "define"
                    && matches!(
                        optional_text_attribute(op, "section"),
                        Some("working" | "file")
                    )
            })
            .filter_map(|op| optional_text_attribute(op, "name"))
            .collect();
        self.operations
            .iter()
            .filter(|op| op.identity.name() == "init")
            .filter_map(|op| {
                optional_text_attribute(op, "name")
                    .filter(|name| persistent.contains(name))
                    .map(|name| (name, op))
            })
            .map(|(name, op)| {
                Ok((
                    name.into(),
                    op.storage
                        .first()
                        .ok_or(MachineProblem::InvalidOperation)?
                        .storage,
                ))
            })
            .collect()
    }

    fn retained_indexes(&self) -> BTreeSet<String> {
        self.operations
            .iter()
            .filter(|op| {
                op.identity.name() == "define"
                    && matches!(
                        optional_text_attribute(op, "section"),
                        Some("working" | "file")
                    )
            })
            .filter_map(|op| optional_text_attribute(op, "indexes"))
            .flat_map(|indexes| {
                indexes
                    .split('\u{1f}')
                    .filter(|name| !name.is_empty())
                    .map(normalize)
            })
            .collect()
    }

    /// Called only after normal GOBACK/EXIT PROGRAM. It does not retain linkage,
    /// LOCAL-STORAGE, PC, pending effects, output, call stacks or condition flags.
    pub fn retained_program_state(&self) -> Result<Vec<u8>, MachineProblem> {
        self.installed_call_is_initial()?;
        if self
            .operations
            .get(self.pc)
            .is_some_and(|op| op.identity.name() == "stop_run")
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        if !self.sql_cursors.is_empty()
            || !self.sort_workspaces.is_empty()
            || self.active_sort_procedure.is_some()
            || self.sort_io.is_some()
            || !self.linkage_addresses.is_empty()
            || self.bases.len() != self.static_base_count
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        let roots: BTreeMap<String, Vec<u8>> = self
            .retained_roots()?
            .into_iter()
            .map(|(name, id)| {
                let view = self
                    .views_by_id
                    .get(&id)
                    .ok_or(MachineProblem::UnknownStorage)?;
                Ok((
                    name,
                    self.bases[view.base][view.offset..view.offset + view.length].to_vec(),
                ))
            })
            .collect::<Result<_, MachineProblem>>()?;
        let indexes: BTreeMap<String, (i128, u32)> = self
            .retained_indexes()
            .into_iter()
            .map(|name| match self.implicit.get(&name) {
                Some(CobolValue::Decimal(value)) => Ok((name, (value.coefficient, value.scale))),
                _ => Err(MachineProblem::InvalidOperation),
            })
            .collect::<Result<_, _>>()?;
        serde_json::to_vec(&(
            1_u32,
            roots,
            self.altered
                .iter()
                .filter(|(name, _)| self.labels.contains_key(*name))
                .collect::<BTreeMap<_, _>>(),
            indexes,
            &self.last_file_status,
            &self.dataset_cursors,
            self.random_state.get(),
        ))
        .map_err(|_| MachineProblem::ResourceExhausted)
    }

    /// The ready instance is artifact-pinned by its owner. Validate the complete
    /// payload before writing any state. Initializers use these saved values on
    /// normal entry; invocation-local initializers retain the fresh call inputs.
    pub fn install_retained_program_state(&mut self, payload: &[u8]) -> Result<(), MachineProblem> {
        if self.installed_call_is_initial()? {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        let limit = (self.invocation.limits.max_storage_bytes as usize)
            .saturating_mul(5)
            .saturating_add(64 * 1024);
        if payload.len() > limit {
            return Err(MachineProblem::ResourceExhausted);
        }
        let (version, roots, altered, indexes, status, cursors, random): RetainedState =
            serde_json::from_slice(payload).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        let expected = self.retained_roots()?;
        if version != 1
            || roots.keys().ne(expected.keys())
            || indexes.keys().cloned().collect::<BTreeSet<_>>() != self.retained_indexes()
            || indexes.values().any(|(_, scale)| *scale != 0)
            || status.len() != 2
            || altered
                .iter()
                .any(|(from, to)| !self.labels.contains_key(from) || !self.labels.contains_key(to))
            || roots.iter().any(|(name, bytes)| {
                self.views_by_id
                    .get(&expected[name])
                    .is_none_or(|view| view.length != bytes.len())
            })
            || cursors.len() > self.invocation.limits.max_frames as usize
            || cursors.iter().any(|(dataset, cursor)| {
                DatasetName::new(dataset, 128).is_err() || cursor.is_empty() || cursor.len() > 128
            })
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for (name, bytes) in roots {
            let id = expected[&name];
            self.write_storage(id, &bytes)?;
            self.entry_initials.insert(id, bytes);
        }
        self.altered = altered;
        for (name, (coefficient, scale)) in indexes {
            self.implicit
                .insert(name, CobolValue::Decimal(Decimal { coefficient, scale }));
        }
        self.last_file_status = status;
        self.dataset_cursors = cursors;
        self.random_state.set(random);
        Ok(())
    }
}
