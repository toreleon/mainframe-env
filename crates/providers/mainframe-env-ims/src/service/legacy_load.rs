//! Legacy image load/unload on the existing state and row-publication authority.
use super::*;

pub(super) fn load(
    state: &mut State,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let image: ImsLoadImage =
        serde_json::from_slice(&request.data).map_err(|_| HostProblem::Malformed)?;
    let database_name = normalize(&image.database);
    system::reservations::ensure_no_reservations(state, &database_name)?;
    let (_, root_definition, child_definition) = database_definition(state, &database_name)?;
    let child_definition = child_definition.ok_or(HostProblem::NotFound)?;
    if image.roots.len() > limits.max_roots {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut database = DatabaseState::default();
    let mut affected_count = 0u64;
    for root in image.roots {
        validate_segment_data(&root.data, &root_definition, limits)?;
        if root.children.len() > limits.max_children_per_root {
            return Err(HostProblem::ResourceExhausted);
        }
        let root_key = data_key(&root.data, &root_definition)?;
        let mut record = RootRecord {
            data: root.data,
            children: BTreeMap::new(),
        };
        for child in root.children {
            validate_segment_data(&child, &child_definition, limits)?;
            let child_key = data_key(&child, &child_definition)?;
            if record.children.insert(child_key, child).is_some() {
                return Err(HostProblem::Malformed);
            }
            affected_count += 1;
        }
        if database.roots.insert(root_key.clone(), record).is_some() {
            return Err(HostProblem::Malformed);
        }
        database.secondary_index.insert(root_key.clone(), root_key);
        affected_count += 1;
    }
    state.databases.insert(database_name, Arc::new(database));
    Ok(affected(affected_count))
}

pub(super) fn unload(
    state: &State,
    run: &str,
    request: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let database_name = request
        .psb
        .as_ref()
        .map(|name| normalize(name))
        .or_else(|| context(state, run).ok().map(|context| context.0))
        .ok_or(HostProblem::Malformed)?;
    let (_, root_definition, child_definition) = database_definition(state, &database_name)?;
    let database = state
        .databases
        .get(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let mut segments = Vec::new();
    for (root_key, root) in &database.roots {
        segments.push(ImsSegment {
            name: root_definition.name.clone(),
            parent_key: None,
            data: root.data.clone(),
        });
        if let Some(child_definition) = &child_definition {
            for child in root.children.values() {
                segments.push(ImsSegment {
                    name: child_definition.name.clone(),
                    parent_key: Some(decode_key(root_key)?),
                    data: child.clone(),
                });
            }
        }
        if segments.len() >= request.max_segments as usize {
            break;
        }
    }
    Ok(ImsResult {
        status: "  ".into(),
        segments,
        checkpoint_id: None,
        affected_segments: 0,
        system: None,
    })
}
