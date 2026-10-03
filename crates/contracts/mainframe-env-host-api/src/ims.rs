//! Typed, bounded IMS SSA grammar contracts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Reviewed SSA command byte and applicability metadata; it is not provider execution.
pub struct ImsSsaCommandCodeDescriptor {
    /// Exact reviewed display command byte.
    pub code: u8,
    /// Reviewed behavior label; not a runtime command implementation.
    pub behavior: &'static str,
    /// True when this command requires a pointer digit 1-8.
    pub subset_pointer: bool,
    /// Retained DEDB applicability restriction for later context admission.
    pub dedb_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Comparison identity resolved from exact accepted two-byte SSA syntax.
pub enum ImsSsaRelation {
    /// Equality comparison.
    Equal,
    /// Strict greater-than comparison.
    GreaterThan,
    /// Strict less-than comparison.
    LessThan,
    /// Inclusive greater-than comparison.
    GreaterOrEqual,
    /// Inclusive less-than comparison.
    LessOrEqual,
    /// Inequality comparison.
    NotEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One comparison identity and its accepted two-byte display encodings.
pub struct ImsSsaRelationDescriptor {
    /// Typed comparison identity.
    pub relation: ImsSsaRelation,
    /// All exact admitted two-byte display spellings.
    pub encodings: &'static [[u8; 2]],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Boolean connector identity retained without collapsing dependent and independent conjunction.
pub enum ImsSsaBoolean {
    /// Dependent conjunction, retained distinctly from independent AND.
    DependentAnd,
    /// Logical disjunction.
    LogicalOr,
    /// Independent conjunction, retained distinctly from dependent AND.
    IndependentAnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One connector and its exact display-code byte.
pub struct ImsSsaBooleanDescriptor {
    /// Typed conjunction/disjunction identity.
    pub connector: ImsSsaBoolean,
    /// Exact one-byte display spelling.
    pub encoding: u8,
}

include!("generated/ims_ssa_rules.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Accepted command byte and optional numeric subset pointer, retaining input command order.
pub struct ImsSsaCommand {
    /// Exact admitted command byte.
    pub code: u8,
    /// Optional numeric pointer in 1-8, only for commands requiring it.
    pub subset_pointer: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Named metadata field or explicit one-based byte position and length.
pub enum ImsSsaField {
    /// Resolve a named field through trusted metadata.
    Named(String),
    /// Use a positive one-based byte slice for record search.
    Offset {
        /** Positive one-based record-search byte position. */
        position: u16,
        /** Positive record-search byte count. */
        length: u16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Comparison field, relation and exact fixed-width value bytes, without trimming or value scanning.
pub struct ImsSsaPredicate {
    /// Named metadata slice or checked explicit record-search range.
    pub field: ImsSsaField,
    /// Admitted exact comparison identity.
    pub relation: ImsSsaRelation,
    /// Exact comparison bytes, retained at the metadata-declared width.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Parsed display-code SSA preserving command-format choice, qualification and binary comparison bytes.
pub struct ImsSsa {
    /// Parsed fixed-width segment name, excluding trailing blank padding.
    pub segment: String,
    /// True for the explicit asterisk command form, including an empty command list.
    pub command_format: bool,
    /// Accepted command sequence in source order.
    pub commands: Vec<ImsSsaCommand>,
    /// Optional exact concatenated-key bytes, mutually exclusive with predicates.
    pub concatenated_key: Option<Vec<u8>>,
    /// Ordered accepted field comparisons.
    pub predicates: Vec<ImsSsaPredicate>,
    /// Exactly one connector between adjacent predicates; empty for an unqualified form.
    pub connectors: Vec<ImsSsaBoolean>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Finite SSA input, command, predicate and value byte ceilings checked by the parser.
pub struct ImsSsaLimits {
    /// Maximum complete SSA input bytes.
    pub max_bytes: usize,
    /// Positive maximum parsed command count.
    pub max_command_codes: usize,
    /// Positive maximum parsed predicate count.
    pub max_predicates: usize,
    /// Positive maximum individual comparison/key bytes.
    pub max_value_bytes: usize,
}

impl Default for ImsSsaLimits {
    fn default() -> Self {
        Self {
            max_bytes: 32 * 1024,
            max_command_codes: 16,
            max_predicates: 16,
            max_value_bytes: 32 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// SSA parsing refusal; malformed/truncated input is never admitted as an unqualified request.
pub enum ImsSsaProblem {
    /// Input cannot contain the fixed segment name and control byte.
    TooShort,
    /// Input, count, value or arithmetic bound exceeded.
    ResourceExhausted,
    /// Segment name has invalid bytes or internal blank padding.
    InvalidSegmentName,
    /// The command/qualification boundary is not admitted syntax.
    InvalidControlByte,
    /// A command byte is absent from the reviewed vocabulary.
    InvalidCommandCode,
    /// Required pointer is absent/outside 1-8 or supplied for a nonpointer command.
    InvalidSubsetPointer,
    /// Command form ends without its required terminator.
    MissingCommandTerminator,
    /// Concatenated-key form conflicts with predicate qualification.
    CommandQualificationConflict,
    /// Metadata supplies no concatenated-key width.
    UnknownConcatenatedKey,
    /// Fixed field name is malformed.
    InvalidFieldName,
    /// Metadata supplies no named field width.
    UnknownField,
    /// Explicit record-search slice is zero, invalid or overflowing.
    InvalidOffset,
    /// Two-byte relation spelling is not reviewed.
    InvalidRelation,
    /// Connector byte is not reviewed.
    InvalidBooleanConnector,
    /// A forbidden parenthesis occurs within exact comparison bytes.
    ParenthesisInValue,
    /// Input ends before its metadata-declared comparison/key width.
    Truncated,
    /// Bytes remain after the completed SSA form.
    TrailingData,
}

/// Trusted metadata lengths for exact SSA slicing; unknown fields/keys fail closed.
pub trait ImsSsaFieldResolver {
    /// Return exact comparison byte width for the named segment field; None refuses unknown metadata.
    fn field_length(&self, segment: &str, field: &str) -> Option<usize>;

    /// Return exact concatenated-key byte width; the default None refuses unsupported key qualification.
    fn concatenated_key_length(&self, _segment: &str) -> Option<usize> {
        None
    }
}

impl<F> ImsSsaFieldResolver for F
where
    F: Fn(&str, &str) -> Option<usize>,
{
    fn field_length(&self, segment: &str, field: &str) -> Option<usize> {
        self(segment, field)
    }
}

#[must_use]
/// Look up exact reviewed command-byte metadata; unknown bytes return None.
pub fn ims_ssa_command_code(code: u8) -> Option<&'static ImsSsaCommandCodeDescriptor> {
    IMS_SSA_COMMAND_CODES
        .iter()
        .find(|descriptor| descriptor.code == code)
}

#[must_use]
/// Resolve only exact reviewed two-byte relation encodings; unknown bytes return None.
pub fn ims_ssa_relation(bytes: [u8; 2]) -> Option<ImsSsaRelation> {
    IMS_SSA_RELATIONAL_OPERATORS
        .iter()
        .find(|descriptor| descriptor.encodings.contains(&bytes))
        .map(|descriptor| descriptor.relation)
}

#[must_use]
/// Resolve only exact reviewed connector bytes; unknown bytes return None.
pub fn ims_ssa_boolean(byte: u8) -> Option<ImsSsaBoolean> {
    IMS_SSA_BOOLEAN_CONNECTORS
        .iter()
        .find(|descriptor| descriptor.encoding == byte)
        .map(|descriptor| descriptor.connector)
}

/// Parse display-code SSA bytes after the encoding adapter has converted
/// invariant syntax. DBD metadata supplies exact comparative field lengths.
pub fn parse_ims_ssa(
    input: &[u8],
    limits: ImsSsaLimits,
    fields: &dyn ImsSsaFieldResolver,
) -> Result<ImsSsa, ImsSsaProblem> {
    if input.len() < IMS_SSA_SEGMENT_NAME_BYTES + 1 {
        return Err(ImsSsaProblem::TooShort);
    }
    if input.len() > limits.max_bytes
        || limits.max_command_codes == 0
        || limits.max_predicates == 0
        || limits.max_value_bytes == 0
    {
        return Err(ImsSsaProblem::ResourceExhausted);
    }
    let segment = fixed_name(
        &input[..IMS_SSA_SEGMENT_NAME_BYTES],
        ImsSsaProblem::InvalidSegmentName,
    )?;
    let mut cursor = IMS_SSA_SEGMENT_NAME_BYTES;
    let mut command_format = false;
    let mut commands = Vec::new();
    let mut command_slots = 0usize;

    match input[cursor] {
        b' ' => {
            cursor += 1;
            if cursor != input.len() {
                return Err(ImsSsaProblem::TrailingData);
            }
            return Ok(ImsSsa {
                segment,
                command_format,
                commands,
                concatenated_key: None,
                predicates: Vec::new(),
                connectors: Vec::new(),
            });
        }
        b'*' => {
            command_format = true;
            cursor += 1;
            while cursor < input.len() && !matches!(input[cursor], b' ' | b'(') {
                if command_slots >= limits.max_command_codes {
                    return Err(ImsSsaProblem::ResourceExhausted);
                }
                command_slots += 1;
                let code = input[cursor];
                if code == IMS_SSA_NULL_COMMAND {
                    cursor += 1;
                    if input.get(cursor).is_some_and(u8::is_ascii_digit) {
                        return Err(ImsSsaProblem::InvalidSubsetPointer);
                    }
                    continue;
                }
                let descriptor =
                    ims_ssa_command_code(code).ok_or(ImsSsaProblem::InvalidCommandCode)?;
                cursor += 1;
                let subset_pointer = if descriptor.subset_pointer {
                    let value = input
                        .get(cursor)
                        .copied()
                        .ok_or(ImsSsaProblem::InvalidSubsetPointer)?;
                    if !(b'1'..=b'8').contains(&value) {
                        return Err(ImsSsaProblem::InvalidSubsetPointer);
                    }
                    cursor += 1;
                    Some(value - b'0')
                } else {
                    if input.get(cursor).is_some_and(u8::is_ascii_digit) {
                        return Err(ImsSsaProblem::InvalidSubsetPointer);
                    }
                    None
                };
                commands.push(ImsSsaCommand {
                    code,
                    subset_pointer,
                });
            }
            let control = input
                .get(cursor)
                .copied()
                .ok_or(ImsSsaProblem::MissingCommandTerminator)?;
            if control == b' ' {
                cursor += 1;
                if cursor != input.len() {
                    return Err(ImsSsaProblem::TrailingData);
                }
                return Ok(ImsSsa {
                    segment,
                    command_format,
                    commands,
                    concatenated_key: None,
                    predicates: Vec::new(),
                    connectors: Vec::new(),
                });
            }
        }
        b'(' => {}
        _ => return Err(ImsSsaProblem::InvalidControlByte),
    }

    if input.get(cursor) != Some(&b'(') {
        return Err(ImsSsaProblem::InvalidControlByte);
    }
    cursor += 1;
    if commands.iter().any(|command| command.code == b'C') {
        let key_length = fields
            .concatenated_key_length(&segment)
            .ok_or(ImsSsaProblem::UnknownConcatenatedKey)?;
        if key_length == 0 || key_length > limits.max_value_bytes {
            return Err(ImsSsaProblem::ResourceExhausted);
        }
        let key_end = cursor
            .checked_add(key_length)
            .ok_or(ImsSsaProblem::ResourceExhausted)?;
        let key = input.get(cursor..key_end).ok_or(ImsSsaProblem::Truncated)?;
        if key.contains(&b'(') || key.contains(&b')') {
            return Err(ImsSsaProblem::ParenthesisInValue);
        }
        if input.get(key_end) != Some(&b')') || key_end + 1 != input.len() {
            return Err(ImsSsaProblem::CommandQualificationConflict);
        }
        return Ok(ImsSsa {
            segment,
            command_format,
            commands,
            concatenated_key: Some(key.to_vec()),
            predicates: Vec::new(),
            connectors: Vec::new(),
        });
    }
    let mut predicates = Vec::new();
    let mut connectors = Vec::new();
    loop {
        if predicates.len() >= limits.max_predicates {
            return Err(ImsSsaProblem::ResourceExhausted);
        }
        let field_end = cursor
            .checked_add(IMS_SSA_FIELD_NAME_BYTES)
            .ok_or(ImsSsaProblem::ResourceExhausted)?;
        let raw_field = input
            .get(cursor..field_end)
            .ok_or(ImsSsaProblem::Truncated)?;
        cursor = field_end;
        let field = if commands.iter().any(|command| command.code == b'O')
            && raw_field.iter().all(u8::is_ascii_digit)
        {
            let position = decimal(&raw_field[..4]).ok_or(ImsSsaProblem::InvalidOffset)?;
            let length = decimal(&raw_field[4..]).ok_or(ImsSsaProblem::InvalidOffset)?;
            if position == 0 || length == 0 {
                return Err(ImsSsaProblem::InvalidOffset);
            }
            ImsSsaField::Offset { position, length }
        } else {
            ImsSsaField::Named(fixed_name(raw_field, ImsSsaProblem::InvalidFieldName)?)
        };
        let relation_end = cursor
            .checked_add(IMS_SSA_RELATIONAL_OPERATOR_BYTES)
            .ok_or(ImsSsaProblem::ResourceExhausted)?;
        let raw_relation = input
            .get(cursor..relation_end)
            .ok_or(ImsSsaProblem::Truncated)?;
        cursor = relation_end;
        let relation = ims_ssa_relation([raw_relation[0], raw_relation[1]])
            .ok_or(ImsSsaProblem::InvalidRelation)?;
        let value_length = match &field {
            ImsSsaField::Named(name) => fields
                .field_length(&segment, name)
                .ok_or(ImsSsaProblem::UnknownField)?,
            ImsSsaField::Offset { length, .. } => usize::from(*length),
        };
        if value_length == 0 || value_length > limits.max_value_bytes {
            return Err(ImsSsaProblem::ResourceExhausted);
        }
        let value_end = cursor
            .checked_add(value_length)
            .ok_or(ImsSsaProblem::ResourceExhausted)?;
        let value = input
            .get(cursor..value_end)
            .ok_or(ImsSsaProblem::Truncated)?;
        if value.contains(&b'(') || value.contains(&b')') {
            return Err(ImsSsaProblem::ParenthesisInValue);
        }
        cursor = value_end;
        predicates.push(ImsSsaPredicate {
            field,
            relation,
            value: value.to_vec(),
        });
        let control = input.get(cursor).copied().ok_or(ImsSsaProblem::Truncated)?;
        cursor += 1;
        if control == b')' {
            if cursor != input.len() {
                return Err(ImsSsaProblem::TrailingData);
            }
            break;
        }
        connectors.push(ims_ssa_boolean(control).ok_or(ImsSsaProblem::InvalidBooleanConnector)?);
    }
    debug_assert_eq!(connectors.len() + 1, predicates.len());
    Ok(ImsSsa {
        segment,
        command_format,
        commands,
        concatenated_key: None,
        predicates,
        connectors,
    })
}

fn fixed_name(bytes: &[u8], problem: ImsSsaProblem) -> Result<String, ImsSsaProblem> {
    let first_blank = bytes
        .iter()
        .position(|byte| *byte == b' ')
        .unwrap_or(bytes.len());
    if first_blank == 0
        || bytes[first_blank..].iter().any(|byte| *byte != b' ')
        || bytes[..first_blank].iter().any(|byte| {
            !(byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'@' | b'#' | b'$'))
        })
    {
        return Err(problem);
    }
    String::from_utf8(bytes[..first_blank].to_vec()).map_err(|_| problem)
}

fn decimal(bytes: &[u8]) -> Option<u16> {
    bytes.iter().try_fold(0_u16, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(u16::from(byte.checked_sub(b'0')?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(_: &str, field: &str) -> Option<usize> {
        match field {
            "KEY" | "NAME" => Some(6),
            "ONE" => Some(1),
            _ => None,
        }
    }

    struct Metadata;

    impl ImsSsaFieldResolver for Metadata {
        fn field_length(&self, segment: &str, field: &str) -> Option<usize> {
            fields(segment, field)
        }

        fn concatenated_key_length(&self, segment: &str) -> Option<usize> {
            (segment == "ILLNESS").then_some(13)
        }
    }

    #[test]
    fn parses_unqualified_null_command_and_subset_pointer_forms() {
        let plain = parse_ims_ssa(b"ROOT     ", ImsSsaLimits::default(), &fields).unwrap();
        assert_eq!(plain.segment, "ROOT");
        assert!(!plain.command_format);
        let null = parse_ims_ssa(b"ROOT    * ", ImsSsaLimits::default(), &fields).unwrap();
        assert!(null.command_format);
        assert!(null.commands.is_empty());
        let subset = parse_ims_ssa(b"ROOT    *M1Z8 ", ImsSsaLimits::default(), &fields).unwrap();
        assert_eq!(
            subset.commands,
            [
                ImsSsaCommand {
                    code: b'M',
                    subset_pointer: Some(1)
                },
                ImsSsaCommand {
                    code: b'Z',
                    subset_pointer: Some(8)
                }
            ]
        );
    }

    #[test]
    fn parses_named_offset_and_boolean_qualifications_without_scanning_values() {
        let named = parse_ims_ssa(
            b"ROOT    (KEY     EQ000001&NAME    NEALICE )",
            ImsSsaLimits::default(),
            &fields,
        )
        .unwrap();
        assert_eq!(named.predicates.len(), 2);
        assert_eq!(named.connectors, [ImsSsaBoolean::DependentAnd]);
        assert_eq!(named.predicates[1].value, b"ALICE ");
        let offset = parse_ims_ssa(
            b"CHILD   *DO(00010004>=ABCD)",
            ImsSsaLimits::default(),
            &fields,
        )
        .unwrap();
        assert_eq!(
            offset.predicates[0].field,
            ImsSsaField::Offset {
                position: 1,
                length: 4
            }
        );
        assert_eq!(
            offset.predicates[0].relation,
            ImsSsaRelation::GreaterOrEqual
        );

        let concatenated = parse_ims_ssa(
            b"ILLNESS *C(0775520090303)",
            ImsSsaLimits::default(),
            &Metadata,
        )
        .unwrap();
        assert_eq!(
            concatenated.concatenated_key.as_deref(),
            Some(b"0775520090303".as_slice())
        );
        assert!(concatenated.predicates.is_empty());
    }

    #[test]
    fn every_reviewed_relation_and_boolean_encoding_resolves_exactly() {
        assert_eq!(IMS_SSA_COMMAND_CODES.len(), 17);
        for descriptor in IMS_SSA_RELATIONAL_OPERATORS {
            for encoding in descriptor.encodings {
                assert_eq!(ims_ssa_relation(*encoding), Some(descriptor.relation));
            }
        }
        for descriptor in IMS_SSA_BOOLEAN_CONNECTORS {
            assert_eq!(
                ims_ssa_boolean(descriptor.encoding),
                Some(descriptor.connector)
            );
        }
        assert_eq!(ims_ssa_relation(*b"!="), None);
        assert_eq!(ims_ssa_boolean(b'!'), None);
    }

    #[test]
    fn malformed_names_commands_offsets_and_qualifications_fail_closed() {
        let limits = ImsSsaLimits::default();
        for (input, problem) in [
            (b"ROOT".as_slice(), ImsSsaProblem::TooShort),
            (b"RO OT    ".as_slice(), ImsSsaProblem::InvalidSegmentName),
            (b"ROOT    ?".as_slice(), ImsSsaProblem::InvalidControlByte),
            (b"ROOT    *B ".as_slice(), ImsSsaProblem::InvalidCommandCode),
            (
                b"ROOT    *M ".as_slice(),
                ImsSsaProblem::InvalidSubsetPointer,
            ),
            (
                b"ROOT    *M9 ".as_slice(),
                ImsSsaProblem::InvalidSubsetPointer,
            ),
            (
                b"ROOT    *A1 ".as_slice(),
                ImsSsaProblem::InvalidSubsetPointer,
            ),
            (
                b"ROOT    *O(00000004EQABCD)".as_slice(),
                ImsSsaProblem::InvalidOffset,
            ),
            (
                b"ROOT    (MISSING EQ000001)".as_slice(),
                ImsSsaProblem::UnknownField,
            ),
            (
                b"ROOT    (KEY     !=000001)".as_slice(),
                ImsSsaProblem::InvalidRelation,
            ),
            (
                b"ROOT    (KEY     EQ00)001)".as_slice(),
                ImsSsaProblem::ParenthesisInValue,
            ),
            (
                b"ROOT    (KEY     EQ000001!NAME    EQALICE )".as_slice(),
                ImsSsaProblem::InvalidBooleanConnector,
            ),
            (
                b"ROOT    (KEY     EQ000001)X".as_slice(),
                ImsSsaProblem::TrailingData,
            ),
        ] {
            assert_eq!(parse_ims_ssa(input, limits, &fields), Err(problem));
        }
        assert_eq!(
            parse_ims_ssa(b"ILLNESS *C(KEY     EQ000001)", limits, &Metadata,),
            Err(ImsSsaProblem::CommandQualificationConflict)
        );
        assert_eq!(
            parse_ims_ssa(b"ROOT    *C(1234)", limits, &fields),
            Err(ImsSsaProblem::UnknownConcatenatedKey)
        );
    }

    #[test]
    fn byte_command_predicate_and_value_bounds_fail_before_unbounded_work() {
        let tiny = ImsSsaLimits {
            max_bytes: 12,
            max_command_codes: 1,
            max_predicates: 1,
            max_value_bytes: 5,
        };
        assert_eq!(
            parse_ims_ssa(b"ROOT    *AD ", tiny, &fields),
            Err(ImsSsaProblem::ResourceExhausted)
        );
        assert_eq!(
            parse_ims_ssa(b"ROOT    (KEY     EQ000001)", tiny, &fields),
            Err(ImsSsaProblem::ResourceExhausted)
        );
        assert_eq!(
            parse_ims_ssa(
                b"ROOT    (ONE     EQA&ONE     EQB)",
                ImsSsaLimits {
                    max_bytes: 64,
                    max_command_codes: 4,
                    max_predicates: 1,
                    max_value_bytes: 8,
                },
                &fields,
            ),
            Err(ImsSsaProblem::ResourceExhausted)
        );
    }
}
