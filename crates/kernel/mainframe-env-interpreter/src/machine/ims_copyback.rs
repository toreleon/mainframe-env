//! Fixed-view local EXEC DLI copyback; no raw PCB or CALL ABI authority.
use super::*;
use mainframe_env_host_api::ImsResult;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Slot {
    storage: StorageId,
    name: String,
    view: StorageView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Capture {
    area: Option<Slot>,
    status: Option<Slot>,
    segment: Option<String>,
}

fn extent(view: &StorageView) -> Result<std::ops::Range<usize>, MachineProblem> {
    let end = view
        .offset
        .checked_add(view.length)
        .ok_or(MachineProblem::ResourceExhausted)?;
    Ok(view.offset..end)
}

impl ReferenceMachine {
    fn ims_slot(&self, target: &str) -> Result<Slot, MachineProblem> {
        let layout = self.layout(target).ok_or(MachineProblem::UnknownStorage)?;
        if layout.dynamic || layout.unbounded || layout.linkage || layout.length == 0 {
            return Err(MachineProblem::UnsupportedForm);
        }
        let view = self.storage_view(&layout.name)?.clone();
        let mut identities = self
            .storage_names_by_id
            .iter()
            .filter(|(_, name)| *name == &layout.name);
        let (&storage, _) = identities.next().ok_or(MachineProblem::UnknownStorage)?;
        let range = extent(&view)?;
        if identities.next().is_some()
            || self.views_by_id.get(&storage) != Some(&view)
            || view.length != layout.length
            || self
                .bases
                .get(view.base)
                .and_then(|base| base.get(range))
                .is_none()
        {
            return Err(MachineProblem::DataException);
        }
        Ok(Slot {
            storage,
            name: layout.name.clone(),
            view,
        })
    }

    fn capture_ims_copyback(
        &self,
        target: Option<&str>,
        segment: Option<&str>,
    ) -> Result<Capture, MachineProblem> {
        if !matches!(self.implicit.get("DIBSTAT"), Some(CobolValue::Bytes(bytes)) if bytes.len() == 2)
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let area = target.map(|name| self.ims_slot(name)).transpose()?;
        let status = match self.layout("DIBSTAT") {
            Some(layout) => Some(self.ims_slot(&layout.name)?),
            None if self.simple_layouts.contains_key("DIBSTAT") => {
                return Err(MachineProblem::UnknownStorage);
            }
            None => None,
        };
        if status.as_ref().is_some_and(|slot| slot.view.length != 2) {
            return Err(MachineProblem::UnsupportedForm);
        }
        if let (Some(area), Some(status)) = (&area, &status)
            && area.view.base == status.view.base
            && extent(&area.view)?.start < extent(&status.view)?.end
            && extent(&status.view)?.start < extent(&area.view)?.end
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        Ok(Capture {
            area,
            status,
            segment: segment.map(str::to_owned),
        })
    }

    pub(super) fn ims_storage_effect(
        &mut self,
        request: HostRequest,
        target: Option<String>,
    ) -> Result<Step, MachineProblem> {
        let HostRequest::Ims(operands) = &request else {
            return Err(MachineProblem::InvalidOperation);
        };
        let capture = self.capture_ims_copyback(
            target.as_deref(),
            operands.segments.last().map(String::as_str),
        )?;
        self.effect(request, PendingKind::Ims { capture })
    }

    pub(super) fn finish_ims_copyback(
        &mut self,
        capture: Capture,
        mut result: ImsResult,
    ) -> Result<(), MachineProblem> {
        // The original producer offers max_segments=1. Preserve exact status
        // bytes and status-only replies; never fit a malformed returned area.
        if result.status.len() != 2 || result.segments.len() > 1 {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        if let Some(segment) = result.segments.first()
            && capture
                .segment
                .as_ref()
                .is_some_and(|name| name != &segment.name)
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let mut writes = Vec::with_capacity(2);
        let status = result.status.into_bytes();
        if let Some(slot) = &capture.status {
            writes.push((slot.view.clone(), status.clone()));
        }
        if let (Some(slot), Some(segment)) = (&capture.area, result.segments.pop()) {
            if segment.data.len() != slot.view.length {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            writes.push((slot.view.clone(), segment.data));
        }
        for slot in [&capture.area, &capture.status].into_iter().flatten() {
            if self.ims_slot(&slot.name)? != *slot {
                return Err(MachineProblem::UnexpectedHostResult);
            }
        }
        if !matches!(self.implicit.get("DIBSTAT"), Some(CobolValue::Bytes(bytes)) if bytes.len() == 2)
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let implicit = CobolValue::Bytes(status);
        let current = self
            .implicit
            .get_mut("DIBSTAT")
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        // Every allocation, fallible lookup, identity/range check and overlap
        // refusal precedes this commit. No callback occurs in the write set.
        *current = implicit;
        for (view, bytes) in writes {
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> ReferenceMachine {
        let mut machine = ReferenceMachine::from_binary(
            &super::super::tests::binary(),
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        machine.layouts.insert(
            "MSG".into(),
            LayoutMetadata {
                name: "MSG".into(),
                simple_name: "MSG".into(),
                category: LayoutCategory::Alphanumeric,
                picture: "X(5)".into(),
                digits: 0,
                scale: 0,
                native_binary: false,
                signed: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                linkage: false,
                offset: 0,
                length: 5,
                element_length: 5,
                occurs: 1,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                dynamic: false,
                dynamic_limit: 0,
                parent: None,
                alias_of: None,
                occurs_clause: false,
                condition_values: Vec::new(),
                object_class: None,
            },
        );
        machine
    }

    #[test]
    fn ims_copyback_checked_extent_refuses_wraparound() {
        assert_eq!(
            extent(&StorageView {
                base: 0,
                offset: 7,
                length: 5
            })
            .unwrap(),
            7..12
        );
        assert!(matches!(
            extent(&StorageView {
                base: 0,
                offset: usize::MAX,
                length: 2
            }),
            Err(MachineProblem::ResourceExhausted)
        ));
    }

    #[test]
    fn ims_copyback_changed_identity_view_and_status_preserve_all_storage() {
        for case in 0..5 {
            let mut machine = machine();
            let capture = machine
                .capture_ims_copyback(Some("MSG"), Some("ROOT"))
                .unwrap();
            let area = capture.area.as_ref().unwrap();
            match case {
                0 => {
                    machine.storage_names_by_id.remove(&area.storage);
                }
                1 => {
                    machine.views_by_id.get_mut(&area.storage).unwrap().length = 4;
                }
                2 => {
                    machine.views.get_mut("MSG").unwrap().offset = usize::MAX;
                }
                3 => {
                    machine
                        .implicit
                        .insert("DIBSTAT".into(), CobolValue::Bytes(vec![b'!']));
                }
                _ => {
                    machine.layouts.get_mut("MSG").unwrap().linkage = true;
                }
            }
            let before = machine.snapshot();
            let result = ImsResult {
                status: "XY".into(),
                segments: vec![mainframe_env_host_api::ImsSegment {
                    name: "ROOT".into(),
                    parent_key: None,
                    data: b"ABCDE".to_vec(),
                }],
                checkpoint_id: None,
                affected_segments: 0,
                system: None,
            };
            assert!(machine.finish_ims_copyback(capture, result).is_err());
            let after = machine.snapshot();
            assert_eq!(after.base_storage, before.base_storage);
            assert_eq!(after.implicit_values, before.implicit_values);
            assert_eq!(after.output, before.output);
        }
        for unsupported in ["dynamic", "unbounded", "linkage"] {
            let mut machine = machine();
            let layout = machine.layouts.get_mut("MSG").unwrap();
            match unsupported {
                "dynamic" => layout.dynamic = true,
                "unbounded" => layout.unbounded = true,
                _ => layout.linkage = true,
            }
            assert!(matches!(
                machine.capture_ims_copyback(Some("MSG"), Some("ROOT")),
                Err(MachineProblem::UnsupportedForm)
            ));
        }
    }
}
