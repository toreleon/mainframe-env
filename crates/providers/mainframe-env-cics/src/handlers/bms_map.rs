#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmsMapDefinition {
    pub mapset: String,
    pub map: String,
    /// One-based terminal line at which the map is positioned.
    pub line: u16,
    /// One-based terminal column at which the map is positioned.
    pub column: u16,
    pub rows: u16,
    pub columns: u16,
    pub fields: Vec<BmsFieldDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmsFieldDefinition {
    pub name: String,
    pub row: u16,
    pub column: u16,
    pub length: u16,
    pub initial: Vec<u8>,
    pub color: Option<String>,
    pub highlight: Option<String>,
    pub protected: bool,
    pub secret: bool,
    pub fset: bool,
    pub justify_right: bool,
    pub fill_zero: bool,
    pub output_offset: Option<u32>,
    pub attribute_offset: Option<u32>,
}

use super::super::{CicsLimits, Session};
use mainframe_env_host_api::HostProblem;

pub(in crate::service) fn terminal_field_address(
    session: &Session,
    map: &BmsMapDefinition,
    field: &BmsFieldDefinition,
) -> Result<u16, HostProblem> {
    let row = map
        .line
        .checked_sub(1)
        .and_then(|origin| origin.checked_add(field.row.checked_sub(1)?))
        .ok_or(HostProblem::Malformed)?;
    let column = map
        .column
        .checked_sub(1)
        .and_then(|origin| origin.checked_add(field.column.checked_sub(1)?))
        .ok_or(HostProblem::Malformed)?;
    row.checked_mul(session.columns)
        .and_then(|value| value.checked_add(column))
        .filter(|value| *value < session.rows.saturating_mul(session.columns))
        .ok_or(HostProblem::Malformed)
}

pub(super) fn map_fits_terminal(session: &Session, map: &BmsMapDefinition) -> bool {
    map.rows
        .checked_sub(1)
        .and_then(|extent| map.line.checked_add(extent))
        .is_some_and(|last| last <= session.rows)
        && map
            .columns
            .checked_sub(1)
            .and_then(|extent| map.column.checked_add(extent))
            .is_some_and(|last| last <= session.columns)
}

pub(in crate::service) fn encode_terminal_address(address: u16) -> Result<[u8; 2], HostProblem> {
    if address > 0x3fff {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok([(address >> 8) as u8, address as u8])
}

pub(in crate::service) fn decode_terminal_address(
    first: u8,
    second: u8,
) -> Result<u16, HostProblem> {
    if first & 0xc0 != 0 {
        return Err(HostProblem::Unsupported);
    }
    Ok((u16::from(first) << 8) | u16::from(second))
}

pub(in crate::service) fn validate_map(
    map: &BmsMapDefinition,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    if map.mapset.is_empty()
        || map.map.is_empty()
        || map.line == 0
        || map.column == 0
        || map.rows == 0
        || map.columns == 0
        || map.fields.len() > limits.max_fields
    {
        return Err(HostProblem::Malformed);
    }
    if map.line.checked_add(map.rows - 1).is_none()
        || map.column.checked_add(map.columns - 1).is_none()
    {
        return Err(HostProblem::Malformed);
    }
    for field in &map.fields {
        if field.name.is_empty()
            || field.length == 0
            || field.row == 0
            || field.column == 0
            || field.row > map.rows
            || field.column > map.columns
            || field.initial.len() > usize::from(field.length)
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}
