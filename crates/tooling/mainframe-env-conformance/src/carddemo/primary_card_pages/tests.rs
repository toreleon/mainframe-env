use super::*;

fn terminal(page: Page) -> serde_json::Value {
    let expected = if matches!(page, Page::Next) {
        oracles::NEXT
    } else {
        oracles::FIRST
    };
    let mut fields = expected
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.to_vec()))
        .collect::<BTreeMap<_, _>>();
    if matches!(page, Page::Back) {
        fields.insert("ERRMSG".into(), oracles::BACK_ERROR.to_vec());
        fields.insert("INFOMSG".into(), oracles::BACK_INFO.to_vec());
    }
    screen(&fields)
}

fn screen(fields: &BTreeMap<String, Vec<u8>>) -> serde_json::Value {
    let mut bytes = Vec::new();
    for (name, value) in fields {
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value);
    }
    serde_json::json!({"session":"selected-session","mapset":"COCRDLI","map":"CCRDLIA",
        "screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
}

fn trace(page: Page) -> Vec<CicsTraceEntry> {
    let read = if matches!(page, Page::Back) {
        CicsOperation::ReadPrev
    } else {
        CicsOperation::ReadNext
    };
    (0..10)
        .map(|at| {
            let (operation, length) = if at == 0 {
                (CicsOperation::StartBrowse, 0)
            } else if at == 9 {
                (CicsOperation::EndBrowse, 0)
            } else {
                (read, 150)
            };
            CicsTraceEntry {
                operation,
                outcome: "NORMAL".into(),
                response: 0,
                response2: 0,
                payload_bytes: length,
            }
        })
        .collect()
}

fn refused(terminal: &serde_json::Value, actual: &[CicsTraceEntry], page: Page) {
    assert!(require_page(terminal, "selected-session", page, &[], actual).is_err());
}

#[test]
fn frozen_selected_primary_pages_are_controls_only() {
    for page in [Page::First, Page::Next, Page::Back] {
        require_page(&terminal(page), "selected-session", page, &[], &trace(page)).unwrap();
    }
    // No synthetic helper can grant observation credit.
    let observations = RouteObservations::default();
    assert!(observations.journeys().is_empty());
    assert!(observations.issues().is_empty());
}

#[test]
fn card_account_status_selection_page_and_message_corruptions_refuse() {
    for page in [Page::First, Page::Next, Page::Back] {
        for name in [
            "CRDNUM1", "ACCTNO7", "CRDSTS4", "CRDSEL2", "PAGENO", "INFOMSG", "ERRMSG",
        ] {
            let mut fields = online_screen_fields(&terminal(page)).unwrap();
            fields.get_mut(name).unwrap()[0] ^= 1;
            refused(&screen(&fields), &trace(page), page);
        }
    }
    // A valid back screen must pass before the whole, phase-wrong message is refused.
    require_page(
        &terminal(Page::Back),
        "selected-session",
        Page::Back,
        &[],
        &trace(Page::Back),
    )
    .unwrap();
    let mut fields = online_screen_fields(&terminal(Page::Back)).unwrap();
    let action = oracles::FIRST
        .iter()
        .find(|(name, _)| *name == "INFOMSG")
        .unwrap()
        .1;
    fields.insert("INFOMSG".into(), action.to_vec());
    refused(&screen(&fields), &trace(Page::Back), Page::Back);
    // Conversely, a blank info field is wrong during the first and next phases.
    for page in [Page::First, Page::Next] {
        let mut fields = online_screen_fields(&terminal(page)).unwrap();
        fields.insert("INFOMSG".into(), oracles::BACK_INFO.to_vec());
        refused(&screen(&fields), &trace(page), page);
    }
}

#[test]
fn whole_row_order_missing_extra_and_short_width_refuse() {
    let original = online_screen_fields(&terminal(Page::First)).unwrap();
    let mut fields = original.clone();
    for prefix in ["ACCTNO", "CRDNUM", "CRDSTS", "CRDSEL"] {
        fields.insert(
            format!("{prefix}1"),
            original[&format!("{prefix}2")].clone(),
        );
        fields.insert(
            format!("{prefix}2"),
            original[&format!("{prefix}1")].clone(),
        );
    }
    refused(&screen(&fields), &trace(Page::First), Page::First);
    let mut fields = original.clone();
    fields.remove("CRDNUM3");
    refused(&screen(&fields), &trace(Page::First), Page::First);
    let mut fields = original.clone();
    fields.insert("CRDNUM8".into(), b"9999999999999999".to_vec());
    refused(&screen(&fields), &trace(Page::First), Page::First);
    let mut fields = original;
    fields.get_mut("ACCTNO1").unwrap().pop();
    refused(&screen(&fields), &trace(Page::First), Page::First);
}

#[test]
fn wrong_direction_condition_length_missing_extra_and_mutation_refuse() {
    for page in [Page::First, Page::Next, Page::Back] {
        let source = trace(page);
        let mut changed = source.clone();
        changed[1].operation = if matches!(page, Page::Back) {
            CicsOperation::ReadNext
        } else {
            CicsOperation::ReadPrev
        };
        refused(&terminal(page), &changed, page);
        let mut changed = source.clone();
        changed[8].outcome = "ENDFILE".into();
        changed[8].response = 20;
        changed[8].payload_bytes = 0;
        refused(&terminal(page), &changed, page);
        let mut changed = source.clone();
        changed[2].response2 = 1;
        refused(&terminal(page), &changed, page);
        let mut changed = source.clone();
        changed[3].payload_bytes = 149;
        refused(&terminal(page), &changed, page);
        let mut changed = source.clone();
        changed.remove(8);
        refused(&terminal(page), &changed, page);
        let mut changed = source.clone();
        changed.insert(8, source[8].clone());
        refused(&terminal(page), &changed, page);
        let mut changed = source.clone();
        changed[4].operation = CicsOperation::Rewrite;
        refused(&terminal(page), &changed, page);
        let mut changed = source;
        changed.swap(0, 1);
        refused(&terminal(page), &changed, page);
    }
}

#[test]
fn other_session_map_and_reordered_or_saturated_trace_prefix_refuse() {
    let mut changed = terminal(Page::First);
    changed["session"] = "other".into();
    refused(&changed, &trace(Page::First), Page::First);
    let mut changed = terminal(Page::First);
    changed["map"] = "CCRDLIB".into();
    refused(&changed, &trace(Page::First), Page::First);
    let old = trace(Page::First);
    let mut after = old.clone();
    after.extend(trace(Page::Next));
    assert!(trace_matches(&old, &after, Page::Next));
    after[1].response = 1;
    assert!(!trace_matches(&old, &after, Page::Next));
    let after = vec![old[0].clone(); 4096];
    assert!(!trace_matches(&[], &after, Page::First));
}
