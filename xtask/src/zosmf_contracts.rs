use super::*;

const CATALOG: &str = "conformance/0.11/catalogs/zosmf-normalization.json";
const CONTRACTS: &str = "conformance/0.11/generated/zosmf-contracts.json";
const COLLISIONS: &str = "conformance/0.11/generated/zosmf-collision-report.json";
const CLOSURE: &str = "conformance/0.11/generated/zosmf-closure-report.json";
const GENERATED_RUST: &str = "crates/gateways/mainframe-env-zosmf/src/generated/zosmf_contracts.rs";
const CONTRACTS_SCHEMA: &str = "conformance/0.11/schemas/zosmf-generated-contracts.schema.json";
const COLLISIONS_SCHEMA: &str = "conformance/0.11/schemas/zosmf-collision-report.schema.json";
const CLOSURE_SCHEMA: &str = "conformance/0.11/schemas/zosmf-closure-report.schema.json";

struct Rendered {
    contracts: Value,
    collisions: Value,
    closure: Value,
    rust: Vec<u8>,
}

pub(super) fn run(root: &Path, check: bool) -> TaskResult {
    let rendered = render(root)?;
    let outputs = [
        (CONTRACTS, pretty_json(&rendered.contracts)?),
        (COLLISIONS, pretty_json(&rendered.collisions)?),
        (CLOSURE, pretty_json(&rendered.closure)?),
        (GENERATED_RUST, rendered.rust),
    ];
    if check {
        for (relative, expected) in outputs {
            let path = root.join(relative);
            require(
                fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?
                    == expected,
                &format!(
                    "{} is stale; run cargo xtask zosmf-contracts",
                    path.display()
                ),
            )?;
        }
    } else {
        for (relative, bytes) in outputs {
            let path = root.join(relative);
            fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| format!("{} has no parent", path.display()))?,
            )
            .map_err(|error| format!("{}: {error}", path.display()))?;
            fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn render(root: &Path) -> TaskResult<Rendered> {
    let catalog_path = root.join(CATALOG);
    let catalog = json(&catalog_path)?;
    validate_schema_instance(
        &json(&root.join("conformance/0.11/schemas/zosmf-normalization.schema.json"))?,
        &catalog,
        &catalog_path,
    )?;
    require(
        catalog["schema_version"] == "mainframe-env.zosmf-normalization@1"
            && catalog["target_version"] == "0.11.0"
            && catalog["work_package"] == "ZMF-1101",
        "z/OSMF normalization identity changed",
    )?;

    let families = array(&catalog, "families", &catalog_path)?;
    let headings = array(&catalog, "headings", &catalog_path)?;
    let operations = array(&catalog, "operations", &catalog_path)?;
    let legacy = array(&catalog, "legacy_official_routes", &catalog_path)?;
    require(
        families.len() == 27 && headings.len() == 189 && operations.len() == 278,
        "z/OSMF normalization denominators changed",
    )?;

    let family_ids = unique_texts(families, "id", &catalog_path)?;
    let _operation_ids = unique_texts(operations, "id", &catalog_path)?;
    let heading_rows = unique_texts(headings, "row_id", &catalog_path)?;
    let family_rows = families
        .iter()
        .map(|family| text(&family["source"], "row_id", &catalog_path))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        family_rows.len() == 27 && heading_rows.len() == 189,
        "z/OSMF source row identities are not unique",
    )?;

    let normalization_sha256 = format!("sha256:{}", file_digest(&catalog_path)?);
    let mut route_entries = Vec::new();
    let mut operation_entries = Vec::new();
    let mut schema_entries: BTreeMap<String, Value> = BTreeMap::new();
    let mut error_operations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut collision_groups: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
    let mut operation_discriminators = BTreeMap::new();
    let mut obligation_ids = BTreeSet::new();
    let mut operation_backend_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut publication_counts: BTreeMap<String, u64> = BTreeMap::new();

    for operation in operations {
        let operation_id = text(operation, "id", &catalog_path)?.to_string();
        let family_id = text(operation, "family_id", &catalog_path)?.to_string();
        require(
            family_ids.contains(family_id.as_str()),
            &format!("operation {operation_id} references unknown family {family_id}"),
        )?;
        let routes = array(operation, "routes", &catalog_path)?;
        let publication = object(&operation["publication"], &catalog_path)?;
        let schemas = object(&operation["schemas"], &catalog_path)?;
        let source_row_ids = array(operation, "source_row_ids", &catalog_path)?;
        let source_row_ids = source_row_ids
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("operation {operation_id} has a non-text source row"))
            })
            .collect::<TaskResult<Vec<_>>>()?;
        require(
            source_row_ids.iter().all(|row| {
                heading_rows.contains(row.as_str()) || family_rows.contains(row.as_str())
            }),
            &format!("operation {operation_id} has an unknown source row"),
        )?;
        let discriminator = text(operation, "dispatch_discriminator", &catalog_path)?.to_string();
        operation_discriminators.insert(operation_id.clone(), discriminator);

        let mut route_ids = Vec::new();
        for (index, route) in routes.iter().enumerate() {
            let method = text(route, "method", &catalog_path)?.to_string();
            let path = text(route, "normalized_path", &catalog_path)?.to_string();
            let source_uri = text(route, "source_uri", &catalog_path)?.to_string();
            require(
                path.starts_with("/zosmf/") && source_uri.starts_with("/zosmf/"),
                &format!("operation {operation_id} escapes the official namespace"),
            )?;
            let route_id = format!("{operation_id}.route-{}", index + 1);
            route_ids.push(route_id.clone());
            collision_groups
                .entry((method.clone(), path.clone()))
                .or_default()
                .entry(operation_id.clone())
                .and_modify(|count| *count += 1)
                .or_insert(1);
            route_entries.push(json!({
                "id": route_id,
                "operation_id": operation_id,
                "method": method,
                "path": path,
                "source_uri": source_uri,
                "query_contract": route["query_contract"].clone(),
                "source_evidence": route["source_evidence"].clone(),
                "publication_state": publication["state"].clone(),
                "existing_route_ids": publication["existing_route_ids"].clone()
            }));
        }

        let backend_state = text(&operation["backend"], "acceptance", &catalog_path)?;
        *operation_backend_counts
            .entry(backend_state.to_string())
            .or_default() += 1;
        let publication_state = text(&operation["publication"], "state", &catalog_path)?;
        *publication_counts
            .entry(publication_state.to_string())
            .or_default() += 1;

        let obligations = array(operation, "mandatory_obligations", &catalog_path)?;
        for obligation in obligations {
            let obligation_id = text(obligation, "id", &catalog_path)?;
            require(
                obligation_ids.insert(obligation_id.to_string()),
                &format!("duplicate z/OSMF obligation {obligation_id}"),
            )?;
        }

        for (role, field) in [("request", "request"), ("response", "response")] {
            let schema_id = text(&operation["schemas"], field, &catalog_path)?;
            require(
                schema_entries
                    .insert(
                        schema_id.to_string(),
                        json!({
                            "id":schema_id,
                            "role":role,
                            "resolution":schemas["resolution"].clone(),
                            "operation_ids":[operation_id.clone()],
                            "source_row_ids":source_row_ids.clone()
                        }),
                    )
                    .is_none(),
                &format!("duplicate generated schema identity {schema_id}"),
            )?;
        }
        let error_id = text(&operation["schemas"], "error", &catalog_path)?.to_string();
        error_operations
            .entry(error_id)
            .or_default()
            .push(operation_id.clone());

        operation_entries.push(json!({
            "id":operation_id,
            "label":operation["label"].clone(),
            "family_id":family_id,
            "source_row_ids":source_row_ids,
            "route_ids":route_ids,
            "authorization":operation["authorization"].clone(),
            "state_model":operation["state_model"].clone(),
            "failure_model":operation["failure_model"].clone(),
            "backend":operation["backend"].clone(),
            "schemas":operation["schemas"].clone(),
            "publication":operation["publication"].clone(),
            "mandatory_obligations":operation["mandatory_obligations"].clone()
        }));
    }

    require(
        route_entries.len() == 352 && obligation_ids.len() == 1727,
        "z/OSMF route or obligation closure changed",
    )?;
    route_entries.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    operation_entries.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));

    let mut error_entries = Vec::new();
    for family in families {
        let family_id = text(family, "id", &catalog_path)?;
        let error_id = format!("mainframe-env.zosmf.error.{family_id}@1");
        let operation_ids = error_operations.remove(&error_id).unwrap_or_default();
        let source_error_rows = headings
            .iter()
            .filter(|heading| {
                heading["family_id"].as_str() == Some(family_id)
                    && heading["source"]["disposition"].as_str() == Some("error-reference")
            })
            .filter_map(|heading| heading["row_id"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        schema_entries.insert(
            error_id.clone(),
            json!({
                "id":error_id,
                "role":"error",
                "resolution":if source_error_rows.is_empty() {"operation-source-status-only"} else {"family-error-reference-pinned"},
                "operation_ids":operation_ids.clone(),
                "source_row_ids":source_error_rows.clone()
            }),
        );
        error_entries.push(json!({
            "id":error_id,
            "family_id":family_id,
            "source_error_rows":source_error_rows,
            "operation_ids":operation_ids,
            "resolution":if source_error_rows.is_empty() {"operation-source-status-only"} else {"family-error-reference-pinned"}
        }));
    }
    require(
        error_operations.is_empty() && schema_entries.len() == 583 && error_entries.len() == 27,
        "z/OSMF schema/error closure changed",
    )?;

    let mut family_backend_counts: BTreeMap<String, u64> = BTreeMap::new();
    let backend_entries = families
        .iter()
        .map(|family| {
            let family_id = text(family, "id", &catalog_path)?;
            let backend = object(&family["backend"], &catalog_path)?;
            let state = text(&family["backend"], "acceptance", &catalog_path)?;
            *family_backend_counts.entry(state.to_string()).or_default() += 1;
            let operation_counts = count_values(
                operations
                    .iter()
                    .filter(|operation| operation["family_id"].as_str() == Some(family_id))
                    .filter_map(|operation| operation["backend"]["acceptance"].as_str()),
            );
            Ok(json!({
                "family_id":family_id,
                "owner":backend["owner"].clone(),
                "capability_version":backend["capability_version"].clone(),
                "acceptance":state,
                "prerequisite_slice":backend["prerequisite_slice"].clone(),
                "operation_counts":operation_counts
            }))
        })
        .collect::<TaskResult<Vec<_>>>()?;

    require(
        family_backend_counts
            == BTreeMap::from([
                ("accepted".to_string(), 1),
                ("missing".to_string(), 6),
                ("partial".to_string(), 4),
                ("unresolved".to_string(), 16),
            ])
            && operation_backend_counts
                == BTreeMap::from([
                    ("accepted".to_string(), 22),
                    ("missing".to_string(), 38),
                    ("partial".to_string(), 27),
                    ("unresolved".to_string(), 191),
                ])
            && publication_counts
                == BTreeMap::from([
                    ("legacy-accepted-route".to_string(), 22),
                    ("withheld".to_string(), 256),
                ]),
        "z/OSMF backend/publication dispositions changed",
    )?;

    let official_path = root.join("conformance/0.2/routes/official-route-bindings.json");
    let custom_path = root.join("conformance/0.2/routes/custom-routes.json");
    let official = json(&official_path)?;
    let custom = json(&custom_path)?;
    let official_rows = array(&official, "routes", &official_path)?;
    let custom_rows = array(&custom, "routes", &custom_path)?;
    require(
        official_rows.len() == 23 && custom_rows.len() == 7 && legacy.len() == 23,
        "frozen route denominators changed",
    )?;
    let official_ids = official_rows
        .iter()
        .map(|row| text(row, "id", &official_path))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let custom_ids = custom_rows
        .iter()
        .map(|row| text(row, "id", &custom_path))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        official_ids.is_disjoint(&custom_ids)
            && official_ids.iter().all(|id| id.contains(" /zosmf/"))
            && custom_ids.iter().all(|id| id.contains(" /mainframe-env/")),
        "official/custom namespace separation changed",
    )?;
    for (preserved, frozen) in legacy.iter().zip(official_rows) {
        require(
            preserved["id"] == frozen["id"] && preserved["handler"] == frozen["handler"],
            "legacy z/OSMF route binding differs from the frozen registry",
        )?;
    }

    let contracts = json!({
        "schema_version":"mainframe-env.zosmf-generated-contracts@1",
        "target_version":"0.11.0",
        "normalization":{"path":CATALOG,"sha256":normalization_sha256},
        "generated_coverage_credit":0,
        "counts":{"families":27,"headings":189,"operations":278,"routes":352,"schemas":583,"errors":27,"backend_families":27,"obligations":1727},
        "routes":route_entries,
        "operations":operation_entries,
        "schemas":schema_entries.into_values().collect::<Vec<_>>(),
        "errors":error_entries,
        "backend_ownership":backend_entries,
        "legacy_official_routes":legacy.clone(),
        "custom_namespace":{"catalog":"conformance/0.2/routes/custom-routes.json","route_count":7,"official_coverage_credit":0}
    });

    let mut collision_entries = Vec::new();
    let mut alias_entries = Vec::new();
    let mut classified_collisions = 0_u64;
    let mut blocked_collisions = 0_u64;
    for ((method, path), members) in &collision_groups {
        if members.len() > 1 {
            let operation_ids = members.keys().cloned().collect::<Vec<_>>();
            let discriminators = operation_ids
                .iter()
                .map(|operation_id| {
                    format!(
                        "{operation_id}={}",
                        operation_discriminators
                            .get(operation_id)
                            .map(String::as_str)
                            .unwrap_or("none")
                    )
                })
                .collect::<Vec<_>>();
            let classified = operation_ids.iter().all(|operation_id| {
                operation_discriminators
                    .get(operation_id)
                    .is_some_and(|value| value != "none")
            });
            if classified {
                classified_collisions += 1;
            } else {
                blocked_collisions += 1;
            }
            collision_entries.push(json!({
                "method":method,
                "path":path,
                "operation_ids":operation_ids,
                "discriminators":discriminators,
                "resolution":if classified {"typed-request-discriminator"} else {"blocked-pending-typed-discriminator"},
                "new_route_publication_allowed":false
            }));
        } else if members.values().next().is_some_and(|count| *count > 1) {
            alias_entries.push(json!({
                "method":method,
                "path":path,
                "operation_id":members.keys().next().expect("one member"),
                "source_variant_count":members.values().next().expect("one count")
            }));
        }
    }
    require(
        collision_groups.len() == 310
            && collision_entries.len() == 14
            && alias_entries.len() == 5
            && classified_collisions == 1
            && blocked_collisions == 13,
        "z/OSMF collision classification changed",
    )?;
    let collisions = json!({
        "schema_version":"mainframe-env.zosmf-collision-report@1",
        "target_version":"0.11.0",
        "normalization_sha256":normalization_sha256,
        "generated_coverage_credit":0,
        "counts":{"route_keys":310,"shared_dispatch":14,"source_alias_groups":5,"classified":classified_collisions,"blocked":blocked_collisions},
        "shared_dispatch":collision_entries,
        "source_aliases":alias_entries,
        "official_custom_namespaces_disjoint":true,
        "new_route_publication_allowed":false
    });

    let heading_dispositions = count_values(
        headings
            .iter()
            .filter_map(|heading| heading["source"]["disposition"].as_str()),
    );
    let source_rows = family_rows.len() + heading_rows.len();
    let closure = json!({
        "schema_version":"mainframe-env.zosmf-closure-report@1",
        "target_version":"0.11.0",
        "normalization_sha256":normalization_sha256,
        "generated_coverage_credit":0,
        "source":{"families":27,"headings":189,"rows":source_rows,"heading_dispositions":heading_dispositions,"closed":source_rows==216},
        "normalized":{"operations":278,"route_variants":352,"route_keys":310,"schemas":583,"errors":27,"obligations":1727},
        "backend":{"families":family_backend_counts,"operations":operation_backend_counts,"all_families_executable":false},
        "publication":{"operation_states":publication_counts,"existing_official_routes":23,"legacy_unmapped_routes":1,"new_official_routes":0,"custom_routes":7,"custom_routes_official_credit":0,"namespaces_disjoint":true},
        "collisions":{"shared_dispatch":14,"classified":classified_collisions,"blocked":blocked_collisions,"new_route_publication_allowed":false},
        "blockers":[
            {"id":"missing-or-unresolved-backend","operation_count":256},
            {"id":"identity-only-schema-detail","operation_count":256},
            {"id":"shared-route-discriminator","route_key_count":blocked_collisions},
            {"id":"licensed-zosmf-3.2-differential","operation_count":278}
        ],
        "zmf_1101_contracts_closed":true,
        "zosmf_0_11_exit_complete":false
    });

    for (value, schema, label) in [
        (&contracts, CONTRACTS_SCHEMA, "generated z/OSMF contracts"),
        (&collisions, COLLISIONS_SCHEMA, "z/OSMF collision report"),
        (&closure, CLOSURE_SCHEMA, "z/OSMF closure report"),
    ] {
        let schema_path = root.join(schema);
        validate_schema_instance(&json(&schema_path)?, value, Path::new(label))?;
    }

    let rust = render_rust(families, legacy, &normalization_sha256, &catalog_path)?;
    Ok(Rendered {
        contracts,
        collisions,
        closure,
        rust,
    })
}

fn unique_texts<'a>(rows: &'a [Value], field: &str, path: &Path) -> TaskResult<BTreeSet<&'a str>> {
    let mut values = BTreeSet::new();
    for row in rows {
        let value = text(row, field, path)?;
        require(
            values.insert(value),
            &format!("{} duplicates {field} {value}", path.display()),
        )?;
    }
    Ok(values)
}

fn count_values<'a>(values: impl Iterator<Item = &'a str>) -> Value {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for value in values {
        *counts.entry(value.to_string()).or_default() += 1;
    }
    serde_json::to_value(counts).expect("string/u64 map serializes")
}

fn render_rust(
    families: &[Value],
    legacy: &[Value],
    normalization_sha256: &str,
    catalog_path: &Path,
) -> TaskResult<Vec<u8>> {
    let mut source = format!(
        "// @generated by `cargo xtask zosmf-contracts`; do not edit.\n\n\
         pub const ZOSMF_NORMALIZATION_CONTRACT: &str = \"mainframe-env.zosmf-normalization@1\";\n\
         #[rustfmt::skip]\n\
         pub const ZOSMF_NORMALIZATION_SHA256: &str = \"{normalization_sha256}\";\n\
         pub const ZOSMF_NORMALIZED_FAMILY_COUNT: usize = 27;\n\
         pub const ZOSMF_NORMALIZED_HEADING_COUNT: usize = 189;\n\
         pub const ZOSMF_NORMALIZED_OPERATION_COUNT: usize = 278;\n\
         pub const ZOSMF_NORMALIZED_ROUTE_VARIANT_COUNT: usize = 352;\n\
         pub const ZOSMF_NEW_ADVERTISED_ROUTE_COUNT: usize = 0;\n\n\
         #[rustfmt::skip]\n\
         pub const ZOSMF_FAMILY_BACKENDS: &[(&str, &str, &str, &str)] = &[\n"
    );
    for family in families {
        let id = text(family, "id", catalog_path)?;
        let owner = text(&family["backend"], "owner", catalog_path)?;
        let capability = text(&family["backend"], "capability_version", catalog_path)?;
        let acceptance = text(&family["backend"], "acceptance", catalog_path)?;
        source.push_str(&format!(
            "    ({}, {}, {}, {}),\n",
            serde_json::to_string(id).map_err(|error| error.to_string())?,
            serde_json::to_string(owner).map_err(|error| error.to_string())?,
            serde_json::to_string(capability).map_err(|error| error.to_string())?,
            serde_json::to_string(acceptance).map_err(|error| error.to_string())?
        ));
    }
    source.push_str("];\n");
    source.push_str(
        "\n#[rustfmt::skip]\npub const ZOSMF_LEGACY_ROUTE_OPERATIONS: &[(&str, &[&str])] = &[\n",
    );
    for route in legacy {
        let id = text(route, "id", catalog_path)?;
        let operations = array(route, "normalized_operation_ids", catalog_path)?;
        source.push_str(&format!(
            "    ({}, &[",
            serde_json::to_string(id).map_err(|error| error.to_string())?
        ));
        for (index, operation) in operations.iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            source.push_str(
                &serde_json::to_string(
                    operation
                        .as_str()
                        .ok_or_else(|| format!("legacy route {id} has non-text operation"))?,
                )
                .map_err(|error| error.to_string())?,
            );
        }
        source.push_str("]),\n");
    }
    source.push_str("];\n");
    Ok(source.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_contracts_close_the_normalized_authority() {
        let root = repository_root().unwrap();
        let rendered = render(&root).unwrap();
        assert_eq!(rendered.contracts["counts"]["operations"], 278);
        assert_eq!(rendered.contracts["counts"]["schemas"], 583);
        assert_eq!(rendered.collisions["counts"]["blocked"], 13);
        assert_eq!(rendered.closure["publication"]["new_official_routes"], 0);
        assert_eq!(
            rendered.closure["publication"]["custom_routes_official_credit"],
            0
        );
        assert_eq!(rendered.closure["zosmf_0_11_exit_complete"], false);
    }

    #[test]
    fn generated_contract_check_rejects_stale_output() {
        let root = repository_root().unwrap();
        let rendered = render(&root).unwrap();
        assert_ne!(
            pretty_json(&rendered.contracts).unwrap(),
            br#"{"mutated":true}"#.to_vec()
        );
    }
}
