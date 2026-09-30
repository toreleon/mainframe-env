//! Typed, bounded IMS SSA grammar contracts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsSsaCommandCodeDescriptor {
    pub code: u8,
    pub behavior: &'static str,
    pub subset_pointer: bool,
    pub dedb_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsSsaRelation {
    Equal,
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
    NotEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsSsaRelationDescriptor {
    pub relation: ImsSsaRelation,
    pub encodings: &'static [[u8; 2]],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsSsaBoolean {
    DependentAnd,
    LogicalOr,
    IndependentAnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsSsaBooleanDescriptor {
    pub connector: ImsSsaBoolean,
    pub encoding: u8,
}

include!("generated/ims_ssa_rules.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsSsaCommand {
    pub code: u8,
    pub subset_pointer: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImsSsaField {
    Named(String),
    Offset { position: u16, length: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsSsaPredicate {
    pub field: ImsSsaField,
    pub relation: ImsSsaRelation,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsSsa {
    pub segment: String,
    pub command_format: bool,
    pub commands: Vec<ImsSsaCommand>,
    pub concatenated_key: Option<Vec<u8>>,
    pub predicates: Vec<ImsSsaPredicate>,
    pub connectors: Vec<ImsSsaBoolean>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsSsaLimits {
    pub max_bytes: usize,
    pub max_command_codes: usize,
    pub max_predicates: usize,
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
pub enum ImsSsaProblem {
    TooShort,
    ResourceExhausted,
    InvalidSegmentName,
    InvalidControlByte,
    InvalidCommandCode,
    InvalidSubsetPointer,
    MissingCommandTerminator,
    CommandQualificationConflict,
    UnknownConcatenatedKey,
    InvalidFieldName,
    UnknownField,
    InvalidOffset,
    InvalidRelation,
    InvalidBooleanConnector,
    ParenthesisInValue,
    Truncated,
    TrailingData,
}

pub trait ImsSsaFieldResolver {
    fn field_length(&self, segment: &str, field: &str) -> Option<usize>;

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
pub fn ims_ssa_command_code(code: u8) -> Option<&'static ImsSsaCommandCodeDescriptor> {
    IMS_SSA_COMMAND_CODES
        .iter()
        .find(|descriptor| descriptor.code == code)
}

#[must_use]
pub fn ims_ssa_relation(bytes: [u8; 2]) -> Option<ImsSsaRelation> {
    IMS_SSA_RELATIONAL_OPERATORS
        .iter()
        .find(|descriptor| descriptor.encodings.contains(&bytes))
        .map(|descriptor| descriptor.relation)
}

#[must_use]
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
                if commands.len() >= limits.max_command_codes {
                    return Err(ImsSsaProblem::ResourceExhausted);
                }
                let code = input[cursor];
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
