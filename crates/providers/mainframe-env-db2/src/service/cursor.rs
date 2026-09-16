use super::*;

pub(super) fn open_cursor(
    state: &mut State,
    run: &str,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    if state.cursors.len() >= limits.max_cursors {
        return Err(HostProblem::ResourceExhausted);
    }
    let cursor_name = request.cursor.as_deref().ok_or(HostProblem::Malformed)?;
    if !state
        .cursor_declarations
        .contains_key(&cursor_key(run, cursor_name))
        && state.cursor_declarations.len() >= limits.max_cursors
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let statement = if request.statement.to_ascii_uppercase().contains(" FROM ") {
        request.statement.clone()
    } else {
        state
            .cursor_declarations
            .get(&cursor_key(run, cursor_name))
            .cloned()
            .ok_or(HostProblem::NotFound)?
    };
    let (_, definition, table) = read_relation(state, run, &statement)?;
    let backward = statement.to_ascii_uppercase().contains(" DESC");
    let columns = selected_column_indices(&statement, definition)?;
    let key_index = *definition
        .primary_key_indices()?
        .first()
        .ok_or(HostProblem::Malformed)?;
    let start = request
        .inputs
        .values()
        .next()
        .map(|value| predicate_operand(&definition.columns[key_index], &value.value))
        .transpose()?;
    let mut ordered = table.rows.values().collect::<Vec<_>>();
    ordered.sort_by(|left, right| compare_primary_key_rows(definition, left, right));
    let mut rows = ordered
        .into_iter()
        .filter(|row| {
            start.as_ref().is_none_or(|start| {
                start.bytes().is_empty()
                    || if backward {
                        row[key_index].as_slice() <= start.bytes()
                    } else {
                        row[key_index].as_slice() >= start.bytes()
                    }
            })
        })
        .map(|row| result_row(definition, row, &columns).map(|row| row.columns))
        .collect::<Result<Vec<_>, _>>()?;
    if backward {
        rows.reverse();
    }
    state
        .cursor_declarations
        .insert(cursor_key(run, cursor_name), statement);
    state.cursors.insert(
        cursor_key(run, cursor_name),
        Arc::new(Cursor { rows, index: 0 }),
    );
    Ok(success(0, "CURSOR OPEN", Vec::new()))
}
