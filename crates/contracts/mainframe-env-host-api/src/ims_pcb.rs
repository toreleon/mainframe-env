//! Bounded IMS PCB mask layout and execution-context contracts.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Explicit IMS execution environment used for mask and call applicability checks.
pub enum ImsExecutionContext {
    /// Combined database/data-communications context.
    DbDc,
    /// Database-control context.
    Dbctl,
    /// Data-communications-control context.
    Dcctl,
    /// Database batch context.
    DbBatch,
    /// Transaction-manager batch context.
    TmBatch,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Closed PCB layout family; kind identity does not allocate or authorize a PCB.
pub enum ImsPcbKind {
    /// Database PCB mask.
    Database,
    /// GSAM PCB mask.
    Gsam,
    /// Input/output PCB mask.
    Io,
    /// Alternate destination PCB mask.
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Field identity in a generated PCB declaration; widths and applicability come from its descriptor.
pub enum ImsPcbField {
    /// Database identity bytes; width and context meaning are supplied by the mask descriptor.
    DatabaseName,
    /// Segment depth bytes; width and context meaning are supplied by the mask descriptor.
    SegmentLevelNumber,
    /// Two-byte application status; width and context meaning are supplied by the mask descriptor.
    StatusCode,
    /// PROCOPT declaration bytes; width and context meaning are supplied by the mask descriptor.
    ProcessingOptions,
    /// Reserved IMS bytes; width and context meaning are supplied by the mask descriptor.
    ReservedForIms,
    /// Observed segment identity bytes; width and context meaning are supplied by the mask descriptor.
    SegmentName,
    /// Declared feedback byte length; width and context meaning are supplied by the mask descriptor.
    KeyFeedbackLength,
    /// Sensitive-segment count field; width and context meaning are supplied by the mask descriptor.
    SensitiveSegmentCount,
    /// Variable feedback bytes; width and context meaning are supplied by the mask descriptor.
    KeyFeedbackArea,
    /// GSAM combined feedback/undefined-length area; width and context meaning are supplied by the mask descriptor.
    KeyFeedbackAndUndefinedLength,
    /// GSAM record-search bytes; width and context meaning are supplied by the mask descriptor.
    RecordSearchArgument,
    /// Undefined record-length field; width and context meaning are supplied by the mask descriptor.
    UndefinedRecordLength,
    /// Logical terminal identity bytes; width and context meaning are supplied by the mask descriptor.
    LogicalTerminalName,
    /// Local date bytes; width and context meaning are supplied by the mask descriptor.
    LocalDate,
    /// Local time bytes; width and context meaning are supplied by the mask descriptor.
    LocalTime,
    /// Input message sequence field; width and context meaning are supplied by the mask descriptor.
    InputMessageSequenceNumber,
    /// Output descriptor identity bytes; width and context meaning are supplied by the mask descriptor.
    MessageOutputDescriptorName,
    /// User identity bytes; width and context meaning are supplied by the mask descriptor.
    UserId,
    /// Group identity bytes; width and context meaning are supplied by the mask descriptor.
    GroupName,
    /// Extended date bytes; width and context meaning are supplied by the mask descriptor.
    ExtendedDate,
    /// Extended time bytes; width and context meaning are supplied by the mask descriptor.
    ExtendedTime,
    /// UTC offset bytes; width and context meaning are supplied by the mask descriptor.
    UtcOffset,
    /// User identity indicator; width and context meaning are supplied by the mask descriptor.
    UserIdIndicator,
    /// Reserved extension bytes; width and context meaning are supplied by the mask descriptor.
    ReservedExtension,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// PCB field byte-width rule resolved under explicit variable-area bounds.
pub enum ImsPcbFieldWidth {
    /// Exactly the declared byte width.
    Fixed(usize),
    /// Caller-selected key feedback bytes bounded by complete-mask capacity.
    VariableKeyFeedback,
    /// Caller-selected width must exactly match one declared alternative.
    OneOf(&'static [usize]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Whether a PCB field is application-defined, reserved or unused in the selected mask.
pub enum ImsPcbSemanticValue {
    /// Meaningful application field in applicable contexts.
    Application,
    /// Reserved bytes, not application-defined output.
    Reserved,
    /// Field present in layout but unused for this mask family.
    Unused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Ordered mask field, byte-width rule and context applicability without native memory access.
pub struct ImsPcbFieldDescriptor {
    /// Generated field identity in declaration order.
    pub field: ImsPcbField,
    /// Byte-width rule resolved by checked layout construction.
    pub width: ImsPcbFieldWidth,
    /// Application/reserved/unused classification preserved separately from raw width.
    pub semantic_value: ImsPcbSemanticValue,
    /// Exact contexts in which this field is meaningful.
    pub applicable_contexts: &'static [ImsExecutionContext],
}

impl ImsPcbFieldDescriptor {
    #[must_use]
    /// Test exact field-context membership without changing the mask layout.
    pub fn applies_in(&self, context: ImsExecutionContext) -> bool {
        self.applicable_contexts.contains(&context)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source-pinned PCB mask declaration with explicit allowed contexts and ordered fields.
pub struct ImsPcbMaskDescriptor {
    /// Closed layout family selected by the caller.
    pub kind: ImsPcbKind,
    /// Pinned publication topic locator; metadata presence earns no execution credit.
    pub source_topic: &'static str,
    /// Expected source-body SHA-256, binding the descriptor to retained source bytes.
    pub source_sha256: &'static str,
    /// Exact contexts permitted for the entire mask.
    pub allowed_contexts: &'static [ImsExecutionContext],
    /// Generated field order used for offsets; never sorted by field name.
    pub fields: &'static [ImsPcbFieldDescriptor],
}

impl ImsPcbMaskDescriptor {
    #[must_use]
    /// Test exact whole-mask execution-context membership.
    pub fn allowed_in(&self, context: ImsExecutionContext) -> bool {
        self.allowed_contexts.contains(&context)
    }
}

include!("generated/ims_pcb_masks.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Maximum complete PCB mask size in bytes; zero refuses layout construction.
pub struct ImsPcbLimits {
    /// Maximum complete resolved mask byte size; zero rejects every layout.
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
/// Layout refusal preserving context, variable-area and capacity failures.
pub enum ImsPcbProblem {
    /// The selected mask is not allowed in this context.
    ForbiddenExecutionContext,
    /// Variable bytes supplied to a fixed mask or not one of its declared alternatives.
    InvalidVariableAreaLength,
    /// Zero capacity, arithmetic overflow, multiple variable fields or total mask bound exceeded.
    ResourceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Checked byte layout retaining its exact generated mask and resolved variable field size.
pub struct ImsPcbLayout {
    mask: &'static ImsPcbMaskDescriptor,
    variable_field_bytes: usize,
    total_bytes: usize,
}

impl ImsPcbLayout {
    #[must_use]
    /// Borrow the exact generated mask used by this checked layout.
    pub const fn mask(self) -> &'static ImsPcbMaskDescriptor {
        self.mask
    }

    #[must_use]
    /// Return complete resolved mask capacity in bytes.
    pub const fn total_bytes(self) -> usize {
        self.total_bytes
    }

    #[must_use]
    /// Return the resolved field byte width by zero-based declaration index; unknown index returns None.
    pub fn field_width(self, index: usize) -> Option<usize> {
        self.mask
            .fields
            .get(index)
            .map(|field| resolved_width(field.width, self.variable_field_bytes))
    }

    #[must_use]
    /// Return the checked zero-based byte offset in declaration order; unknown index or overflow returns None.
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
    /// Return the byte offset of the declared status field, if present.
    pub fn status_offset(self) -> Option<usize> {
        self.mask
            .fields
            .iter()
            .position(|field| field.field == ImsPcbField::StatusCode)
            .and_then(|index| self.field_offset(index))
    }
}

#[must_use]
/// Borrow the complete generated mask registry; this does not allocate PCB storage.
pub fn ims_pcb_masks() -> &'static [ImsPcbMaskDescriptor] {
    IMS_PCB_MASKS
}

#[must_use]
/// Select the generated mask for a closed kind; panics only if the generated registry violates closed-kind coverage.
pub fn ims_pcb_mask(kind: ImsPcbKind) -> &'static ImsPcbMaskDescriptor {
    IMS_PCB_MASKS
        .iter()
        .find(|mask| mask.kind == kind)
        .expect("generated IMS PCB mask registry must cover every closed kind")
}

/// Resolve fields with checked arithmetic, exact context and at most one variable area; reject overflow, invalid alternate widths or total capacity excess.
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
