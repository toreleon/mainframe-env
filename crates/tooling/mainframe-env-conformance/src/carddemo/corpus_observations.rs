//! Actual corpus guard witnesses; partial CD-001, never application/license credit.
use super::corpus_validation::CorpusContract;
use super::journey_closure::{AuthorityKind, MAX_AUTHORITY_BYTES, MAX_TEXT_BYTES};
use super::{CardDemoCorpusReceipt, CorpusProblem, RouteObservations};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub(super) const INVENTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json"
));
const MAX_CHECK_IDENTITIES: usize = 64;
const IDENTITIES: &str = "commit, tree, cleanliness, license, content, runtime-archive, and file-count identities are checked";
const PATHS: &str = "no absolute local path enters evidence";

fn problem(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.corpus.observation_invalid", detail)
}

// Origins can be local paths. Preserve refusal identity without disclosing
// either offered origin; the original source/inventory comparisons remain.
pub(super) fn require_repository(expected: &str, actual: &str) -> Result<(), CorpusProblem> {
    if expected == actual {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.corpus.repository_drift",
            "repository origin differs from the pinned inventory",
        ))
    }
}

pub(super) fn require_pinned_inventory(path: &Path) -> Result<(), CorpusProblem> {
    let file = File::open(path).map_err(|_| problem("cannot read owning corpus inventory"))?;
    let mut bytes = Vec::new();
    file.take((INVENTORY.len() + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| problem("cannot read owning corpus inventory"))?;
    if bytes != INVENTORY {
        return Err(problem("owning corpus inventory bytes differ"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum FixedCheck {
    Cleanliness,
    Commit,
    Tree,
    Repository,
    License,
    Content,
}

const FIXED_CHECKS: [FixedCheck; 6] = [
    FixedCheck::Cleanliness,
    FixedCheck::Commit,
    FixedCheck::Tree,
    FixedCheck::Repository,
    FixedCheck::License,
    FixedCheck::Content,
];

#[derive(Default)]
pub(super) struct GateChecks {
    invalid: bool,
    fixed: BTreeMap<FixedCheck, String>,
    archive_order: Vec<String>,
    expected_archives: BTreeMap<String, String>,
    archives: BTreeMap<String, String>,
    expected_counts: BTreeMap<String, usize>,
    counts: BTreeMap<String, usize>,
}

fn admit_identity(text: &str) -> Result<(), CorpusProblem> {
    if text.is_empty() || text.len() > MAX_TEXT_BYTES || text.chars().any(char::is_control) {
        return Err(problem("malformed or oversized corpus check identity"));
    }
    Ok(())
}

impl GateChecks {
    pub(super) fn declare(&mut self, contract: &CorpusContract) -> Result<(), CorpusProblem> {
        let already_invalid = self.invalid;
        self.invalid = true;
        if already_invalid
            || !self.expected_archives.is_empty()
            || !self.expected_counts.is_empty()
            || contract.runtime_oracles.is_empty()
            || contract.file_count_checks.is_empty()
            || contract
                .runtime_oracles
                .len()
                .checked_add(contract.file_count_checks.len())
                .is_none_or(|total| total > MAX_CHECK_IDENTITIES)
        {
            return Err(problem(
                "duplicate, empty or oversized corpus check declaration",
            ));
        }
        for archive in &contract.runtime_oracles {
            admit_identity(&archive.path)?;
            if self
                .expected_archives
                .insert(archive.path.clone(), archive.sha256.clone())
                .is_some()
            {
                return Err(problem("duplicate runtime archive check declaration"));
            }
            self.archive_order.push(archive.path.clone());
        }
        for check in &contract.file_count_checks {
            admit_identity(&check.id)?;
            if self
                .expected_counts
                .insert(check.id.clone(), check.expected)
                .is_some()
            {
                return Err(problem("duplicate file-count check declaration"));
            }
        }
        self.invalid = false;
        Ok(())
    }

    pub(super) fn fixed(
        &mut self,
        check: FixedCheck,
        expected: &str,
        actual: &str,
    ) -> Result<(), CorpusProblem> {
        if self.invalid
            || expected != actual
            || expected.len() > MAX_TEXT_BYTES
            || actual.len() > MAX_TEXT_BYTES
            || self.fixed.contains_key(&check)
        {
            self.invalid = true;
            return Err(problem(
                "failed, duplicate or oversized fixed corpus comparison",
            ));
        }
        self.fixed.insert(check, actual.into());
        Ok(())
    }

    pub(super) fn archive(
        &mut self,
        path: &str,
        expected: &str,
        actual: &str,
    ) -> Result<(), CorpusProblem> {
        if self.invalid
            || expected != actual
            || expected.len() != 64
            || actual.len() != 64
            || self.expected_archives.get(path).map(String::as_str) != Some(expected)
            || self.archives.contains_key(path)
        {
            self.invalid = true;
            return Err(problem(
                "failed, undeclared or duplicate runtime archive comparison",
            ));
        }
        self.archives.insert(path.into(), actual.into());
        Ok(())
    }

    pub(super) fn count(
        &mut self,
        id: &str,
        expected: usize,
        actual: usize,
    ) -> Result<(), CorpusProblem> {
        if self.invalid
            || expected != actual
            || self.expected_counts.get(id) != Some(&expected)
            || self.counts.contains_key(id)
        {
            self.invalid = true;
            return Err(problem(
                "failed, undeclared or duplicate file-count comparison",
            ));
        }
        self.counts.insert(id.into(), actual);
        Ok(())
    }

    fn evidence_matches(&self, receipt: &CardDemoCorpusReceipt) -> bool {
        self.fixed.len() == FIXED_CHECKS.len()
            && FIXED_CHECKS.iter().all(|key| self.fixed.contains_key(key))
            && self.fixed.get(&FixedCheck::Cleanliness).map(String::as_str) == Some("")
            && receipt.clean
            && self.fixed.get(&FixedCheck::Commit) == Some(&receipt.commit)
            && self.fixed.get(&FixedCheck::Tree) == Some(&receipt.tree)
            && self.fixed.get(&FixedCheck::Repository) == Some(&receipt.repository)
            && self.fixed.get(&FixedCheck::License) == Some(&receipt.license.sha256)
            && self.fixed.get(&FixedCheck::Content) == Some(&receipt.content.sha256)
            && receipt
                .runtime_oracles
                .iter()
                .map(|row| &row.path)
                .eq(self.archive_order.iter())
            && receipt
                .runtime_oracles
                .iter()
                .all(|row| self.archives.get(&row.path) == Some(&row.sha256))
            && receipt.file_counts == self.counts
    }

    pub(super) fn finish(
        self,
        corpus: &Path,
        receipt: &CardDemoCorpusReceipt,
    ) -> Result<RouteObservations, CorpusProblem> {
        let refused = self.invalid
            || !self.evidence_matches(receipt)
            || self.expected_archives.is_empty()
            || self.expected_counts.is_empty()
            || self.archives != self.expected_archives
            || self.counts != self.expected_counts;
        if refused {
            return Err(problem(
                "corpus comparison witnesses are incomplete or changed",
            ));
        }
        let path_refused = relative_evidence_refused(corpus, receipt)?;
        if path_refused {
            return Err(problem(
                "corpus evidence has malformed identities or local paths",
            ));
        }
        let mut observations = RouteObservations::default();
        observations.compare(AuthorityKind::Issue, "CD-001", IDENTITIES, refused, || {
            Ok(problem("corpus comparison witnesses are incomplete"))
        })?;
        observations.compare(AuthorityKind::Issue, "CD-001", PATHS, path_refused, || {
            Ok(problem("corpus evidence contains a local path"))
        })?;
        Ok(observations)
    }
}

fn atom(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_TEXT_BYTES
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
fn hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn relative(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_TEXT_BYTES
        && !text.chars().any(char::is_control)
        && !text.contains(['\\', ':'])
        && text
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
fn runtime_locator(text: &str) -> bool {
    let parts = text.split('!').collect::<Vec<_>>();
    parts.len() <= 2 && parts.iter().all(|part| relative(part))
}

fn relative_evidence_refused(
    corpus: &Path,
    receipt: &CardDemoCorpusReceipt,
) -> Result<bool, CorpusProblem> {
    let bytes =
        serde_json::to_vec(receipt).map_err(|_| problem("cannot serialize corpus evidence"))?;
    let corpus = corpus
        .canonicalize()
        .map_err(|_| problem("cannot identify corpus directory"))?;
    let local_path_present = corpus.to_str().is_some_and(|path| {
        bytes
            .windows(path.len())
            .any(|window| window == path.as_bytes())
    });
    Ok(bytes.len() > MAX_AUTHORITY_BYTES
        || local_path_present
        || receipt.schema_version != "mainframe-env.carddemo-corpus-receipt@1"
        || receipt.status != "pass"
        || !receipt.clean
        || receipt.repository
            != "https://github.com/aws-samples/aws-mainframe-modernization-carddemo.git"
        || !hex(&receipt.commit, 40)
        || !hex(&receipt.tree, 40)
        || receipt.license.identifier != "Apache-2.0"
        || !hex(&receipt.license.sha256, 64)
        || receipt.content.algorithm != "sha256-u64be-path-u64be-content-v1"
        || !hex(&receipt.content.sha256, 64)
        || receipt
            .runtime_oracles
            .iter()
            .any(|row| !runtime_locator(&row.path) || !hex(&row.sha256, 64))
        || receipt.file_counts.keys().any(|id| !atom(id)))
}

#[cfg(test)]
pub(super) fn require_relative_evidence(
    corpus: &Path,
    receipt: &CardDemoCorpusReceipt,
) -> Result<(), CorpusProblem> {
    if relative_evidence_refused(corpus, receipt)? {
        Err(problem(
            "corpus evidence has malformed identities or local paths",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "corpus_observation_tests.rs"]
mod tests;
