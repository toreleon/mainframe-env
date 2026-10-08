//! Validate compiled declarations against the sole generated raw projection.
use super::*;
use mainframe_env_host_api::mq_raw_layout::{MqRawFieldKind, mq_raw_layout};

impl ReferenceMachine {
    pub(in crate::machine::typed_mq) fn point_suffix_members(
        &self,
        group: &connx::Storage,
        members: &mut Vec<connx::Storage>,
    ) -> Result<(), MachineProblem> {
        for layout in self.layouts.values().filter(|l| {
            l.category != LayoutCategory::Condition
                && l.offset >= group.layout.offset
                && l.offset < group.layout.offset + group.layout.length
        }) {
            let mut parent = layout.parent.as_deref();
            let mut depth = 0;
            while let Some(name) = parent {
                if name == group.layout.name {
                    if !members.iter().any(|m| m.layout.name == layout.name) {
                        let member = self.connx_storage(&layout.name)?;
                        if member.view.base != group.view.base
                            || member.view.offset < group.view.offset
                            || member.layout.offset.checked_sub(group.layout.offset)
                                != member.view.offset.checked_sub(group.view.offset)
                            || member
                                .view
                                .offset
                                .checked_add(member.view.length)
                                .is_none_or(|end| end > group.view.offset + group.view.length)
                        {
                            return Err(MachineProblem::UnsupportedForm);
                        }
                        members
                            .try_reserve(1)
                            .map_err(|_| MachineProblem::ResourceExhausted)?;
                        members.push(member);
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
        Ok(())
    }

    pub(in crate::machine::typed_mq) fn point_group(
        &self,
        group: &connx::Storage,
        kind: MqRawLayoutKind,
        encoding: MqRawStructureEncoding,
    ) -> Result<(MqRawCapture, Vec<connx::Storage>), MachineProblem> {
        let descriptor = mq_raw_layout(kind);
        if group.layout.category != LayoutCategory::Group
            || group.view.length < descriptor.prefix_bytes
            || !group.view.offset.is_multiple_of(4)
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        let children = self
            .layouts
            .values()
            .filter(|l| {
                l.parent.as_deref() == Some(&group.layout.name)
                    && l.category != LayoutCategory::Condition
                    && l.offset
                        .checked_sub(group.layout.offset)
                        .is_some_and(|o| o < descriptor.prefix_bytes)
            })
            .take(descriptor.fields.len() + 1)
            .collect::<Vec<_>>();
        let wrapper = if children.len() == 1 && children[0].category == LayoutCategory::Group {
            let inner = self.connx_storage(&children[0].name)?;
            if inner.layout.offset != group.layout.offset
                || inner.view.base != group.view.base
                || inner.view.offset != group.view.offset
                || inner.view.length < descriptor.prefix_bytes
                || inner.view.length > group.view.length
            {
                return Err(MachineProblem::UnsupportedForm);
            }
            Some(inner)
        } else {
            None
        };
        let prefix = wrapper.as_ref().unwrap_or(group);
        let end = prefix
            .layout
            .offset
            .checked_add(descriptor.prefix_bytes)
            .ok_or(MachineProblem::UnsupportedForm)?;
        // The declaration is a direct fixed field sequence, not arbitrary bytes,
        // nested groups, REDEFINES, variable OCCURS or a lucky structure-ID value.
        for layout in self.layouts.values().filter(|l| {
            l.category != LayoutCategory::Condition
                && l.offset >= prefix.layout.offset
                && l.offset < end
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
                    && l.offset < end
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
                MqRawFieldKind::Characters | MqRawFieldKind::Bytes
                    if member.layout.category == LayoutCategory::Alphanumeric => {}
                // GMO1 Signal1 is the source-declared COBOL four-byte BINARY
                // slot, but raw capture retains it as opaque signal/pointer
                // bytes. No value/address conversion or dereference occurs;
                // the finite GET constructor refuses SET_SIGNAL entirely.
                MqRawFieldKind::Long | MqRawFieldKind::Alias | MqRawFieldKind::SignalSlot => {
                    self.mq_long_target(&member.layout.name)?;
                    if member.layout.native_binary || member.view.offset % 4 != 0 {
                        return Err(MachineProblem::UnsupportedForm);
                    }
                }
                _ => return Err(MachineProblem::UnsupportedForm),
            }
        }
        let raw = MqRawCapture::capture(kind, &group.bytes, encoding)
            .map_err(|_| MachineProblem::UnsupportedForm)?;
        if let Some(wrapper) = wrapper {
            members.push(wrapper);
        }
        Ok((raw, members))
    }
}
