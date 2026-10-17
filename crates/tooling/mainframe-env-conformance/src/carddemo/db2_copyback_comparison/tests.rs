//! Synthetic refusals only; never a source execution or storage acceptance receipt.
use super::*;
use mainframe_env_ir::{IrLimits, ModuleBuilder};
struct Fixture {
    data: Vec<Data>,
    slots: Vec<Slot>,
    sites: Vec<usize>,
    rows: Vec<Vec<u8>>,
    fixed: Vec<Vec<u8>>,
}
fn fixture() -> Fixture {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let slots = TARGETS
        .iter()
        .enumerate()
        .map(|(base, (name, width))| {
            let id = builder.add_storage(*name, *width as u64, None).unwrap();
            Slot {
                descriptor: Db2CopybackSlot::new(id, (*name).into(), *width).unwrap(),
                base,
                offset: 0,
                length: *width,
            }
        })
        .collect::<Vec<_>>();
    let rows = (1..=7).map(|index| vec![index; 54]).collect::<Vec<_>>();
    let fixed = vec![
        vec![0; 4],
        vec![0, 0, 0, 100],
        b"00000".to_vec(),
        b"02000".to_vec(),
        vec![0, 0, 0x1c],
    ];
    let sites = vec![10, 20, 30, 30, 30, 30, 30, 30, 30, 40, 50];
    let data = operations()
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            let eof = index == 9;
            let state = if eof { *b"02000" } else { *b"00000" };
            let code = if eof { 100 } else { 0 };
            Data {
                operation,
                sequence: index as u64 + 1,
                pc: sites[index],
                keyed: operation.is_mutating(),
                inputs: usize::from(index == 1) * 5,
                indicators: 0,
                sqlcode: code,
                sqlstate: state,
                affected: 0,
                rows: usize::from(index == 0 || (2..=8).contains(&index)),
                canonical_resource: true,
                cells: slots
                    .iter()
                    .enumerate()
                    .map(|(role, slot)| Cell {
                        id: slot.descriptor.storage().get(),
                        name: slot.descriptor.name().into(),
                        base: slot.base,
                        offset: slot.offset,
                        bytes: match role {
                            0 => fixed[usize::from(eof)].clone(),
                            1 => fixed[2 + usize::from(eof)].clone(),
                            2 => fixed[4].clone(),
                            3 if index >= 2 => rows[(index - 2).min(6)].clone(),
                            3 => vec![0; 54],
                            _ => unreachable!(),
                        },
                    })
                    .collect(),
                implicit: vec![
                    (
                        "SQLCODE",
                        Db2CopybackValue::Decimal {
                            coefficient: i128::from(code),
                            scale: 0,
                        },
                    ),
                    ("SQLSTATE", Db2CopybackValue::Bytes(state.to_vec())),
                ],
            }
        })
        .collect();
    Fixture {
        data,
        slots,
        sites,
        rows,
        fixed,
    }
}
#[test]
fn full_physical_sqlcode_state_packed_and_dcl_varchar_bytes_refuse_every_corruption() {
    let Fixture {
        data,
        slots,
        sites,
        rows,
        fixed,
    } = fixture();
    assert!(compare_data(&data, &slots, &sites, &rows, &fixed).is_ok());
    for (index, record) in data.iter().enumerate() {
        for (role, cell) in record.cells.iter().enumerate() {
            if role == 3 && index < 2 {
                continue;
            }
            for (byte, _) in cell.bytes.iter().enumerate() {
                let mut changed = data.clone();
                changed[index].cells[role].bytes[byte] ^= 1;
                assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
            }
        }
    }
    let mut changed = data;
    changed[9].implicit[0].1 = Db2CopybackValue::Bytes(vec![0, 0, 0, 100]);
    assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
}
#[test]
fn exact_eleven_sql_operations_sites_inputs_indicators_eof_and_close_refuse_drift() {
    let Fixture {
        data,
        slots,
        sites,
        rows,
        fixed,
    } = fixture();
    for field in 0..12 {
        let mut changed = data.clone();
        match field {
            0 => changed[0].sequence = 0,
            1 => changed[1].operation = Db2Operation::Select,
            2 => changed[2].pc += 1,
            3 => changed[0].keyed = true,
            4 => changed[1].inputs = 4,
            5 => changed[2].indicators = 1,
            6 => changed[9].sqlcode = 0,
            7 => changed[9].sqlstate = *b"00000",
            8 => changed[10].sqlcode = 100,
            9 => changed[2].affected = 1,
            10 => changed[9].rows = 1,
            11 => changed[2].canonical_resource = false,
            _ => unreachable!(),
        }
        assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
    }
    let mut changed = data.clone();
    changed.remove(9);
    assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
    changed = data.clone();
    changed.push(data[10].clone());
    assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
    changed = data;
    changed.swap(2, 3);
    assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
}
#[test]
fn exact_admitted_cell_identity_geometry_and_current_terminal_context_refuse_drift() {
    assert_source_names_select_exact_lowercase_mir_with_original_uppercase_runtime_descriptors();
    assert_lowercase_mir_selection_refuses_noncanonical_missing_duplicate_and_width_drift();
    let Fixture {
        data,
        slots,
        sites,
        rows,
        fixed,
    } = fixture();
    for field in 0..6 {
        let mut changed = data.clone();
        match field {
            0 => changed[2].cells[3].id += 1,
            1 => changed[2].cells[3].name.push('X'),
            2 => changed[2].cells[3].base += 1,
            3 => changed[2].cells[3].offset += 1,
            4 => {
                changed[2].cells[3].bytes.pop();
            }
            5 => {
                changed[2].cells.pop();
            }
            _ => unreachable!(),
        }
        assert!(compare_data(&changed, &slots, &sites, &rows, &fixed).is_err());
    }
    let expected = serde_json::json!({"schema_version":"mainframe-env.cics-terminal@1", "session":"selected", "principal":"WEBADM",
        "run_unit":"actual-run", "connected":true, "suspended":false, "mapset":"COTRTLI", "map":"CTRTLIA",
        "aid":125, "transaction":"CA00", "rows":24, "columns":80, "version":1});
    assert!(terminal_matches(&expected, "selected", "actual-run"));
    for (name, value) in [
        ("schema_version", serde_json::json!("foreign")),
        ("session", serde_json::json!("foreign")),
        ("principal", serde_json::json!("WEBUSER")),
        ("run_unit", serde_json::json!("foreign")),
        ("connected", serde_json::json!(false)),
        ("suspended", serde_json::json!(true)),
        ("mapset", serde_json::json!("foreign")),
        ("map", serde_json::json!("foreign")),
        ("aid", serde_json::json!(0)),
        ("transaction", serde_json::json!("CTLI")),
        ("rows", serde_json::json!(25)),
        ("columns", serde_json::json!(81)),
        ("version", serde_json::json!(0)),
    ] {
        let mut changed = expected.clone();
        changed[name] = value;
        assert!(!terminal_matches(&changed, "selected", "actual-run"));
    }
}

fn named_selection_module(name: &str, width: u64, duplicate: bool) -> Module {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let base = builder.add_storage("__program_storage", 128, None).unwrap();
    let alias = mainframe_env_ir::StorageReference {
        storage: base,
        offset: 7,
        length: width,
    };
    builder
        .add_storage(name, width, Some(alias.clone()))
        .unwrap();
    if duplicate {
        builder.add_storage(name, width, Some(alias)).unwrap();
    }
    let region = builder.add_region().unwrap();
    builder.add_block(region).unwrap();
    builder.finish().unwrap()
}

fn assert_source_names_select_exact_lowercase_mir_with_original_uppercase_runtime_descriptors() {
    for (source, mir, width) in [
        ("SQLCA.SQLCODE", "sqlca.sqlcode", 4),
        ("SQLCA.SQLSTATE", "sqlca.sqlstate", 5),
        (
            "WS-MISC-STORAGE.WS-DB2-COMMON-VARS.WS-DUMMY-DB2-INT",
            "ws-misc-storage.ws-db2-common-vars.ws-dummy-db2-int",
            3,
        ),
        ("DCLTRANSACTION-TYPE", "dcltransaction-type", 54),
    ] {
        let module = named_selection_module(mir, width, false);
        let selected = selected_slot(&module, source, usize::try_from(width).unwrap()).unwrap();
        assert_eq!(selected.descriptor.storage(), module.storage()[1].id);
        assert_eq!(selected.descriptor.name(), source);
        assert_eq!(
            selected.descriptor.length(),
            usize::try_from(width).unwrap()
        );
        assert_eq!(
            (selected.base, selected.offset, selected.length),
            (0, 7, usize::try_from(width).unwrap())
        );
    }
}

fn assert_lowercase_mir_selection_refuses_noncanonical_missing_duplicate_and_width_drift() {
    for name in ["SQLCA.SQLCODE", "Sqlca.Sqlcode", "sqlca.sqlcode.other"] {
        let module = named_selection_module(name, 4, false);
        assert!(selected_slot(&module, "SQLCA.SQLCODE", 4).is_err());
    }
    let duplicate = named_selection_module("sqlca.sqlcode", 4, true);
    assert!(selected_slot(&duplicate, "SQLCA.SQLCODE", 4).is_err());
    let wrong_width = named_selection_module("sqlca.sqlcode", 5, false);
    assert!(selected_slot(&wrong_width, "SQLCA.SQLCODE", 4).is_err());
}
