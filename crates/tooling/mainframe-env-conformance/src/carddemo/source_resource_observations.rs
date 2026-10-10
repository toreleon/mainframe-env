//! Exact full parsed-source inventory, qualified separately from installation.
//! Declared operands are compared as metadata; macro assembly, default resolution
//! and terminal capability semantics remain unproved. Only the explicit orphan
//! disposition observation is emitted by this owner.
use super::*;
use mainframe_env_application::{BmsMap, CsdResource};

const EXPECTED: &str = include_str!("source_resource_expected.json");
const DISPOSITION: &str = "CDV1/COCRDSEC remains an explicit unresolved or accepted correction";

#[derive(Clone, Deserialize)]
struct ExpectedField {
    name: Option<String>,
    length: Option<usize>,
    position: Option<(usize, usize)>,
    attributes: Vec<String>,
    justify: Vec<String>,
    initial: Option<String>,
    options: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
struct ExpectedMap {
    mapset: String,
    name: String,
    line: Option<usize>,
    column: Option<usize>,
    size: Option<(usize, usize)>,
    fields: Vec<ExpectedField>,
    mapset_options: BTreeMap<String, String>,
    map_options: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
struct ExpectedResource {
    kind: String,
    name: String,
    properties: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
struct Oracle {
    schema_version: String,
    corpus_commit: String,
    source_pins: BTreeMap<String, String>,
    bms: BTreeMap<String, ExpectedMap>,
    csd: BTreeMap<String, Vec<ExpectedResource>>,
    source_paths: Vec<String>,
    available_programs: BTreeSet<String>,
    merged_resources: BTreeMap<String, ExpectedResource>,
    transaction_programs: BTreeMap<String, String>,
    unresolved: Vec<String>,
    counts: BTreeMap<String, usize>,
    fields: usize,
}

fn problem(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.resources.source_comparison", detail)
}

fn require(value: bool, detail: &str) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(problem(detail)) }
}

fn oracle() -> Result<Oracle, CorpusProblem> {
    let oracle: Oracle =
        serde_json::from_str(EXPECTED).map_err(|_| problem("frozen oracle is malformed"))?;
    require(
        oracle.schema_version == "mainframe-env.carddemo-full-source-resource-oracle@2"
            && oracle.corpus_commit == "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            && oracle.source_pins.len() == 69
            && oracle.bms.len() == 21
            && oracle.csd.len() == 4
            && oracle.source_paths.len() == 44
            && oracle.available_programs.len() == 44
            && oracle.unresolved == ["CDV1->COCRDSEC"],
        "frozen source identities or sole declared orphan differ",
    )?;
    Ok(oracle)
}

fn map_matches(actual: &BmsMap, expected: &ExpectedMap) -> bool {
    actual.mapset == expected.mapset
        && actual.name == expected.name
        && actual.line == expected.line
        && actual.column == expected.column
        && actual.size == expected.size
        && actual.mapset_options == expected.mapset_options
        && actual.map_options == expected.map_options
        && actual.fields.len() == expected.fields.len()
        && actual
            .fields
            .iter()
            .zip(&expected.fields)
            .all(|(actual, expected)| {
                actual.name == expected.name
                    && actual.length == expected.length
                    && actual.position == expected.position
                    && actual.attributes == expected.attributes
                    && actual.justify == expected.justify
                    && actual.initial == expected.initial
                    && actual.options == expected.options
            })
}

fn resource_matches(actual: &CsdResource, expected: &ExpectedResource) -> bool {
    actual.kind == expected.kind
        && actual.name == expected.name
        && actual.properties == expected.properties
}

fn csd_matches(actual: &[CsdResource], expected: &[ExpectedResource]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| resource_matches(actual, expected))
}

// This builder can witness only actual parse objects offered by the owner.
// It holds no server/router/store/artifact owner across application cleanup.
pub(super) struct SourceResourceWitness {
    oracle: Oracle,
    bms_seen: BTreeSet<String>,
    csd_seen: BTreeSet<String>,
}

// Constructor is private to successful completion of every source comparison.
// Counts/status/digests cannot construct this proof or an installation token.
pub(super) struct SourceResourceProof {
    unresolved: Vec<String>,
}

impl SourceResourceWitness {
    pub(super) fn new(corpus: &Path) -> Result<Self, CorpusProblem> {
        let oracle = oracle()?;
        for (path, digest) in &oracle.source_pins {
            validate_relative_path(path, "frozen resource source")?;
            let bytes = read_corpus_file(corpus, &corpus.join(path))?;
            require(
                format!("{:x}", Sha256::digest(&bytes)) == *digest,
                "pinned source bytes differ",
            )?;
        }
        Ok(Self {
            oracle,
            bms_seen: BTreeSet::new(),
            csd_seen: BTreeSet::new(),
        })
    }

    pub(super) fn observe_bms(&mut self, path: &str, actual: &BmsMap) -> Result<(), CorpusProblem> {
        require(
            self.oracle
                .bms
                .get(path)
                .is_some_and(|expected| map_matches(actual, expected))
                && self.bms_seen.insert(path.into()),
            "actual BMS path or complete represented field metadata differs",
        )
    }

    pub(super) fn observe_csd(
        &mut self,
        path: &str,
        actual: &[CsdResource],
    ) -> Result<(), CorpusProblem> {
        require(
            self.oracle
                .csd
                .get(path)
                .is_some_and(|expected| csd_matches(actual, expected))
                && self.csd_seen.insert(path.into()),
            "actual per-file CSD declaration order or complete properties differ",
        )
    }

    pub(super) fn finish(
        self,
        source_paths: &[String],
        available: &BTreeSet<String>,
        resources: &BTreeMap<String, CsdResource>,
        receipt: &CardDemoResourceReceipt,
    ) -> Result<SourceResourceProof, CorpusProblem> {
        require(
            self.bms_seen == self.oracle.bms.keys().cloned().collect()
                && self.csd_seen == self.oracle.csd.keys().cloned().collect()
                && source_paths == self.oracle.source_paths.as_slice()
                && available == &self.oracle.available_programs,
            "complete observed input-name inventory differs",
        )?;
        require(
            resources.len() == self.oracle.merged_resources.len()
                && resources.iter().all(|(key, actual)| {
                    self.oracle
                        .merged_resources
                        .get(key)
                        .is_some_and(|expected| resource_matches(actual, expected))
                }),
            "actual merged CSD catalog differs",
        )?;
        let transactions = resources
            .values()
            .filter(|row| row.kind == "TRANSACTION")
            .map(|row| {
                row.properties
                    .get("PROGRAM")
                    .map(|program| (row.name.clone(), program.clone()))
                    .ok_or_else(|| problem("transaction program reference missing"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mapsets = resources
            .values()
            .filter(|row| row.kind == "MAPSET")
            .map(|row| row.name.clone())
            .collect::<BTreeSet<_>>();
        let source_mapsets = self
            .oracle
            .bms
            .values()
            .map(|row| row.mapset.clone())
            .collect::<BTreeSet<_>>();
        require(
            transactions == self.oracle.transaction_programs && mapsets == source_mapsets,
            "complete transaction/program or CSD/map-source relation differs",
        )?;
        let count = |kind: &str| self.oracle.counts.get(kind).copied().unwrap_or(0);
        require(
            receipt.corpus_commit == self.oracle.corpus_commit
                && receipt.bms_sources == self.oracle.bms.len()
                && receipt.mapsets == source_mapsets.len()
                && receipt.maps == self.oracle.bms.len()
                && receipt.fields == self.oracle.fields
                && receipt.transactions == count("TRANSACTION")
                && receipt.programs == count("PROGRAM")
                && receipt.files == count("FILE")
                && receipt.tdqueues == count("TDQUEUE")
                && receipt.cross_references == self.oracle.transaction_programs.len()
                && receipt.unresolved == self.oracle.unresolved,
            "original receipt projection or explicit sole orphan differs",
        )?;
        // Runtime encoder/debug hash and pass status are preserved public data;
        // neither supplies this source proof or an installed catalog assertion.
        Ok(SourceResourceProof {
            unresolved: self.oracle.unresolved,
        })
    }
}

impl SourceResourceProof {
    pub(super) fn bind(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        require(
            self.unresolved == ["CDV1->COCRDSEC"],
            "retained explicit orphan differs",
        )?;
        observations.compare(
            journey_closure::AuthorityKind::Issue,
            "CD-010",
            DISPOSITION,
            false,
            || Ok(problem("explicit original orphan comparison refused")),
        )
    }
}

pub(super) fn verify_from_env(
    inventory: &Path,
) -> Result<(CardDemoResourceReceipt, SourceResourceProof), CorpusProblem> {
    let (receipt, proof) = verify_carddemo_resources(inventory, true)?;
    Ok((
        receipt,
        proof.ok_or_else(|| problem("observed source owner omitted proof"))?,
    ))
}

#[cfg(test)]
mod tests;
