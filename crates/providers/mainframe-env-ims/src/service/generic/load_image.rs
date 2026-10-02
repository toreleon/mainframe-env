//! Adapt the existing two-level bulk job contract to metadata-selected records.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TwoLevelImage {
    database: String,
    roots: Vec<TwoLevelRoot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::{
        IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsSegmentMetadata,
    };

    fn state(segments: &[(&str, Option<&str>)]) -> State {
        State {
            metadata: Some(ImsMetadataCatalog {
                schema_version: IMS_METADATA_SCHEMA_V1.into(),
                databases: vec![ImsDatabaseMetadata {
                    name: "RENAMED".into(),
                    version: 1,
                    organization: ImsDatabaseOrganization::Hidam,
                    segments: segments
                        .iter()
                        .map(|(name, parent)| ImsSegmentMetadata {
                            name: (*name).into(),
                            parent: parent.map(str::to_string),
                            min_length: 1,
                            max_length: 1,
                            fields: Vec::new(),
                        })
                        .collect(),
                    secondary_indexes: Vec::new(),
                    logical_relationships: Vec::new(),
                }],
                psbs: Vec::new(),
            }),
            ..State::default()
        }
    }

    #[test]
    fn two_level_job_image_resolves_renamed_metadata_and_parent_indexes() {
        let state = state(&[("TOP", None), ("DETAIL", Some("TOP"))]);
        let bytes = br#"{"database":"renamed","roots":[{"data":[1],"children":[[2],[3]]},{"data":[4],"children":[[5]]}]}"#;
        assert_eq!(
            decode(&state, bytes).unwrap().records,
            vec![
                ImsGenericLoadRecord {
                    segment: "TOP".into(),
                    parent: None,
                    data: vec![1]
                },
                ImsGenericLoadRecord {
                    segment: "DETAIL".into(),
                    parent: Some(0),
                    data: vec![2]
                },
                ImsGenericLoadRecord {
                    segment: "DETAIL".into(),
                    parent: Some(0),
                    data: vec![3]
                },
                ImsGenericLoadRecord {
                    segment: "TOP".into(),
                    parent: None,
                    data: vec![4]
                },
                ImsGenericLoadRecord {
                    segment: "DETAIL".into(),
                    parent: Some(3),
                    data: vec![5]
                },
            ]
        );
    }

    #[test]
    fn ambiguous_or_unrepresentable_job_images_fail_closed() {
        let bytes = br#"{"database":"RENAMED","roots":[{"data":[1],"children":[[2]]}]}"#;
        for segments in [
            vec![("TOP", None), ("A", Some("TOP")), ("B", Some("TOP"))],
            vec![("TOP", None), ("A", Some("TOP")), ("B", Some("A"))],
            vec![("TOP", None), ("OTHER", None)],
        ] {
            assert_eq!(
                decode(&state(&segments), bytes),
                Err(HostProblem::Unsupported)
            );
        }
        assert_eq!(
            decode(&state(&[("TOP", None)]), bytes),
            Err(HostProblem::Malformed)
        );
        assert_eq!(decode(&State::default(), bytes), Err(HostProblem::NotFound));
    }

    #[test]
    fn explicit_record_wire_is_unchanged_and_mixed_shapes_are_rejected() {
        let state = state(&[("TOP", None)]);
        let explicit = ImsGenericLoadImage {
            database: "RENAMED".into(),
            records: vec![ImsGenericLoadRecord {
                segment: "TOP".into(),
                parent: None,
                data: vec![7],
            }],
        };
        assert_eq!(
            decode(&state, &serde_json::to_vec(&explicit).unwrap()),
            Ok(explicit)
        );
        for bytes in [
            br#"{"database":"RENAMED","records":[],"roots":[]}"#.as_slice(),
            br#"{"database":"RENAMED","roots":[{"data":[1],"children":[],"ignored":1}]}"#,
            br#"{"database":"RENAMED","roots":[],"ignored":1}"#,
        ] {
            assert_eq!(decode(&state, bytes), Err(HostProblem::Malformed));
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TwoLevelRoot {
    data: Vec<u8>,
    children: Vec<Vec<u8>>,
}

pub(super) fn decode(state: &State, bytes: &[u8]) -> Result<ImsGenericLoadImage, HostProblem> {
    if let Ok(image) = serde_json::from_slice::<ImsGenericLoadImage>(bytes) {
        return Ok(image);
    }
    let image: TwoLevelImage = serde_json::from_slice(bytes).map_err(|_| HostProblem::Malformed)?;
    let database = state
        .metadata
        .as_ref()
        .and_then(|catalog| {
            catalog
                .databases
                .iter()
                .find(|db| normalize(&db.name) == normalize(&image.database))
        })
        .ok_or(HostProblem::NotFound)?;
    let roots = database
        .segments
        .iter()
        .filter(|segment| segment.parent.is_none())
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(HostProblem::Unsupported);
    };
    let children = database
        .segments
        .iter()
        .filter(|segment| segment.parent.as_deref() == Some(root.name.as_str()))
        .collect::<Vec<_>>();
    // The old wire shape has no segment identities. Never guess among siblings
    // or discard levels that cannot be represented by that shape.
    if children.len() > 1 || database.segments.len() != 1 + children.len() {
        return Err(HostProblem::Unsupported);
    }
    let mut records = Vec::new();
    for entry in image.roots {
        let parent = records.len();
        records.push(ImsGenericLoadRecord {
            segment: root.name.clone(),
            parent: None,
            data: entry.data,
        });
        for data in entry.children {
            let child = children.first().ok_or(HostProblem::Malformed)?;
            records.push(ImsGenericLoadRecord {
                segment: child.name.clone(),
                parent: Some(parent),
                data,
            });
        }
    }
    Ok(ImsGenericLoadImage {
        database: image.database,
        records,
    })
}
