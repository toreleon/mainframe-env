//! Bounded IMS PCB mask layout and execution-context contracts.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ImsExecutionContext {
    DbDc,
    Dbctl,
    Dcctl,
    DbBatch,
    TmBatch,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ImsPcbKind {
    Database,
    Gsam,
    Io,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ImsPcbField {
    DatabaseName,
    SegmentLevelNumber,
    StatusCode,
    ProcessingOptions,
    ReservedForIms,
    SegmentName,
    KeyFeedbackLength,
    SensitiveSegmentCount,
    KeyFeedbackArea,
    KeyFeedbackAndUndefinedLength,
    RecordSearchArgument,
    UndefinedRecordLength,
    LogicalTerminalName,
    LocalDate,
    LocalTime,
    InputMessageSequenceNumber,
    MessageOutputDescriptorName,
    UserId,
    GroupName,
    ExtendedDate,
    ExtendedTime,
    UtcOffset,
    UserIdIndicator,
    ReservedExtension,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsPcbFieldWidth {
    Fixed(usize),
    VariableKeyFeedback,
    OneOf(&'static [usize]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsPcbSemanticValue {
    Application,
    Reserved,
    Unused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsPcbFieldDescriptor {
    pub field: ImsPcbField,
    pub width: ImsPcbFieldWidth,
    pub semantic_value: ImsPcbSemanticValue,
    pub applicable_contexts: &'static [ImsExecutionContext],
}

impl ImsPcbFieldDescriptor {
    #[must_use]
    pub fn applies_in(&self, context: ImsExecutionContext) -> bool {
        self.applicable_contexts.contains(&context)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsPcbMaskDescriptor {
    pub kind: ImsPcbKind,
    pub source_topic: &'static str,
    pub source_sha256: &'static str,
    pub allowed_contexts: &'static [ImsExecutionContext],
    pub fields: &'static [ImsPcbFieldDescriptor],
}

impl ImsPcbMaskDescriptor {
    #[must_use]
    pub fn allowed_in(&self, context: ImsExecutionContext) -> bool {
        self.allowed_contexts.contains(&context)
    }
}

include!("generated/ims_pcb_masks.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsPcbLimits {
    pub max_mask_bytes: usize,
}

impl Default for ImsPcbLimits {
    fn default() -> Self {
        Self {
            max_mask_bytes: 64 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsPcbProblem {
    ForbiddenExecutionContext,
    InvalidVariableAreaLength,
    ResourceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsPcbLayout {
    mask: &'static ImsPcbMaskDescriptor,
    variable_field_bytes: usize,
    total_bytes: usize,
}

impl ImsPcbLayout {
    #[must_use]
    pub const fn mask(self) -> &'static ImsPcbMaskDescriptor {
        self.mask
    }

    #[must_use]
    pub const fn total_bytes(self) -> usize {
        self.total_bytes
    }

    #[must_use]
    pub fn field_width(self, index: usize) -> Option<usize> {
        self.mask
            .fields
            .get(index)
            .map(|field| resolved_width(field.width, self.variable_field_bytes))
    }

    #[must_use]
    pub fn field_offset(self, index: usize) -> Option<usize> {
        if index >= self.mask.fields.len() {
            return None;
        }
        self.mask.fields[..index]
            .iter()
            .try_fold(0_usize, |offset, field| {
                offset.checked_add(resolved_width(field.width, self.variable_field_bytes))
            })
    }

    #[must_use]
    pub fn status_offset(self) -> Option<usize> {
        self.mask
            .fields
            .iter()
            .position(|field| field.field == ImsPcbField::StatusCode)
            .and_then(|index| self.field_offset(index))
    }
}

#[must_use]
pub fn ims_pcb_masks() -> &'static [ImsPcbMaskDescriptor] {
    IMS_PCB_MASKS
}

#[must_use]
pub fn ims_pcb_mask(kind: ImsPcbKind) -> &'static ImsPcbMaskDescriptor {
    IMS_PCB_MASKS
        .iter()
        .find(|mask| mask.kind == kind)
        .expect("generated IMS PCB mask registry must cover every closed kind")
}

pub fn ims_pcb_layout(
    kind: ImsPcbKind,
    context: ImsExecutionContext,
    variable_field_bytes: usize,
    limits: ImsPcbLimits,
) -> Result<ImsPcbLayout, ImsPcbProblem> {
    let mask = ims_pcb_mask(kind);
    if !mask.allowed_in(context) {
        return Err(ImsPcbProblem::ForbiddenExecutionContext);
    }
    if limits.max_mask_bytes == 0 {
        return Err(ImsPcbProblem::ResourceExhausted);
    }

    let mut variable_fields = 0_usize;
    let mut total_bytes = 0_usize;
    for field in mask.fields {
        let width = match field.width {
            ImsPcbFieldWidth::Fixed(width) => width,
            ImsPcbFieldWidth::VariableKeyFeedback => {
                variable_fields += 1;
                variable_field_bytes
            }
            ImsPcbFieldWidth::OneOf(widths) => {
                variable_fields += 1;
                if !widths.contains(&variable_field_bytes) {
                    return Err(ImsPcbProblem::InvalidVariableAreaLength);
                }
                variable_field_bytes
            }
        };
        total_bytes = total_bytes
            .checked_add(width)
            .ok_or(ImsPcbProblem::ResourceExhausted)?;
    }
    if variable_fields == 0 && variable_field_bytes != 0 {
        return Err(ImsPcbProblem::InvalidVariableAreaLength);
    }
    if variable_fields > 1 || total_bytes > limits.max_mask_bytes {
        return Err(ImsPcbProblem::ResourceExhausted);
    }
    Ok(ImsPcbLayout {
        mask,
        variable_field_bytes,
        total_bytes,
    })
}

fn resolved_width(width: ImsPcbFieldWidth, variable_field_bytes: usize) -> usize {
    match width {
        ImsPcbFieldWidth::Fixed(width) => width,
        ImsPcbFieldWidth::VariableKeyFeedback | ImsPcbFieldWidth::OneOf(_) => variable_field_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcb_masks_preserve_exact_field_order_widths_and_offsets() {
        let database = ims_pcb_layout(
            ImsPcbKind::Database,
            ImsExecutionContext::DbDc,
            13,
            ImsPcbLimits::default(),
        )
        .unwrap();
        assert_eq!(database.total_bytes(), 49);
        assert_eq!(database.status_offset(), Some(10));
        assert_eq!(
            database
                .mask()
                .fields
                .iter()
                .map(|field| field.field)
                .collect::<Vec<_>>(),
            [
                ImsPcbField::DatabaseName,
                ImsPcbField::SegmentLevelNumber,
                ImsPcbField::StatusCode,
                ImsPcbField::ProcessingOptions,
                ImsPcbField::ReservedForIms,
                ImsPcbField::SegmentName,
                ImsPcbField::KeyFeedbackLength,
                ImsPcbField::SensitiveSegmentCount,
                ImsPcbField::KeyFeedbackArea,
            ]
        );
        assert_eq!(
            (0..database.mask().fields.len())
                .map(|index| database.field_width(index).unwrap())
                .collect::<Vec<_>>(),
            [8, 2, 2, 4, 4, 8, 4, 4, 13]
        );

        let gsam_basic = ims_pcb_layout(
            ImsPcbKind::Gsam,
            ImsExecutionContext::TmBatch,
            8,
            ImsPcbLimits::default(),
        )
        .unwrap();
        let gsam_large = ims_pcb_layout(
            ImsPcbKind::Gsam,
            ImsExecutionContext::DbBatch,
            12,
            ImsPcbLimits::default(),
        )
        .unwrap();
        assert_eq!(gsam_basic.total_bytes(), 48);
        assert_eq!(gsam_large.total_bytes(), 52);
        assert_eq!(gsam_basic.status_offset(), Some(10));
        assert_eq!(
            gsam_basic
                .mask()
                .fields
                .iter()
                .filter(|field| field.semantic_value == ImsPcbSemanticValue::Unused)
                .map(|field| field.field)
                .collect::<Vec<_>>(),
            [
                ImsPcbField::SegmentLevelNumber,
                ImsPcbField::SegmentName,
                ImsPcbField::SensitiveSegmentCount,
            ]
        );

        let io = ims_pcb_layout(
            ImsPcbKind::Io,
            ImsExecutionContext::Dbctl,
            0,
            ImsPcbLimits::default(),
        )
        .unwrap();
        let alternate = ims_pcb_layout(
            ImsPcbKind::Alternate,
            ImsExecutionContext::Dcctl,
            0,
            ImsPcbLimits::default(),
        )
        .unwrap();
        assert_eq!(io.total_bytes(), 64);
        assert_eq!(alternate.total_bytes(), 12);
        assert_eq!(io.status_offset(), Some(10));
        assert_eq!(alternate.status_offset(), Some(10));
        assert_eq!(
            (0..io.mask().fields.len())
                .map(|index| io.field_width(index).unwrap())
                .collect::<Vec<_>>(),
            [8, 2, 2, 4, 4, 4, 8, 8, 8, 4, 6, 2, 1, 3]
        );
    }

    #[test]
    fn execution_context_and_field_applicability_are_fail_closed() {
        for context in [
            ImsExecutionContext::DbDc,
            ImsExecutionContext::Dbctl,
            ImsExecutionContext::DbBatch,
        ] {
            assert!(
                ims_pcb_layout(ImsPcbKind::Database, context, 0, ImsPcbLimits::default()).is_ok()
            );
        }
        for context in [ImsExecutionContext::Dcctl, ImsExecutionContext::TmBatch] {
            assert_eq!(
                ims_pcb_layout(ImsPcbKind::Database, context, 0, ImsPcbLimits::default()),
                Err(ImsPcbProblem::ForbiddenExecutionContext)
            );
        }
        for context in [
            ImsExecutionContext::Dbctl,
            ImsExecutionContext::DbBatch,
            ImsExecutionContext::TmBatch,
        ] {
            assert_eq!(
                ims_pcb_layout(ImsPcbKind::Alternate, context, 0, ImsPcbLimits::default()),
                Err(ImsPcbProblem::ForbiddenExecutionContext)
            );
        }

        let io = ims_pcb_mask(ImsPcbKind::Io);
        let terminal = &io.fields[0];
        let status = &io.fields[2];
        assert!(terminal.applies_in(ImsExecutionContext::DbDc));
        assert!(terminal.applies_in(ImsExecutionContext::Dcctl));
        assert!(!terminal.applies_in(ImsExecutionContext::Dbctl));
        assert!(status.applies_in(ImsExecutionContext::Dbctl));
        assert!(status.applies_in(ImsExecutionContext::DbBatch));
        assert!(status.applies_in(ImsExecutionContext::TmBatch));
    }

    #[test]
    fn mask_limits_and_variable_widths_are_enforced() {
        assert_eq!(
            ims_pcb_layout(
                ImsPcbKind::Database,
                ImsExecutionContext::DbDc,
                29,
                ImsPcbLimits { max_mask_bytes: 64 },
            ),
            Err(ImsPcbProblem::ResourceExhausted)
        );
        assert_eq!(
            ims_pcb_layout(
                ImsPcbKind::Gsam,
                ImsExecutionContext::DbDc,
                9,
                ImsPcbLimits::default(),
            ),
            Err(ImsPcbProblem::InvalidVariableAreaLength)
        );
        assert_eq!(
            ims_pcb_layout(
                ImsPcbKind::Io,
                ImsExecutionContext::DbDc,
                1,
                ImsPcbLimits::default(),
            ),
            Err(ImsPcbProblem::InvalidVariableAreaLength)
        );
    }
}
