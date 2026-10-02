use super::*;

pub(super) fn validate_definition(
    definition: &DatabaseDefinition,
    limits: EngineLimits,
) -> Result<(), EngineProblem> {
    if let Some(format) = &definition.gsam_format {
        let record = definition
            .segments
            .first()
            .ok_or(EngineProblem::InvalidDefinition)?;
        if definition.organization != DatabaseOrganization::Gsam
            || definition.segments.len() != 1
            || !record.fields.is_empty()
            || format
                .validate(record.min_length, record.max_length)
                .is_err()
        {
            return Err(EngineProblem::InvalidDefinition);
        }
    }
    if !valid_name(&definition.name, limits.max_name_bytes)
        || definition.segments.is_empty()
        || definition.segments.len() > limits.max_segments
        || definition.secondary_indexes.len() > limits.max_secondary_indexes
    {
        return Err(EngineProblem::InvalidDefinition);
    }
    let names = definition
        .segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect::<BTreeSet<_>>();
    if names.len() != definition.segments.len() {
        return Err(EngineProblem::InvalidDefinition);
    }
    for segment in &definition.segments {
        if !valid_name(&segment.name, limits.max_name_bytes)
            || segment.min_length == 0
            || segment.min_length > segment.max_length
            || segment.max_length > limits.max_segment_bytes
            || segment.fields.len() > limits.max_fields_per_segment
            || segment
                .parent
                .as_ref()
                .is_some_and(|parent| !names.contains(parent.as_str()))
        {
            return Err(EngineProblem::InvalidDefinition);
        }
        let mut fields = BTreeSet::new();
        for field in &segment.fields {
            let end = field.offset.checked_add(field.length);
            if !valid_name(&field.name, limits.max_name_bytes)
                || field.length == 0
                || !fields.insert(field.name.as_str())
                || end.is_none_or(|end| end > segment.max_length)
            {
                return Err(EngineProblem::InvalidDefinition);
            }
        }
        if let Some(key) = &segment.key_field {
            let field = segment
                .fields
                .iter()
                .find(|field| &field.name == key)
                .ok_or(EngineProblem::InvalidDefinition)?;
            if field.offset + field.length > segment.min_length {
                return Err(EngineProblem::InvalidDefinition);
            }
        }
        let mut current = segment.parent.as_deref();
        let mut depth = 0;
        while let Some(parent) = current {
            if parent == segment.name || depth >= definition.segments.len() {
                return Err(EngineProblem::InvalidDefinition);
            }
            current = definition
                .segments
                .iter()
                .find(|candidate| candidate.name == parent)
                .and_then(|candidate| candidate.parent.as_deref());
            depth += 1;
        }
    }
    if definition
        .segments
        .iter()
        .filter(|segment| segment.parent.is_none())
        .count()
        != 1
    {
        return Err(EngineProblem::InvalidDefinition);
    }
    let index_names = definition
        .secondary_indexes
        .iter()
        .map(|index| index.name.as_str())
        .collect::<BTreeSet<_>>();
    if index_names.len() != definition.secondary_indexes.len() {
        return Err(EngineProblem::InvalidDefinition);
    }
    for index in &definition.secondary_indexes {
        let Some(segment) = definition
            .segments
            .iter()
            .find(|segment| segment.name == index.source_segment)
        else {
            return Err(EngineProblem::InvalidDefinition);
        };
        let target = definition
            .segments
            .iter()
            .find(|segment| segment.name == index.target_segment())
            .ok_or(EngineProblem::InvalidDefinition)?;
        let mut ancestor = Some(segment);
        while ancestor.is_some_and(|segment| segment.name != target.name) {
            ancestor = ancestor
                .and_then(|segment| segment.parent.as_deref())
                .and_then(|parent| {
                    definition
                        .segments
                        .iter()
                        .find(|segment| segment.name == parent)
                });
        }
        let mut fields = BTreeSet::new();
        let mut length = 0usize;
        for name in index.fields() {
            let field = segment
                .fields
                .iter()
                .find(|field| field.name == name)
                .ok_or(EngineProblem::InvalidDefinition)?;
            if !fields.insert(name) {
                return Err(EngineProblem::InvalidDefinition);
            }
            length = length
                .checked_add(field.length)
                .ok_or(EngineProblem::InvalidDefinition)?;
        }
        if !valid_name(&index.name, limits.max_name_bytes)
            || ancestor.is_none()
            || fields.len() > limits.max_fields_per_segment.min(5)
            || length > limits.max_segment_bytes.min(240)
            || target.fields.iter().any(|field| field.name == index.name)
        {
            return Err(EngineProblem::InvalidDefinition);
        }
    }
    match definition.organization {
        DatabaseOrganization::Gsam
            if definition.segments.len() != 1
                || definition.segments[0].key_field.is_some()
                || !definition.secondary_indexes.is_empty() =>
        {
            Err(EngineProblem::InvalidDefinition)
        }
        DatabaseOrganization::Msdb
        | DatabaseOrganization::Shisam
        | DatabaseOrganization::Shsam
        | DatabaseOrganization::Index
        | DatabaseOrganization::Psindex
            if definition.segments.len() != 1 =>
        {
            Err(EngineProblem::InvalidDefinition)
        }
        DatabaseOrganization::Shsam | DatabaseOrganization::Shisam
            if definition.segments[0].min_length != definition.segments[0].max_length =>
        {
            Err(EngineProblem::InvalidDefinition)
        }
        DatabaseOrganization::Hdam
        | DatabaseOrganization::Hidam
        | DatabaseOrganization::Hisam
        | DatabaseOrganization::Shisam
        | DatabaseOrganization::Phidam
        | DatabaseOrganization::Dedb
        | DatabaseOrganization::Index
        | DatabaseOrganization::Msdb
            if definition.segments[0].key_field.is_none() =>
        {
            Err(EngineProblem::InvalidDefinition)
        }
        DatabaseOrganization::Gsam
        | DatabaseOrganization::Hsam
        | DatabaseOrganization::Index
        | DatabaseOrganization::Msdb
        | DatabaseOrganization::Psindex
        | DatabaseOrganization::Shisam
        | DatabaseOrganization::Shsam
            if !definition.secondary_indexes.is_empty() =>
        {
            Err(EngineProblem::InvalidDefinition)
        }
        _ => Ok(()),
    }
}

fn valid_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
