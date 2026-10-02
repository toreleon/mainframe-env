//! Compiled VERSION1 CONNX boundary; no provider or lifecycle authority.
use super::*;
use mainframe_env_host_api::mq_raw_layout::{
    MqConnxProfile, MqRawCapture, MqRawCharacterEncoding, MqRawFieldKind, MqRawLayoutKind,
    MqRawNumberEncoding, MqRawStructureEncoding, mq_raw_layout,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Trusted embedding assertion, never inferred from application storage/bindings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiConnxProfile {
    pub profile: MqConnxProfile,
    pub encoding: MqRawStructureEncoding,
}

// Deliberate adapter bound, not an IBM CURRENT_LENGTH or accepted wire identity.
const MAX_CNO_CAPACITY: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Storage {
    pub(super) layout: LayoutMetadata,
    pub(super) view: StorageView,
    pub(super) bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Capture {
    pub(super) profile: MqMqiConnxProfile,
    raw: MqRawCapture,
    arguments: Vec<Storage>,
    members: Vec<Storage>,
}

pub(super) fn contained<T>(
    callback: impl FnOnce() -> Result<T, MachineProblem>,
) -> Result<T, MachineProblem> {
    catch_unwind(AssertUnwindSafe(callback))
        .map_err(|_| MachineProblem::Host(HostProblem::UnknownOutcome))?
}

fn fixed(layout: &LayoutMetadata) -> bool {
    layout.occurs == 1
        && layout.occurs_min == 1
        && !layout.occurs_clause
        && !layout.dynamic
        && !layout.unbounded
        && layout.depending_on.is_none()
        && layout.alias_of.is_none()
        && layout.element_length == layout.length
}

impl State {
    fn checked_connx(
        &self,
        invocation: &Invocation,
        expected: Option<MqMqiConnxProfile>,
    ) -> Result<MqMqiConnxProfile, MachineProblem> {
        let lookup = || {
            contained(|| {
                self.frame
                    .connx_profile(invocation)
                    .map_err(MachineProblem::Host)
            })
        };
        let first = lookup()?;
        if first.profile != MqConnxProfile::OrdinaryOwnedNonshared
            || first.encoding.numbers != MqRawNumberEncoding::NormalBigEndian
            || first.encoding.characters != MqRawCharacterEncoding::AsciiCompatible
        {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
        // Binary layout encoding uses encode_decimal's big-endian storage;
        // character storage is ASCII-compatible bytes. Never native endian.
        contained(|| self.current(invocation))?;
        let second = lookup()?;
        contained(|| self.current(invocation))?;
        if first != second
            || expected.is_some_and(|old| old != first)
            || self.connx_profile.is_some_and(|old| old != first)
        {
            return Err(MachineProblem::Host(HostProblem::IdempotencyConflict));
        }
        Ok(first)
    }
}

impl ReferenceMachine {
    pub(super) fn connx_storage(&self, name: &str) -> Result<Storage, MachineProblem> {
        let layout = self
            .layout(name)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        if !fixed(&layout) || layout.length > MAX_CNO_CAPACITY {
            return Err(MachineProblem::UnsupportedForm);
        }
        let view = self
            .views
            .get(&layout.name)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        if view.length != layout.length
            || view
                .offset
                .checked_add(view.length)
                .and_then(|end| {
                    self.bases
                        .get(view.base)
                        .and_then(|base| base.get(view.offset..end))
                })
                .is_none()
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        let bytes = self.read(&layout.name)?;
        if bytes.len() != view.length {
            return Err(MachineProblem::UnsupportedForm);
        }
        Ok(Storage {
            layout,
            view,
            bytes,
        })
    }

    pub(super) fn prepare_connx(
        &self,
        parameters: &[String],
    ) -> Result<(MqMqiRequest, Targets), MachineProblem> {
        if parameters.len() != 5 {
            return Err(MachineProblem::InvalidOperation);
        }
        let state = self.mqi.as_ref().ok_or(MachineProblem::UnsupportedForm)?;
        let profile = state.checked_connx(&self.invocation, None)?;
        if state.connections.len() >= MQ_MAX_HANDLE_SLOTS || state.next_connection == i32::MAX {
            return Err(MachineProblem::ResourceExhausted);
        }
        for parameter in &parameters[2..] {
            self.mq_long_target(parameter)?;
        }
        // Includes inputs: output storage must not overwrite manager/CNO or any
        // other output, even through a REDEFINES name or overlapping group.
        self.mq_output_aliases(parameters)?;
        let arguments = parameters
            .iter()
            .map(|p| self.connx_storage(p))
            .collect::<Result<Vec<_>, _>>()?;
        if arguments[2..].iter().any(|s| s.layout.native_binary) {
            return Err(MachineProblem::UnsupportedForm);
        }
        let manager = &arguments[0];
        if manager.layout.category != LayoutCategory::Alphanumeric || manager.view.length != 48 {
            return Err(MachineProblem::UnsupportedForm);
        }
        let significant = manager
            .bytes
            .split(|&b| b == 0)
            .next()
            .unwrap_or(&manager.bytes);
        let name = std::str::from_utf8(significant)
            .map_err(|_| MachineProblem::DataException)?
            .trim_end_matches(' ');
        let manager = (!name.is_empty())
            .then(|| MqRouteName::new(name).map_err(|_| MachineProblem::DataException))
            .transpose()?;
        let cno = &arguments[1];
        let descriptor = mq_raw_layout(MqRawLayoutKind::Cno1);
        if cno.layout.category != LayoutCategory::Group
            || !(descriptor.prefix_bytes..=MAX_CNO_CAPACITY).contains(&cno.view.length)
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        // The call page's level01 CONNECTOPTS/COPY wraps the declaration's
        // level10 MQCNO. Admit that one checked wrapper as well as a direct
        // prefix, never recursively guess a structure from arbitrary groups.
        let direct = self
            .layouts
            .values()
            .filter(|l| {
                l.parent.as_deref() == Some(&cno.layout.name)
                    && l.category != LayoutCategory::Condition
                    && l.offset
                        .checked_sub(cno.layout.offset)
                        .is_some_and(|offset| offset < descriptor.prefix_bytes)
            })
            .take(descriptor.fields.len() + 1)
            .collect::<Vec<_>>();
        let wrapper = if direct.len() == 1 && direct[0].category == LayoutCategory::Group {
            let inner = self.connx_storage(&direct[0].name)?;
            if inner.layout.offset != cno.layout.offset
                || inner.view.offset != cno.view.offset
                || inner.view.base != cno.view.base
                || inner.view.length < descriptor.prefix_bytes
                || inner.view.length > cno.view.length
            {
                return Err(MachineProblem::UnsupportedForm);
            }
            Some(inner)
        } else {
            None
        };
        let prefix = wrapper.as_ref().unwrap_or(cno);
        let prefix_end = prefix
            .layout
            .offset
            .checked_add(descriptor.prefix_bytes)
            .ok_or(MachineProblem::UnsupportedForm)?;
        // A deeper/overlapping declaration is not the reviewed direct-field
        // prefix, even when its current byte contents happen to match CNO1.
        for layout in self.layouts.values().filter(|l| {
            l.category != LayoutCategory::Condition
                && l.offset >= prefix.layout.offset
                && l.offset < prefix_end
        }) {
            let mut parent = layout.parent.as_deref();
            let mut depth = 0;
            while let Some(name) = parent {
                if name == prefix.layout.name {
                    if layout.parent.as_deref() != Some(name) {
                        return Err(MachineProblem::UnsupportedForm);
                    }
                    break;
                }
                depth += 1;
                if depth > 32 {
                    return Err(MachineProblem::UnsupportedForm);
                }
                parent = self.layouts.get(name).and_then(|l| l.parent.as_deref());
            }
        }
        let mut members = self
            .layouts
            .values()
            .filter(|l| {
                l.parent.as_deref() == Some(&prefix.layout.name)
                    && l.category != LayoutCategory::Condition
                    && l.offset < prefix_end
            })
            .take(descriptor.fields.len() + 1)
            .map(|l| self.connx_storage(&l.name))
            .collect::<Result<Vec<_>, _>>()?;
        members.sort_by_key(|s| s.layout.offset);
        if members.len() != descriptor.fields.len() {
            return Err(MachineProblem::UnsupportedForm);
        }
        for (member, field) in members.iter().zip(descriptor.fields) {
            if member.layout.offset.checked_sub(prefix.layout.offset) != Some(field.offset)
                || member.view.base != prefix.view.base
                || member.view.offset.checked_sub(prefix.view.offset) != Some(field.offset)
                || member.view.length != field.width
            {
                return Err(MachineProblem::UnsupportedForm);
            }
            match field.kind {
                MqRawFieldKind::Characters
                    if member.layout.category == LayoutCategory::Alphanumeric => {}
                MqRawFieldKind::Long => {
                    self.mq_long_target(&member.layout.name)?;
                    if member.layout.native_binary {
                        return Err(MachineProblem::UnsupportedForm);
                    }
                }
                _ => return Err(MachineProblem::UnsupportedForm),
            }
        }
        let raw = MqRawCapture::capture(MqRawLayoutKind::Cno1, &cno.bytes, profile.encoding)
            .map_err(|_| MachineProblem::UnsupportedForm)?;
        let request = raw
            .decode_connx_default(profile.profile, manager)
            .map_err(|_| MachineProblem::Host(HostProblem::Unsupported))?;
        if let Some(wrapper) = wrapper {
            members.push(wrapper);
        }
        Ok((
            MqMqiRequest::ConnectExtended(request),
            Targets {
                call: MqMqiCall::ConnectExtended,
                connection: parameters[2].clone(),
                completion: parameters[3].clone(),
                reason: parameters[4].clone(),
                disconnected: None,
                unit: None,
                connx: Some(Capture {
                    profile,
                    raw,
                    arguments,
                    members,
                }),
                connect: None,
            },
        ))
    }

    pub(super) fn recheck_connx(&self, capture: &Capture) -> Result<(), MachineProblem> {
        self.mqi
            .as_ref()
            .ok_or(MachineProblem::InvalidOperation)?
            .checked_connx(&self.invocation, Some(capture.profile))?;
        for stored in capture.arguments.iter().chain(&capture.members) {
            if self.connx_storage(&stored.layout.name)? != *stored {
                return Err(MachineProblem::Host(HostProblem::IdempotencyConflict));
            }
        }
        // Prefix identity remains exact; suffix is included in stored bytes.
        if capture.raw.prefix() != &capture.arguments[1].bytes[..capture.raw.layout().prefix_bytes]
        {
            return Err(MachineProblem::InvalidOperation);
        }
        Ok(())
    }

    pub(super) fn write_connx(
        &mut self,
        capture: &Capture,
        targets: &Targets,
        wire: Option<i32>,
        completion: i32,
        reason: i32,
    ) -> Result<(), MachineProblem> {
        let mut writes = Vec::with_capacity(3);
        for (target, value) in [
            (&targets.connection, wire),
            (&targets.completion, Some(completion)),
            (&targets.reason, Some(reason)),
        ] {
            let Some(value) = value else { continue };
            self.mq_long_target(target)?;
            let storage = self.connx_storage(target)?;
            let bytes = encode_decimal(
                &storage.layout,
                Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                },
            )?;
            if bytes.len() != storage.view.length {
                return Err(MachineProblem::UnsupportedForm);
            }
            writes.push((storage.view, bytes));
        }
        // No callback, allocation, encoding or fallible storage resolution after
        // this final recheck. Touched rows only; no whole-machine clone.
        self.recheck_connx(capture)?;
        for (view, bytes) in writes {
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
        }
        Ok(())
    }
}
