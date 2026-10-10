//! Exact selected primary-file page controls; zero observation tokens.
// The pinned COCRDLIC CARDAIX literal is unused. Do not grant AIX/PATH credit.
use super::*;
use mainframe_env_cics::CicsTraceEntry;

mod oracles;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum Page {
    First,
    Next,
    Back,
}

fn refuse() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.online.primary_card_page_drift",
        "selected primary page, browse sequence or complete raw base/index state differs",
    )
}

pub(super) fn check_inputs(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, digest) in oracles::INPUTS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        if format!("{:x}", Sha256::digest(bytes)) != *digest {
            return Err(refuse());
        }
    }
    Ok(())
}

fn selected_row_name(name: &str) -> bool {
    ["ACCTNO", "CRDNUM", "CRDSTS", "CRDSEL"]
        .into_iter()
        .any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

fn page_matches(
    terminal: &serde_json::Value,
    session: &str,
    page: Page,
) -> Result<bool, CorpusProblem> {
    let fields = online_screen_fields(terminal)?;
    let expected = if matches!(page, Page::Next) {
        oracles::NEXT
    } else {
        oracles::FIRST
    };
    let selected_names = fields.keys().filter(|name| selected_row_name(name)).count();
    Ok(terminal["session"] == session
        && terminal["mapset"] == "COCRDLI"
        && terminal["map"] == "CCRDLIA"
        && selected_names == 28
        && expected.iter().all(|(name, bytes)| {
            let bytes = if *name == "ERRMSG" && matches!(page, Page::Back) {
                oracles::BACK_ERROR
            } else if *name == "INFOMSG" && matches!(page, Page::Back) {
                oracles::BACK_INFO
            } else {
                *bytes
            };
            fields.get(*name).map(Vec::as_slice) == Some(bytes)
        }))
}

fn file_operation(operation: CicsOperation) -> bool {
    matches!(
        operation,
        CicsOperation::Read
            | CicsOperation::Write
            | CicsOperation::Rewrite
            | CicsOperation::Delete
            | CicsOperation::Unlock
            | CicsOperation::StartBrowse
            | CicsOperation::ResetBrowse
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::EndBrowse
    )
}

fn trace_matches(before: &[CicsTraceEntry], after: &[CicsTraceEntry], page: Page) -> bool {
    // Refuse saturation rather than accepting a truncated/incomplete producer trace.
    if after.len() >= 4096 || before.len() >= after.len() || !after.starts_with(before) {
        return false;
    }
    let reads = if matches!(page, Page::Back) {
        CicsOperation::ReadPrev
    } else {
        CicsOperation::ReadNext
    };
    let entries = after[before.len()..]
        .iter()
        .filter(|entry| file_operation(entry.operation))
        .collect::<Vec<_>>();
    entries.len() == 10
        && entries.iter().enumerate().all(|(at, entry)| {
            let (operation, length) = if at == 0 {
                (CicsOperation::StartBrowse, 0)
            } else if at == 9 {
                (CicsOperation::EndBrowse, 0)
            } else {
                (reads, 150)
            };
            entry.operation == operation
                && entry.outcome == "NORMAL"
                && entry.response == 0
                && entry.response2 == 0
                && entry.payload_bytes == length
        })
}

pub(super) fn require_page(
    terminal: &serde_json::Value,
    session: &str,
    page: Page,
    before_trace: &[CicsTraceEntry],
    after_trace: &[CicsTraceEntry],
) -> Result<(), CorpusProblem> {
    if !page_matches(terminal, session, page)? || !trace_matches(before_trace, after_trace, page) {
        return Err(refuse());
    }
    // Controls only: no RouteObservations mutation or journey/issue token.
    Ok(())
}

pub(super) fn require_unchanged(
    before: &card_observations::CardState,
    after: &card_observations::CardState,
) -> Result<(), CorpusProblem> {
    if !card_observations::unchanged_rewritten(before, after) {
        return Err(refuse());
    }
    Ok(())
}
