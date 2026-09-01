use crate::{CoverageGate, GateState};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub const CONFORMANCE_SPEC_DOCUMENT_CONTRACT: &str = "mainframe-env.conformance-spec@1";
pub const CONFORMANCE_VERDICT_CONTRACT: &str = "mainframe-env.conformance-verdict@1";
pub const CONFORMANCE_LEDGER_CONTRACT: &str = "mainframe-env.conformance-ledger@1";
pub const CONFORMANCE_SPEC_VERSION_V1: &str = "mainframe-env.conformance-ir@1";
pub const CONFORMANCE_RUNNER_VERSION_V1: &str = "mainframe-env.conformance-runner@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConformanceLimits {
    pub max_spec_bytes: usize,
    pub max_identity_bytes: usize,
    pub max_locator_bytes: usize,
    pub max_observation_bytes: usize,
    pub max_catalog_rows: usize,
    pub max_row_specs: usize,
    pub max_obligations: usize,
    pub max_bindings: usize,
    pub max_registry_entries: usize,
    pub max_refs_per_case: usize,
    pub max_shards: u16,
}

impl Default for ConformanceLimits {
    fn default() -> Self {
        Self {
            max_spec_bytes: 16 * 1024 * 1024,
            max_identity_bytes: 512,
            max_locator_bytes: 2_048,
            max_observation_bytes: 16 * 1024,
            max_catalog_rows: 100_000,
            max_row_specs: 100_000,
            max_obligations: 1_000_000,
            max_bindings: 1_000_000,
            max_registry_entries: 100_000,
            max_refs_per_case: 256,
            max_shards: 4_096,
        }
    }
}

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(
                value: impl Into<String>,
                limits: ConformanceLimits,
            ) -> Result<Self, SpecProblem> {
                let value = value.into();
                validate_token(&value, limits)?;
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

typed_id!(OfficialRowId);
typed_id!(ObligationId);
typed_id!(OperationRef);
typed_id!(InputShapeRef);
typed_id!(PredicateRef);
typed_id!(TransitionRef);
typed_id!(ObservationRef);
typed_id!(ConditionRef);
typed_id!(RecoveryRef);
typed_id!(OracleRef);
typed_id!(DriverRef);
typed_id!(FixtureRef);
typed_id!(TestId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfficialCatalogRow {
    row_id: OfficialRowId,
    subsystem: String,
    family: String,
    source_locator: String,
    applicable_gates: BTreeSet<CoverageGate>,
}

impl OfficialCatalogRow {
    pub fn new(
        row_id: impl Into<String>,
        subsystem: impl Into<String>,
        family: impl Into<String>,
        source_locator: impl Into<String>,
        applicable_gates: impl IntoIterator<Item = CoverageGate>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        let subsystem = subsystem.into();
        let family = family.into();
        let source_locator = source_locator.into();
        validate_token(&subsystem, limits)?;
        validate_token(&family, limits)?;
        validate_text(&source_locator, limits.max_locator_bytes)?;
        let applicable_gates = applicable_gates.into_iter().collect::<BTreeSet<_>>();
        if applicable_gates.is_empty() {
            return Err(SpecProblem::IncompatibleGate(
                "catalog row has no applicable gates".into(),
            ));
        }
        Ok(Self {
            row_id: OfficialRowId::new(row_id, limits)?,
            subsystem,
            family,
            source_locator,
            applicable_gates,
        })
    }

    #[must_use]
    pub fn row_id(&self) -> &OfficialRowId {
        &self.row_id
    }

    #[must_use]
    pub fn subsystem(&self) -> &str {
        &self.subsystem
    }

    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    #[must_use]
    pub fn source_locator(&self) -> &str {
        &self.source_locator
    }

    #[must_use]
    pub fn applicable_gates(&self) -> &BTreeSet<CoverageGate> {
        &self.applicable_gates
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowSpec {
    row_id: OfficialRowId,
    operation: OperationRef,
    input: InputShapeRef,
    preconditions: Vec<PredicateRef>,
    transition: TransitionRef,
    postconditions: Vec<ObservationRef>,
    conditions: Vec<ConditionRef>,
    recovery: Option<RecoveryRef>,
    oracle: Option<OracleRef>,
    applicable_gates: BTreeSet<CoverageGate>,
    obligations: Vec<ObligationId>,
}

impl RowSpec {
    #[must_use]
    pub fn row_id(&self) -> &OfficialRowId {
        &self.row_id
    }

    #[must_use]
    pub fn operation(&self) -> &OperationRef {
        &self.operation
    }

    #[must_use]
    pub fn input(&self) -> &InputShapeRef {
        &self.input
    }

    #[must_use]
    pub fn preconditions(&self) -> &[PredicateRef] {
        &self.preconditions
    }

    #[must_use]
    pub fn transition(&self) -> &TransitionRef {
        &self.transition
    }

    #[must_use]
    pub fn postconditions(&self) -> &[ObservationRef] {
        &self.postconditions
    }

    #[must_use]
    pub fn conditions(&self) -> &[ConditionRef] {
        &self.conditions
    }

    #[must_use]
    pub fn recovery(&self) -> Option<&RecoveryRef> {
        self.recovery.as_ref()
    }

    #[must_use]
    pub fn oracle(&self) -> Option<&OracleRef> {
        self.oracle.as_ref()
    }

    #[must_use]
    pub fn applicable_gates(&self) -> &BTreeSet<CoverageGate> {
        &self.applicable_gates
    }

    #[must_use]
    pub fn obligations(&self) -> &[ObligationId] {
        &self.obligations
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandatoryObligation {
    row_id: OfficialRowId,
    obligation_id: ObligationId,
    applicable_gates: BTreeSet<CoverageGate>,
}

impl MandatoryObligation {
    #[must_use]
    pub fn row_id(&self) -> &OfficialRowId {
        &self.row_id
    }

    #[must_use]
    pub fn obligation_id(&self) -> &ObligationId {
        &self.obligation_id
    }

    #[must_use]
    pub fn applicable_gates(&self) -> &BTreeSet<CoverageGate> {
        &self.applicable_gates
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BindingKey {
    pub row_id: OfficialRowId,
    pub obligation_id: ObligationId,
    pub gate: CoverageGate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceCase {
    spec_version: String,
    key: BindingKey,
    test_id: TestId,
    driver: DriverRef,
    input: FixtureRef,
    preconditions: Vec<PredicateRef>,
    expected: Vec<ObservationRef>,
    recovery: Option<RecoveryRef>,
    oracle: Option<OracleRef>,
}

impl ConformanceCase {
    #[must_use]
    pub fn spec_version(&self) -> &str {
        &self.spec_version
    }

    #[must_use]
    pub fn key(&self) -> &BindingKey {
        &self.key
    }

    #[must_use]
    pub fn test_id(&self) -> &TestId {
        &self.test_id
    }

    #[must_use]
    pub fn driver(&self) -> &DriverRef {
        &self.driver
    }

    #[must_use]
    pub fn input(&self) -> &FixtureRef {
        &self.input
    }

    #[must_use]
    pub fn preconditions(&self) -> &[PredicateRef] {
        &self.preconditions
    }

    #[must_use]
    pub fn expected(&self) -> &[ObservationRef] {
        &self.expected
    }

    #[must_use]
    pub fn oracle(&self) -> Option<&OracleRef> {
        self.oracle.as_ref()
    }

    #[must_use]
    pub fn recovery(&self) -> Option<&RecoveryRef> {
        self.recovery.as_ref()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegistryDeclarations {
    operations: BTreeSet<OperationRef>,
    input_shapes: BTreeSet<InputShapeRef>,
    predicates: BTreeSet<PredicateRef>,
    transitions: BTreeSet<TransitionRef>,
    observations: BTreeSet<ObservationRef>,
    conditions: BTreeSet<ConditionRef>,
    recoveries: BTreeSet<RecoveryRef>,
    oracles: BTreeMap<OracleRef, String>,
    drivers: BTreeSet<DriverRef>,
    fixtures: BTreeMap<FixtureRef, String>,
}

impl RegistryDeclarations {
    #[must_use]
    pub fn operations(&self) -> &BTreeSet<OperationRef> {
        &self.operations
    }

    #[must_use]
    pub fn input_shapes(&self) -> &BTreeSet<InputShapeRef> {
        &self.input_shapes
    }

    #[must_use]
    pub fn drivers(&self) -> &BTreeSet<DriverRef> {
        &self.drivers
    }

    #[must_use]
    pub fn predicates(&self) -> &BTreeSet<PredicateRef> {
        &self.predicates
    }

    #[must_use]
    pub fn observations(&self) -> &BTreeSet<ObservationRef> {
        &self.observations
    }

    #[must_use]
    pub fn transitions(&self) -> &BTreeSet<TransitionRef> {
        &self.transitions
    }

    #[must_use]
    pub fn conditions(&self) -> &BTreeSet<ConditionRef> {
        &self.conditions
    }

    #[must_use]
    pub fn recoveries(&self) -> &BTreeSet<RecoveryRef> {
        &self.recoveries
    }

    #[must_use]
    pub fn oracles(&self) -> &BTreeMap<OracleRef, String> {
        &self.oracles
    }

    #[must_use]
    pub fn fixtures(&self) -> &BTreeMap<FixtureRef, String> {
        &self.fixtures
    }

    fn fixture_digest(&self, fixture: &FixtureRef) -> Option<&str> {
        self.fixtures.get(fixture).map(String::as_str)
    }

    fn oracle_digest(&self, oracle: &OracleRef) -> Option<&str> {
        self.oracles.get(oracle).map(String::as_str)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ShardKey {
    pub subsystem: String,
    pub family: String,
    pub gate: CoverageGate,
    pub bucket: u16,
    pub bucket_count: u16,
}

impl ShardKey {
    #[must_use]
    pub fn identity(&self) -> String {
        let mut digest = Sha256::new();
        for field in [
            self.subsystem.as_bytes(),
            self.family.as_bytes(),
            self.gate.slug().as_bytes(),
            &self.bucket.to_be_bytes(),
            &self.bucket_count.to_be_bytes(),
        ] {
            digest_field(&mut digest, field);
        }
        format!("sha256:{:x}", digest.finalize())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledSpec {
    spec_version: String,
    catalog_digest: String,
    spec_digest: String,
    shard_count: u16,
    catalog_rows: BTreeMap<OfficialRowId, OfficialCatalogRow>,
    rows: BTreeMap<OfficialRowId, RowSpec>,
    obligations: BTreeMap<(OfficialRowId, ObligationId), MandatoryObligation>,
    cases: BTreeMap<BindingKey, ConformanceCase>,
    registries: RegistryDeclarations,
    expected_shards: BTreeMap<ShardKey, BTreeSet<BindingKey>>,
}

impl CompiledSpec {
    pub fn compile_json(
        catalog_digest: &str,
        catalog_rows: Vec<OfficialCatalogRow>,
        bytes: &[u8],
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        if bytes.len() > limits.max_spec_bytes {
            return Err(SpecProblem::LimitExceeded("spec bytes"));
        }
        validate_digest(catalog_digest)?;
        let raw: RawSpecDocument = serde_json::from_slice(bytes)
            .map_err(|error| SpecProblem::MalformedDocument(error.to_string()))?;
        if raw.schema_version != CONFORMANCE_SPEC_DOCUMENT_CONTRACT {
            return Err(SpecProblem::StaleSpecVersion(raw.schema_version));
        }
        if raw.spec_version != CONFORMANCE_SPEC_VERSION_V1 {
            return Err(SpecProblem::StaleSpecVersion(raw.spec_version));
        }
        if raw.catalog_digest != catalog_digest {
            return Err(SpecProblem::CatalogDigestMismatch);
        }
        if raw.shard_count == 0 || raw.shard_count > limits.max_shards {
            return Err(SpecProblem::LimitExceeded("shards"));
        }
        if catalog_rows.len() > limits.max_catalog_rows {
            return Err(SpecProblem::LimitExceeded("catalog rows"));
        }
        let mut catalog = BTreeMap::new();
        for row in catalog_rows {
            let key = row.row_id.clone();
            if catalog.insert(key.clone(), row).is_some() {
                return Err(SpecProblem::DuplicateCatalogRow(key.to_string()));
            }
        }
        let registries = compile_registries(raw.registries, limits)?;
        if raw.rows.len() > limits.max_row_specs {
            return Err(SpecProblem::LimitExceeded("row specs"));
        }
        let mut rows = BTreeMap::new();
        for raw_row in raw.rows {
            let row = compile_row(raw_row, &catalog, &registries, limits)?;
            let key = row.row_id.clone();
            if rows.insert(key.clone(), row).is_some() {
                return Err(SpecProblem::DuplicateRowSpec(key.to_string()));
            }
        }
        if raw.obligations.len() > limits.max_obligations {
            return Err(SpecProblem::LimitExceeded("obligations"));
        }
        let mut obligations = BTreeMap::new();
        for raw_obligation in raw.obligations {
            let obligation = compile_obligation(raw_obligation, &rows, limits)?;
            let key = (obligation.row_id.clone(), obligation.obligation_id.clone());
            if obligations.insert(key.clone(), obligation).is_some() {
                return Err(SpecProblem::DuplicateObligation(format!(
                    "{}/{}",
                    key.0, key.1
                )));
            }
        }
        for row in rows.values() {
            for obligation in &row.obligations {
                if !obligations.contains_key(&(row.row_id.clone(), obligation.clone())) {
                    return Err(SpecProblem::MissingObligation(format!(
                        "{}/{}",
                        row.row_id, obligation
                    )));
                }
            }
        }
        for obligation in obligations.values() {
            let row = rows
                .get(&obligation.row_id)
                .ok_or_else(|| SpecProblem::UnknownRow(obligation.row_id.to_string()))?;
            if !row.obligations.contains(&obligation.obligation_id) {
                return Err(SpecProblem::UnknownObligation(format!(
                    "{}/{}",
                    obligation.row_id, obligation.obligation_id
                )));
            }
        }
        if raw.cases.len() > limits.max_bindings {
            return Err(SpecProblem::LimitExceeded("case bindings"));
        }
        let mut cases = BTreeMap::new();
        for raw_case in raw.cases {
            let case = compile_case(
                raw_case,
                &raw.spec_version,
                &rows,
                &obligations,
                &registries,
                limits,
            )?;
            let key = case.key.clone();
            if cases.insert(key.clone(), case).is_some() {
                return Err(SpecProblem::DuplicateBinding(format_binding(&key)));
            }
        }
        for obligation in obligations.values() {
            for gate in &obligation.applicable_gates {
                let key = BindingKey {
                    row_id: obligation.row_id.clone(),
                    obligation_id: obligation.obligation_id.clone(),
                    gate: *gate,
                };
                if !cases.contains_key(&key) {
                    return Err(SpecProblem::MissingBinding(format_binding(&key)));
                }
            }
        }
        let mut expected_shards = BTreeMap::<ShardKey, BTreeSet<BindingKey>>::new();
        for key in cases.keys() {
            let catalog_row = catalog
                .get(&key.row_id)
                .ok_or_else(|| SpecProblem::UnknownRow(key.row_id.to_string()))?;
            let bucket = binding_bucket(key, raw.shard_count);
            let shard = ShardKey {
                subsystem: catalog_row.subsystem.clone(),
                family: catalog_row.family.clone(),
                gate: key.gate,
                bucket,
                bucket_count: raw.shard_count,
            };
            expected_shards
                .entry(shard)
                .or_default()
                .insert(key.clone());
        }
        let spec_digest = format!("sha256:{:x}", Sha256::digest(bytes));
        Ok(Self {
            spec_version: raw.spec_version,
            catalog_digest: catalog_digest.into(),
            spec_digest,
            shard_count: raw.shard_count,
            catalog_rows: catalog,
            rows,
            obligations,
            cases,
            registries,
            expected_shards,
        })
    }

    #[must_use]
    pub fn spec_version(&self) -> &str {
        &self.spec_version
    }

    #[must_use]
    pub fn catalog_digest(&self) -> &str {
        &self.catalog_digest
    }

    #[must_use]
    pub fn spec_digest(&self) -> &str {
        &self.spec_digest
    }

    #[must_use]
    pub const fn shard_count(&self) -> u16 {
        self.shard_count
    }

    #[must_use]
    pub fn catalog_row(&self, id: &OfficialRowId) -> Option<&OfficialCatalogRow> {
        self.catalog_rows.get(id)
    }

    pub fn rows(&self) -> impl Iterator<Item = &RowSpec> {
        self.rows.values()
    }

    pub fn obligations(&self) -> impl Iterator<Item = &MandatoryObligation> {
        self.obligations.values()
    }

    pub fn cases(&self) -> impl Iterator<Item = &ConformanceCase> {
        self.cases.values()
    }

    #[must_use]
    pub fn case(&self, key: &BindingKey) -> Option<&ConformanceCase> {
        self.cases.get(key)
    }

    #[must_use]
    pub fn registries(&self) -> &RegistryDeclarations {
        &self.registries
    }

    #[must_use]
    pub fn expected_shards(&self) -> &BTreeMap<ShardKey, BTreeSet<BindingKey>> {
        &self.expected_shards
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheIdentity {
    identity: String,
}

#[derive(Clone, Debug)]
pub struct CacheIdentityInput<'a> {
    pub candidate_digest: &'a str,
    pub catalog_digest: &'a str,
    pub spec_digest: &'a str,
    pub runner_version: &'a str,
    pub fixture_digest: &'a str,
    pub oracle_digest: Option<&'a str>,
    pub environment_class: &'a str,
    pub shard: &'a ShardKey,
    pub binding: &'a BindingKey,
}

impl CacheIdentity {
    pub fn new(
        input: CacheIdentityInput<'_>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        for digest in [
            input.candidate_digest,
            input.catalog_digest,
            input.spec_digest,
            input.fixture_digest,
        ] {
            validate_digest(digest)?;
        }
        if let Some(oracle) = input.oracle_digest {
            validate_digest(oracle)?;
        }
        if input.runner_version != CONFORMANCE_RUNNER_VERSION_V1 {
            return Err(SpecProblem::UnsafeCacheKey("runner version".into()));
        }
        validate_token(input.environment_class, limits)?;
        if input.shard.gate != input.binding.gate
            || input.shard.bucket != binding_bucket(input.binding, input.shard.bucket_count)
        {
            return Err(SpecProblem::UnsafeCacheKey("shard/binding mismatch".into()));
        }
        let mut digest = Sha256::new();
        for field in [
            input.candidate_digest,
            input.catalog_digest,
            input.spec_digest,
            input.runner_version,
            input.fixture_digest,
            input.oracle_digest.unwrap_or("none"),
            input.environment_class,
            input.shard.subsystem.as_str(),
            input.shard.family.as_str(),
            input.shard.gate.slug(),
            input.binding.row_id.as_str(),
            input.binding.obligation_id.as_str(),
            input.binding.gate.slug(),
        ] {
            digest_field(&mut digest, field.as_bytes());
        }
        digest_field(&mut digest, &input.shard.bucket.to_be_bytes());
        digest_field(&mut digest, &input.shard.bucket_count.to_be_bytes());
        Ok(Self {
            identity: format!("sha256:{:x}", digest.finalize()),
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.identity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerdictEvent {
    pub schema_version: &'static str,
    pub spec_version: String,
    pub key: BindingKey,
    pub test_id: TestId,
    pub verdict: Verdict,
    pub observation_digest: String,
    pub cache_identity: String,
    pub replay: String,
    pub source_locator: String,
    pub driver: DriverRef,
    pub fixture_or_seed: FixtureRef,
    pub expected: String,
    pub actual: String,
    pub oracle_receipt_digest: Option<String>,
}

impl VerdictEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        spec_version: impl Into<String>,
        key: BindingKey,
        test_id: TestId,
        verdict: Verdict,
        observation_digest: impl Into<String>,
        cache_identity: impl Into<String>,
        source_locator: impl Into<String>,
        driver: DriverRef,
        fixture_or_seed: FixtureRef,
        expected: impl Into<String>,
        actual: impl Into<String>,
        oracle_receipt_digest: Option<String>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        let spec_version = spec_version.into();
        if spec_version != CONFORMANCE_SPEC_VERSION_V1 {
            return Err(SpecProblem::StaleSpecVersion(spec_version));
        }
        let observation_digest = observation_digest.into();
        let cache_identity = cache_identity.into();
        validate_digest(&observation_digest)?;
        validate_digest(&cache_identity)?;
        let source_locator = source_locator.into();
        let expected = expected.into();
        let actual = actual.into();
        validate_text(&source_locator, limits.max_locator_bytes)?;
        validate_text(&expected, limits.max_observation_bytes)?;
        validate_text(&actual, limits.max_observation_bytes)?;
        if let Some(receipt) = &oracle_receipt_digest {
            validate_digest(receipt)?;
        }
        if key.gate == CoverageGate::Differential
            && verdict == Verdict::Pass
            && oracle_receipt_digest.is_none()
        {
            return Err(SpecProblem::OracleReceiptRequired);
        }
        let replay = format!("cargo xtask conformance --replay {}", test_id.as_str());
        Ok(Self {
            schema_version: CONFORMANCE_VERDICT_CONTRACT,
            spec_version,
            key,
            test_id,
            verdict,
            observation_digest,
            cache_identity,
            replay,
            source_locator,
            driver,
            fixture_or_seed,
            expected,
            actual,
            oracle_receipt_digest,
        })
    }

    pub fn canonical_json(&self) -> Result<Vec<u8>, SpecProblem> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": self.schema_version,
            "spec_version": self.spec_version,
            "row_id": self.key.row_id.as_str(),
            "obligation_id": self.key.obligation_id.as_str(),
            "gate": self.key.gate.slug(),
            "test_id": self.test_id.as_str(),
            "verdict": self.verdict.slug(),
            "observation_digest": self.observation_digest,
            "cache_identity": self.cache_identity,
            "replay": self.replay,
            "source_locator": self.source_locator,
            "driver": self.driver.as_str(),
            "fixture_or_seed": self.fixture_or_seed.as_str(),
            "expected": self.expected,
            "actual": self.actual,
            "oracle_receipt_digest": self.oracle_receipt_digest,
        }))
        .map_err(|error| SpecProblem::RuntimeFailure(error.to_string()))
    }

    pub fn identity(&self) -> Result<String, SpecProblem> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(self.canonical_json()?)
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerBinding {
    pub obligation_id: ObligationId,
    pub test_id: TestId,
    pub verdict: Verdict,
    pub observation_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerGate {
    pub state: GateState,
    pub mandatory_obligations: usize,
    pub passed_obligations: usize,
    pub bindings: Vec<LedgerBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerRow {
    pub row_id: OfficialRowId,
    pub subsystem: String,
    pub family: String,
    pub source_locator: String,
    pub gates: BTreeMap<CoverageGate, LedgerGate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerCount {
    pub pass: usize,
    pub fail: usize,
    pub pending: usize,
    pub non_applicable: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedConformanceLedger {
    pub schema_version: &'static str,
    pub spec_version: String,
    pub catalog_digest: String,
    pub spec_digest: String,
    pub rows: BTreeMap<OfficialRowId, LedgerRow>,
    pub counts: BTreeMap<CoverageGate, LedgerCount>,
}

impl DerivedConformanceLedger {
    pub fn derive_complete(
        spec: &CompiledSpec,
        events: Vec<VerdictEvent>,
    ) -> Result<Self, SpecProblem> {
        Self::derive(spec, events, true)
    }

    pub fn derive_partial(
        spec: &CompiledSpec,
        events: Vec<VerdictEvent>,
    ) -> Result<Self, SpecProblem> {
        Self::derive(spec, events, false)
    }

    fn derive(
        spec: &CompiledSpec,
        events: Vec<VerdictEvent>,
        require_complete: bool,
    ) -> Result<Self, SpecProblem> {
        let mut by_binding = BTreeMap::new();
        for event in events {
            if event.spec_version != spec.spec_version {
                return Err(SpecProblem::StaleSpecVersion(event.spec_version));
            }
            let Some(case) = spec.cases.get(&event.key) else {
                return Err(SpecProblem::UnknownBinding(format_binding(&event.key)));
            };
            if event.test_id != case.test_id {
                return Err(SpecProblem::ConflictingVerdict(format_binding(&event.key)));
            }
            let key = event.key.clone();
            if by_binding.insert(key.clone(), event).is_some() {
                return Err(SpecProblem::DuplicateVerdict(format_binding(&key)));
            }
        }
        if require_complete {
            for key in spec.cases.keys() {
                if !by_binding.contains_key(key) {
                    return Err(SpecProblem::MissingVerdict(format_binding(key)));
                }
            }
        }
        let mut rows = BTreeMap::new();
        for catalog in spec.catalog_rows.values() {
            let mut gates = BTreeMap::new();
            for gate in CoverageGate::ALL {
                if !catalog.applicable_gates.contains(&gate) {
                    gates.insert(
                        gate,
                        LedgerGate {
                            state: GateState::NotApplicable,
                            mandatory_obligations: 0,
                            passed_obligations: 0,
                            bindings: Vec::new(),
                        },
                    );
                    continue;
                }
                let mandatory = spec
                    .obligations
                    .values()
                    .filter(|obligation| {
                        obligation.row_id == catalog.row_id
                            && obligation.applicable_gates.contains(&gate)
                    })
                    .collect::<Vec<_>>();
                let mut bindings = Vec::new();
                for obligation in &mandatory {
                    let key = BindingKey {
                        row_id: catalog.row_id.clone(),
                        obligation_id: obligation.obligation_id.clone(),
                        gate,
                    };
                    if let Some(event) = by_binding.get(&key) {
                        bindings.push(LedgerBinding {
                            obligation_id: obligation.obligation_id.clone(),
                            test_id: event.test_id.clone(),
                            verdict: event.verdict,
                            observation_digest: event.observation_digest.clone(),
                        });
                    }
                }
                bindings.sort_by(|left, right| left.obligation_id.cmp(&right.obligation_id));
                let passed = bindings
                    .iter()
                    .filter(|binding| binding.verdict == Verdict::Pass)
                    .count();
                let failed = bindings
                    .iter()
                    .any(|binding| binding.verdict == Verdict::Fail);
                let state = if failed {
                    GateState::Failed
                } else if !mandatory.is_empty() && passed == mandatory.len() {
                    GateState::Passed
                } else {
                    GateState::Pending
                };
                gates.insert(
                    gate,
                    LedgerGate {
                        state,
                        mandatory_obligations: mandatory.len(),
                        passed_obligations: passed,
                        bindings,
                    },
                );
            }
            rows.insert(
                catalog.row_id.clone(),
                LedgerRow {
                    row_id: catalog.row_id.clone(),
                    subsystem: catalog.subsystem.clone(),
                    family: catalog.family.clone(),
                    source_locator: catalog.source_locator.clone(),
                    gates,
                },
            );
        }
        let counts = CoverageGate::ALL
            .into_iter()
            .map(|gate| {
                let mut count = LedgerCount {
                    pass: 0,
                    fail: 0,
                    pending: 0,
                    non_applicable: 0,
                };
                for row in rows.values() {
                    match row.gates[&gate].state {
                        GateState::Passed => count.pass += 1,
                        GateState::Failed => count.fail += 1,
                        GateState::Pending => count.pending += 1,
                        GateState::NotApplicable => count.non_applicable += 1,
                    }
                }
                (gate, count)
            })
            .collect();
        Ok(Self {
            schema_version: CONFORMANCE_LEDGER_CONTRACT,
            spec_version: spec.spec_version.clone(),
            catalog_digest: spec.catalog_digest.clone(),
            spec_digest: spec.spec_digest.clone(),
            rows,
            counts,
        })
    }

    pub fn canonical_json(&self) -> Result<Vec<u8>, SpecProblem> {
        let counts = CoverageGate::ALL
            .into_iter()
            .map(|gate| {
                let count = &self.counts[&gate];
                serde_json::json!({
                    "gate": gate.slug(),
                    "pass": count.pass,
                    "fail": count.fail,
                    "pending": count.pending,
                    "non_applicable": count.non_applicable,
                })
            })
            .collect::<Vec<_>>();
        let rows = self
            .rows
            .values()
            .map(|row| {
                let gates = CoverageGate::ALL
                    .into_iter()
                    .map(|gate| {
                        let result = &row.gates[&gate];
                        let bindings = result
                            .bindings
                            .iter()
                            .map(|binding| {
                                serde_json::json!({
                                    "obligation_id": binding.obligation_id.as_str(),
                                    "test_id": binding.test_id.as_str(),
                                    "verdict": binding.verdict.slug(),
                                    "observation_digest": binding.observation_digest,
                                })
                            })
                            .collect::<Vec<_>>();
                        serde_json::json!({
                            "gate": gate.slug(),
                            "state": gate_state_slug(result.state),
                            "mandatory_obligations": result.mandatory_obligations,
                            "passed_obligations": result.passed_obligations,
                            "bindings": bindings,
                        })
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "row_id": row.row_id.as_str(),
                    "subsystem": row.subsystem,
                    "family": row.family,
                    "source_locator": row.source_locator,
                    "gates": gates,
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_vec(&serde_json::json!({
            "schema_version": self.schema_version,
            "spec_version": self.spec_version,
            "catalog_digest": self.catalog_digest,
            "spec_digest": self.spec_digest,
            "counts": counts,
            "rows": rows,
        }))
        .map_err(|error| SpecProblem::RuntimeFailure(error.to_string()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpecProblem {
    MalformedDocument(String),
    InvalidIdentity(String),
    InvalidText,
    InvalidDigest,
    CatalogDigestMismatch,
    DuplicateCatalogRow(String),
    DuplicateRowSpec(String),
    UnknownRow(String),
    MissingObligation(String),
    DuplicateObligation(String),
    UnknownObligation(String),
    UnknownRegistryRef(String),
    DuplicateRegistryRef(String),
    MissingBinding(String),
    DuplicateBinding(String),
    UnknownBinding(String),
    StaleSpecVersion(String),
    IncompatibleGate(String),
    IncompleteShardSet,
    UnsafeCacheKey(String),
    OracleReceiptRequired,
    DuplicateVerdict(String),
    MissingVerdict(String),
    ConflictingVerdict(String),
    RuntimeRegistryIncomplete(String),
    RuntimeFailure(String),
    LimitExceeded(&'static str),
}

impl std::fmt::Display for SpecProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Conformance IR rejected input: {self:?}")
    }
}

impl std::error::Error for SpecProblem {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpecDocument {
    schema_version: String,
    spec_version: String,
    catalog_digest: String,
    shard_count: u16,
    registries: RawRegistries,
    rows: Vec<RawRowSpec>,
    obligations: Vec<RawObligation>,
    cases: Vec<RawCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistries {
    operations: Vec<String>,
    input_shapes: Vec<String>,
    predicates: Vec<String>,
    transitions: Vec<String>,
    observations: Vec<String>,
    conditions: Vec<String>,
    recoveries: Vec<String>,
    oracles: Vec<RawArtifactEntry>,
    drivers: Vec<String>,
    fixtures: Vec<RawArtifactEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArtifactEntry {
    id: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRowSpec {
    row_id: String,
    operation: String,
    input: String,
    preconditions: Vec<String>,
    transition: String,
    postconditions: Vec<String>,
    conditions: Vec<String>,
    recovery: Option<String>,
    oracle: Option<String>,
    applicable_gates: Vec<String>,
    obligations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObligation {
    row_id: String,
    obligation_id: String,
    applicable_gates: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    spec_version: String,
    row_id: String,
    obligation_id: String,
    gate: String,
    test_id: String,
    driver: String,
    input: String,
    preconditions: Vec<String>,
    expected: Vec<String>,
    recovery: Option<String>,
    oracle: Option<String>,
}

fn compile_registries(
    raw: RawRegistries,
    limits: ConformanceLimits,
) -> Result<RegistryDeclarations, SpecProblem> {
    Ok(RegistryDeclarations {
        operations: compile_set(raw.operations, OperationRef::new, "operation", limits)?,
        input_shapes: compile_set(raw.input_shapes, InputShapeRef::new, "input", limits)?,
        predicates: compile_set(raw.predicates, PredicateRef::new, "predicate", limits)?,
        transitions: compile_set(raw.transitions, TransitionRef::new, "transition", limits)?,
        observations: compile_set(raw.observations, ObservationRef::new, "observation", limits)?,
        conditions: compile_set(raw.conditions, ConditionRef::new, "condition", limits)?,
        recoveries: compile_set(raw.recoveries, RecoveryRef::new, "recovery", limits)?,
        oracles: compile_artifacts(raw.oracles, OracleRef::new, "oracle", limits)?,
        drivers: compile_set(raw.drivers, DriverRef::new, "driver", limits)?,
        fixtures: compile_artifacts(raw.fixtures, FixtureRef::new, "fixture", limits)?,
    })
}

fn compile_set<T: Ord + std::fmt::Display>(
    raw: Vec<String>,
    constructor: impl Fn(String, ConformanceLimits) -> Result<T, SpecProblem>,
    kind: &str,
    limits: ConformanceLimits,
) -> Result<BTreeSet<T>, SpecProblem> {
    if raw.len() > limits.max_registry_entries {
        return Err(SpecProblem::LimitExceeded("registry entries"));
    }
    let mut result = BTreeSet::new();
    for value in raw {
        let value = constructor(value, limits)?;
        if !result.insert(value) {
            return Err(SpecProblem::DuplicateRegistryRef(kind.into()));
        }
    }
    Ok(result)
}

fn compile_artifacts<T: Ord + Clone + std::fmt::Display>(
    raw: Vec<RawArtifactEntry>,
    constructor: impl Fn(String, ConformanceLimits) -> Result<T, SpecProblem>,
    kind: &str,
    limits: ConformanceLimits,
) -> Result<BTreeMap<T, String>, SpecProblem> {
    if raw.len() > limits.max_registry_entries {
        return Err(SpecProblem::LimitExceeded("artifact registry entries"));
    }
    let mut result = BTreeMap::new();
    for entry in raw {
        validate_digest(&entry.digest)?;
        let id = constructor(entry.id, limits)?;
        if result.insert(id.clone(), entry.digest).is_some() {
            return Err(SpecProblem::DuplicateRegistryRef(format!("{kind}:{id}")));
        }
    }
    Ok(result)
}

fn compile_row(
    raw: RawRowSpec,
    catalog: &BTreeMap<OfficialRowId, OfficialCatalogRow>,
    registries: &RegistryDeclarations,
    limits: ConformanceLimits,
) -> Result<RowSpec, SpecProblem> {
    enforce_ref_limit(raw.preconditions.len(), limits)?;
    enforce_ref_limit(raw.postconditions.len(), limits)?;
    enforce_ref_limit(raw.conditions.len(), limits)?;
    enforce_ref_limit(raw.obligations.len(), limits)?;
    let row_id = OfficialRowId::new(raw.row_id, limits)?;
    let catalog_row = catalog
        .get(&row_id)
        .ok_or_else(|| SpecProblem::UnknownRow(row_id.to_string()))?;
    let operation = OperationRef::new(raw.operation, limits)?;
    require_registry(&registries.operations, &operation, "operation")?;
    let input = InputShapeRef::new(raw.input, limits)?;
    require_registry(&registries.input_shapes, &input, "input")?;
    let preconditions = compile_refs(raw.preconditions, PredicateRef::new, limits)?;
    require_registry_all(&registries.predicates, &preconditions, "predicate")?;
    let transition = TransitionRef::new(raw.transition, limits)?;
    require_registry(&registries.transitions, &transition, "transition")?;
    let postconditions = compile_refs(raw.postconditions, ObservationRef::new, limits)?;
    require_registry_all(&registries.observations, &postconditions, "observation")?;
    let conditions = compile_refs(raw.conditions, ConditionRef::new, limits)?;
    require_registry_all(&registries.conditions, &conditions, "condition")?;
    let recovery = raw
        .recovery
        .map(|value| RecoveryRef::new(value, limits))
        .transpose()?;
    if let Some(reference) = &recovery {
        require_registry(&registries.recoveries, reference, "recovery")?;
    }
    let oracle = raw
        .oracle
        .map(|value| OracleRef::new(value, limits))
        .transpose()?;
    if let Some(reference) = &oracle {
        require_registry_map(&registries.oracles, reference, "oracle")?;
    }
    let applicable_gates = compile_gates(raw.applicable_gates)?;
    if applicable_gates != catalog_row.applicable_gates {
        return Err(SpecProblem::IncompatibleGate(format!(
            "{} must preserve catalog applicability",
            row_id
        )));
    }
    let obligations = compile_refs(raw.obligations, ObligationId::new, limits)?;
    if obligations.is_empty() {
        return Err(SpecProblem::MissingObligation(row_id.to_string()));
    }
    Ok(RowSpec {
        row_id,
        operation,
        input,
        preconditions,
        transition,
        postconditions,
        conditions,
        recovery,
        oracle,
        applicable_gates,
        obligations,
    })
}

fn compile_obligation(
    raw: RawObligation,
    rows: &BTreeMap<OfficialRowId, RowSpec>,
    limits: ConformanceLimits,
) -> Result<MandatoryObligation, SpecProblem> {
    let row_id = OfficialRowId::new(raw.row_id, limits)?;
    let row = rows
        .get(&row_id)
        .ok_or_else(|| SpecProblem::UnknownRow(row_id.to_string()))?;
    let obligation_id = ObligationId::new(raw.obligation_id, limits)?;
    let applicable_gates = compile_gates(raw.applicable_gates)?;
    if !applicable_gates.is_subset(&row.applicable_gates) {
        return Err(SpecProblem::IncompatibleGate(format!(
            "{row_id}/{obligation_id}"
        )));
    }
    Ok(MandatoryObligation {
        row_id,
        obligation_id,
        applicable_gates,
    })
}

fn compile_case(
    raw: RawCase,
    spec_version: &str,
    rows: &BTreeMap<OfficialRowId, RowSpec>,
    obligations: &BTreeMap<(OfficialRowId, ObligationId), MandatoryObligation>,
    registries: &RegistryDeclarations,
    limits: ConformanceLimits,
) -> Result<ConformanceCase, SpecProblem> {
    if raw.spec_version != spec_version {
        return Err(SpecProblem::StaleSpecVersion(raw.spec_version));
    }
    enforce_ref_limit(raw.preconditions.len(), limits)?;
    enforce_ref_limit(raw.expected.len(), limits)?;
    let row_id = OfficialRowId::new(raw.row_id, limits)?;
    let obligation_id = ObligationId::new(raw.obligation_id, limits)?;
    let gate = parse_gate(&raw.gate)?;
    let key = BindingKey {
        row_id: row_id.clone(),
        obligation_id: obligation_id.clone(),
        gate,
    };
    let row = rows
        .get(&row_id)
        .ok_or_else(|| SpecProblem::UnknownRow(row_id.to_string()))?;
    let obligation = obligations
        .get(&(row_id, obligation_id))
        .ok_or_else(|| SpecProblem::UnknownObligation(format_binding(&key)))?;
    if !obligation.applicable_gates.contains(&gate) {
        return Err(SpecProblem::IncompatibleGate(format_binding(&key)));
    }
    let test_id = TestId::new(raw.test_id, limits)?;
    let driver = DriverRef::new(raw.driver, limits)?;
    require_registry(&registries.drivers, &driver, "driver")?;
    let input = FixtureRef::new(raw.input, limits)?;
    require_registry_map(&registries.fixtures, &input, "fixture")?;
    let preconditions = compile_refs(raw.preconditions, PredicateRef::new, limits)?;
    require_registry_all(&registries.predicates, &preconditions, "predicate")?;
    if !preconditions
        .iter()
        .all(|item| row.preconditions.contains(item))
    {
        return Err(SpecProblem::UnknownRegistryRef(
            "case precondition is outside row specification".into(),
        ));
    }
    let expected = compile_refs(raw.expected, ObservationRef::new, limits)?;
    require_registry_all(&registries.observations, &expected, "observation")?;
    if expected.is_empty()
        || !expected
            .iter()
            .all(|item| row.postconditions.contains(item))
    {
        return Err(SpecProblem::UnknownRegistryRef(
            "case expectation is outside row specification".into(),
        ));
    }
    let recovery = raw
        .recovery
        .map(|value| RecoveryRef::new(value, limits))
        .transpose()?;
    if recovery != row.recovery {
        return Err(SpecProblem::UnknownRegistryRef(
            "case recovery differs from row specification".into(),
        ));
    }
    let oracle = raw
        .oracle
        .map(|value| OracleRef::new(value, limits))
        .transpose()?;
    if oracle != row.oracle {
        return Err(SpecProblem::UnknownRegistryRef(
            "case oracle differs from row specification".into(),
        ));
    }
    if gate == CoverageGate::Differential && oracle.is_none() {
        return Err(SpecProblem::OracleReceiptRequired);
    }
    Ok(ConformanceCase {
        spec_version: spec_version.into(),
        key,
        test_id,
        driver,
        input,
        preconditions,
        expected,
        recovery,
        oracle,
    })
}

fn compile_refs<T: Ord + Clone + std::fmt::Display>(
    raw: Vec<String>,
    constructor: impl Fn(String, ConformanceLimits) -> Result<T, SpecProblem>,
    limits: ConformanceLimits,
) -> Result<Vec<T>, SpecProblem> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(raw.len());
    for value in raw {
        let value = constructor(value, limits)?;
        if !seen.insert(value.clone()) {
            return Err(SpecProblem::DuplicateRegistryRef(value.to_string()));
        }
        result.push(value);
    }
    Ok(result)
}

fn compile_gates(raw: Vec<String>) -> Result<BTreeSet<CoverageGate>, SpecProblem> {
    let gates = raw
        .iter()
        .map(|gate| parse_gate(gate))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if gates.is_empty() || gates.len() != raw.len() {
        return Err(SpecProblem::IncompatibleGate(
            "gate set is empty or duplicated".into(),
        ));
    }
    Ok(gates)
}

fn parse_gate(value: &str) -> Result<CoverageGate, SpecProblem> {
    CoverageGate::ALL
        .into_iter()
        .find(|gate| gate.slug() == value)
        .ok_or_else(|| SpecProblem::IncompatibleGate(value.into()))
}

fn require_registry<T: Ord + std::fmt::Display>(
    registry: &BTreeSet<T>,
    reference: &T,
    kind: &str,
) -> Result<(), SpecProblem> {
    if registry.contains(reference) {
        Ok(())
    } else {
        Err(SpecProblem::UnknownRegistryRef(format!(
            "{kind}:{reference}"
        )))
    }
}

fn require_registry_map<T: Ord + std::fmt::Display>(
    registry: &BTreeMap<T, String>,
    reference: &T,
    kind: &str,
) -> Result<(), SpecProblem> {
    if registry.contains_key(reference) {
        Ok(())
    } else {
        Err(SpecProblem::UnknownRegistryRef(format!(
            "{kind}:{reference}"
        )))
    }
}

fn require_registry_all<T: Ord + std::fmt::Display>(
    registry: &BTreeSet<T>,
    references: &[T],
    kind: &str,
) -> Result<(), SpecProblem> {
    for reference in references {
        require_registry(registry, reference, kind)?;
    }
    Ok(())
}

fn enforce_ref_limit(count: usize, limits: ConformanceLimits) -> Result<(), SpecProblem> {
    if count > limits.max_refs_per_case {
        Err(SpecProblem::LimitExceeded("references"))
    } else {
        Ok(())
    }
}

fn format_binding(key: &BindingKey) -> String {
    format!("{}/{}/{}", key.row_id, key.obligation_id, key.gate.slug())
}

fn binding_bucket(key: &BindingKey, bucket_count: u16) -> u16 {
    let mut digest = Sha256::new();
    for field in [
        key.row_id.as_str(),
        key.obligation_id.as_str(),
        key.gate.slug(),
    ] {
        digest_field(&mut digest, field.as_bytes());
    }
    let bytes: [u8; 8] = digest.finalize()[..8].try_into().expect("eight bytes");
    (u64::from_be_bytes(bytes) % u64::from(bucket_count)) as u16
}

fn validate_token(value: &str, limits: ConformanceLimits) -> Result<(), SpecProblem> {
    if value.is_empty()
        || value.len() > limits.max_identity_bytes
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'@' | b'+')
        })
    {
        Err(SpecProblem::InvalidIdentity(value.into()))
    } else {
        Ok(())
    }
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), SpecProblem> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        Err(SpecProblem::InvalidText)
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str) -> Result<(), SpecProblem> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(SpecProblem::InvalidDigest)
    }
}

const fn gate_state_slug(state: GateState) -> &'static str {
    match state {
        GateState::NotApplicable => "not-applicable",
        GateState::Pending => "pending",
        GateState::Failed => "failed",
        GateState::Passed => "passed",
    }
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverOutput {
    bytes: Vec<u8>,
    oracle_receipt_digest: Option<String>,
}

impl DriverOutput {
    pub fn new(bytes: Vec<u8>, limits: ConformanceLimits) -> Result<Self, SpecProblem> {
        if bytes.len() > limits.max_observation_bytes {
            return Err(SpecProblem::LimitExceeded("driver output"));
        }
        Ok(Self {
            bytes,
            oracle_receipt_digest: None,
        })
    }

    pub fn with_oracle_receipt(
        bytes: Vec<u8>,
        oracle_receipt_digest: impl Into<String>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        let mut output = Self::new(bytes, limits)?;
        let receipt = oracle_receipt_digest.into();
        validate_digest(&receipt)?;
        output.oracle_receipt_digest = Some(receipt);
        Ok(output)
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationCheck {
    pub matched: bool,
    pub expected: String,
    pub actual: String,
}

impl ObservationCheck {
    pub fn new(
        matched: bool,
        expected: impl Into<String>,
        actual: impl Into<String>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        let expected = expected.into();
        let actual = actual.into();
        validate_text(&expected, limits.max_observation_bytes)?;
        validate_text(&actual, limits.max_observation_bytes)?;
        Ok(Self {
            matched,
            expected,
            actual,
        })
    }
}

pub trait ConformanceDriver: Send + Sync {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String>;
}

pub trait ConformancePredicate: Send + Sync {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String>;
}

pub trait ConformanceObservation: Send + Sync {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String>;
}

pub struct RuntimeRegistry<'a> {
    drivers: BTreeMap<DriverRef, &'a dyn ConformanceDriver>,
    predicates: BTreeMap<PredicateRef, &'a dyn ConformancePredicate>,
    observations: BTreeMap<ObservationRef, &'a dyn ConformanceObservation>,
}

impl<'a> RuntimeRegistry<'a> {
    pub fn new(
        spec: &CompiledSpec,
        drivers: Vec<(DriverRef, &'a dyn ConformanceDriver)>,
        predicates: Vec<(PredicateRef, &'a dyn ConformancePredicate)>,
        observations: Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        if drivers.len() > limits.max_registry_entries
            || predicates.len() > limits.max_registry_entries
            || observations.len() > limits.max_registry_entries
        {
            return Err(SpecProblem::LimitExceeded("runtime registry entries"));
        }
        let drivers = runtime_map(drivers, "driver")?;
        let predicates = runtime_map(predicates, "predicate")?;
        let observations = runtime_map(observations, "observation")?;
        require_runtime_closure("driver", spec.registries.drivers(), drivers.keys())?;
        require_runtime_closure("predicate", spec.registries.predicates(), predicates.keys())?;
        require_runtime_closure(
            "observation",
            spec.registries.observations(),
            observations.keys(),
        )?;
        Ok(Self {
            drivers,
            predicates,
            observations,
        })
    }
}

fn runtime_map<'a, K: Ord + Clone + std::fmt::Display, V: ?Sized>(
    values: Vec<(K, &'a V)>,
    kind: &str,
) -> Result<BTreeMap<K, &'a V>, SpecProblem> {
    let mut result = BTreeMap::new();
    for (key, value) in values {
        if result.insert(key.clone(), value).is_some() {
            return Err(SpecProblem::DuplicateRegistryRef(format!("{kind}:{key}")));
        }
    }
    Ok(result)
}

fn require_runtime_closure<'a, T: Ord + std::fmt::Display + 'a>(
    kind: &str,
    declared: &BTreeSet<T>,
    actual: impl Iterator<Item = &'a T>,
) -> Result<(), SpecProblem> {
    let actual = actual.collect::<BTreeSet<_>>();
    let declared_refs = declared.iter().collect::<BTreeSet<_>>();
    if actual == declared_refs {
        Ok(())
    } else {
        Err(SpecProblem::RuntimeRegistryIncomplete(format!(
            "{kind}: declared={} bound={}",
            declared.len(),
            actual.len()
        )))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerContext {
    candidate_digest: String,
    environment_class: String,
}

impl RunnerContext {
    pub fn new(
        candidate_digest: impl Into<String>,
        environment_class: impl Into<String>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        let candidate_digest = candidate_digest.into();
        let environment_class = environment_class.into();
        validate_digest(&candidate_digest)?;
        validate_token(&environment_class, limits)?;
        Ok(Self {
            candidate_digest,
            environment_class,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerSelection {
    Focused {
        subsystem: String,
        gate: Option<CoverageGate>,
        shard: Option<u16>,
    },
    Replay(TestId),
}

impl RunnerSelection {
    pub fn focused(
        subsystem: impl Into<String>,
        gate: Option<CoverageGate>,
        shard: Option<u16>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        let subsystem = subsystem.into();
        validate_token(&subsystem, limits)?;
        Ok(Self::Focused {
            subsystem,
            gate,
            shard,
        })
    }

    pub fn replay(
        test_id: impl Into<String>,
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        Ok(Self::Replay(TestId::new(test_id, limits)?))
    }

    fn includes(&self, spec: &CompiledSpec, key: &BindingKey, case: &ConformanceCase) -> bool {
        match self {
            Self::Focused {
                subsystem,
                gate,
                shard,
            } => {
                let Some(row) = spec.catalog_rows.get(&key.row_id) else {
                    return false;
                };
                let key_shard = spec
                    .expected_shards
                    .iter()
                    .find_map(|(shard_key, bindings)| bindings.contains(key).then_some(shard_key));
                row.subsystem == *subsystem
                    && gate.is_none_or(|selected| selected == key.gate)
                    && shard.is_none_or(|selected| {
                        key_shard.is_some_and(|value| value.bucket == selected)
                    })
            }
            Self::Replay(test_id) => case.test_id == *test_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerdictBatch {
    pub shard: ShardKey,
    pub events: Vec<VerdictEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceRunReport {
    pub batches: Vec<VerdictBatch>,
    pub ledger: DerivedConformanceLedger,
}

pub struct ConformanceRunner<'a> {
    spec: &'a CompiledSpec,
    runtime: RuntimeRegistry<'a>,
    limits: ConformanceLimits,
}

impl<'a> ConformanceRunner<'a> {
    #[must_use]
    pub fn new(
        spec: &'a CompiledSpec,
        runtime: RuntimeRegistry<'a>,
        limits: ConformanceLimits,
    ) -> Self {
        Self {
            spec,
            runtime,
            limits,
        }
    }

    pub fn run(
        &self,
        selection: &RunnerSelection,
        context: &RunnerContext,
    ) -> Result<ConformanceRunReport, SpecProblem> {
        let selected = self
            .spec
            .cases
            .iter()
            .filter(|(key, case)| selection.includes(self.spec, key, case))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Err(SpecProblem::RuntimeFailure(
                "selection has no executable bindings".into(),
            ));
        }
        let mut batches = BTreeMap::<ShardKey, Vec<VerdictEvent>>::new();
        for (key, case) in selected {
            let shard = self
                .spec
                .expected_shards
                .iter()
                .find_map(|(shard, bindings)| bindings.contains(key).then_some(shard.clone()))
                .ok_or(SpecProblem::IncompleteShardSet)?;
            let event = self.run_case(case, &shard, context)?;
            batches.entry(shard).or_default().push(event);
        }
        let mut batches = batches
            .into_iter()
            .map(|(shard, mut events)| {
                events.sort_by(|left, right| left.key.cmp(&right.key));
                VerdictBatch { shard, events }
            })
            .collect::<Vec<_>>();
        batches.sort_by(|left, right| left.shard.cmp(&right.shard));
        validate_verdict_batches(self.spec, selection, &batches)?;
        let events = batches
            .iter()
            .flat_map(|batch| batch.events.iter().cloned())
            .collect();
        let ledger = DerivedConformanceLedger::derive_partial(self.spec, events)?;
        Ok(ConformanceRunReport { batches, ledger })
    }

    fn run_case(
        &self,
        case: &ConformanceCase,
        shard: &ShardKey,
        context: &RunnerContext,
    ) -> Result<VerdictEvent, SpecProblem> {
        let fixture_digest = self
            .spec
            .registries
            .fixture_digest(&case.input)
            .ok_or_else(|| SpecProblem::UnknownRegistryRef(case.input.to_string()))?;
        let oracle_digest = case
            .oracle
            .as_ref()
            .map(|oracle| {
                self.spec
                    .registries
                    .oracle_digest(oracle)
                    .ok_or_else(|| SpecProblem::UnknownRegistryRef(oracle.to_string()))
            })
            .transpose()?;
        let cache = CacheIdentity::new(
            CacheIdentityInput {
                candidate_digest: &context.candidate_digest,
                catalog_digest: self.spec.catalog_digest(),
                spec_digest: self.spec.spec_digest(),
                runner_version: CONFORMANCE_RUNNER_VERSION_V1,
                fixture_digest,
                oracle_digest,
                environment_class: &context.environment_class,
                shard,
                binding: &case.key,
            },
            self.limits,
        )?;
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        let mut matched = true;
        let mut output = None;
        for predicate in &case.preconditions {
            let handler = self.runtime.predicates.get(predicate).ok_or_else(|| {
                SpecProblem::RuntimeRegistryIncomplete(format!("predicate:{predicate}"))
            })?;
            match catch_unwind(AssertUnwindSafe(|| handler.evaluate(&case.input))) {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => {
                    matched = false;
                    expected.push(format!("precondition:{predicate}=true"));
                    actual.push(format!("precondition:{predicate}=false"));
                }
                Ok(Err(problem)) => {
                    matched = false;
                    expected.push(format!("precondition:{predicate}=true"));
                    actual.push(format!("precondition:{predicate}=error:{problem}"));
                }
                Err(_) => {
                    matched = false;
                    expected.push(format!("precondition:{predicate}=true"));
                    actual.push(format!("precondition:{predicate}=panic"));
                }
            }
        }
        if matched {
            let driver = self.runtime.drivers.get(&case.driver).ok_or_else(|| {
                SpecProblem::RuntimeRegistryIncomplete(format!("driver:{}", case.driver))
            })?;
            match catch_unwind(AssertUnwindSafe(|| driver.execute(&case.input))) {
                Ok(Ok(value)) => output = Some(value),
                Ok(Err(problem)) => {
                    matched = false;
                    expected.push("driver=success".into());
                    actual.push(format!("driver=error:{problem}"));
                }
                Err(_) => {
                    matched = false;
                    expected.push("driver=success".into());
                    actual.push("driver=panic".into());
                }
            }
        }
        if let Some(output) = &output {
            for observation in &case.expected {
                let handler = self.runtime.observations.get(observation).ok_or_else(|| {
                    SpecProblem::RuntimeRegistryIncomplete(format!("observation:{observation}"))
                })?;
                match catch_unwind(AssertUnwindSafe(|| handler.evaluate(output))) {
                    Ok(Ok(check)) => {
                        matched &= check.matched;
                        expected.push(format!("{observation}:{}", check.expected));
                        actual.push(format!("{observation}:{}", check.actual));
                    }
                    Ok(Err(problem)) => {
                        matched = false;
                        expected.push(format!("{observation}=evaluated"));
                        actual.push(format!("{observation}=error:{problem}"));
                    }
                    Err(_) => {
                        matched = false;
                        expected.push(format!("{observation}=evaluated"));
                        actual.push(format!("{observation}=panic"));
                    }
                }
            }
        }
        let expected = bounded_projection(&expected, self.limits.max_observation_bytes);
        let actual = bounded_projection(&actual, self.limits.max_observation_bytes);
        let mut observation_hash = Sha256::new();
        digest_field(&mut observation_hash, expected.as_bytes());
        digest_field(&mut observation_hash, actual.as_bytes());
        if let Some(output) = &output {
            digest_field(&mut observation_hash, output.bytes());
        }
        let observation_digest = format!("sha256:{:x}", observation_hash.finalize());
        let catalog = self
            .spec
            .catalog_rows
            .get(&case.key.row_id)
            .ok_or_else(|| SpecProblem::UnknownRow(case.key.row_id.to_string()))?;
        VerdictEvent::new(
            self.spec.spec_version(),
            case.key.clone(),
            case.test_id.clone(),
            if matched {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            observation_digest,
            cache.identity,
            catalog.source_locator.clone(),
            case.driver.clone(),
            case.input.clone(),
            expected,
            actual,
            output.and_then(|value| value.oracle_receipt_digest),
            self.limits,
        )
    }
}

pub fn validate_verdict_batches(
    spec: &CompiledSpec,
    selection: &RunnerSelection,
    batches: &[VerdictBatch],
) -> Result<(), SpecProblem> {
    let expected = spec
        .expected_shards
        .iter()
        .filter_map(|(shard, keys)| {
            let selected = keys
                .iter()
                .filter(|key| {
                    spec.cases
                        .get(*key)
                        .is_some_and(|case| selection.includes(spec, key, case))
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            (!selected.is_empty()).then_some((shard.clone(), selected))
        })
        .collect::<BTreeMap<_, _>>();
    let mut actual = BTreeMap::<ShardKey, BTreeSet<BindingKey>>::new();
    for batch in batches {
        let entry = actual.entry(batch.shard.clone()).or_default();
        for event in &batch.events {
            if !entry.insert(event.key.clone()) {
                return Err(SpecProblem::DuplicateVerdict(format_binding(&event.key)));
            }
        }
    }
    if actual == expected {
        Ok(())
    } else {
        Err(SpecProblem::IncompleteShardSet)
    }
}

fn bounded_projection(values: &[String], max_bytes: usize) -> String {
    let joined = values.join(" | ");
    let mut output = String::new();
    let mut truncated = false;
    for character in joined.chars() {
        let character = if character.is_control() {
            '�'
        } else {
            character
        };
        if output.len() + character.len_utf8() > max_bytes.saturating_sub(3) {
            truncated = true;
            break;
        }
        output.push(character);
    }
    if truncated {
        output.push_str("...");
    }
    if output.is_empty() {
        output.push_str("none");
    }
    output
}

#[cfg(test)]
mod conformance_tests {
    use super::*;
    use serde_json::{Value, json};

    const CATALOG_DIGEST: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const CANDIDATE_DIGEST: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const FIXTURE_DIGEST: &str =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn limits() -> ConformanceLimits {
        ConformanceLimits::default()
    }

    fn catalog() -> Vec<OfficialCatalogRow> {
        vec![
            OfficialCatalogRow::new(
                "official:mock:0001",
                "mock",
                "family",
                "official-page:1",
                [CoverageGate::Recognized, CoverageGate::Validated],
                limits(),
            )
            .unwrap(),
        ]
    }

    fn document() -> Value {
        json!({
            "schema_version": CONFORMANCE_SPEC_DOCUMENT_CONTRACT,
            "spec_version": CONFORMANCE_SPEC_VERSION_V1,
            "catalog_digest": CATALOG_DIGEST,
            "shard_count": 8,
            "registries": {
                "operations": ["mock.operation"],
                "input_shapes": ["mock.input"],
                "predicates": ["fixture.ready"],
                "transitions": ["mock.transition"],
                "observations": ["output.exact"],
                "conditions": [],
                "recoveries": [],
                "oracles": [],
                "drivers": ["mock.driver"],
                "fixtures": [{"id": "mock.fixture", "digest": FIXTURE_DIGEST}]
            },
            "rows": [{
                "row_id": "official:mock:0001",
                "operation": "mock.operation",
                "input": "mock.input",
                "preconditions": ["fixture.ready"],
                "transition": "mock.transition",
                "postconditions": ["output.exact"],
                "conditions": [],
                "recovery": null,
                "oracle": null,
                "applicable_gates": ["recognized", "validated"],
                "obligations": ["valid-form", "invalid-form"]
            }],
            "obligations": [
                {"row_id": "official:mock:0001", "obligation_id": "valid-form", "applicable_gates": ["recognized"]},
                {"row_id": "official:mock:0001", "obligation_id": "invalid-form", "applicable_gates": ["validated"]}
            ],
            "cases": [
                {
                    "spec_version": CONFORMANCE_SPEC_VERSION_V1,
                    "row_id": "official:mock:0001",
                    "obligation_id": "valid-form",
                    "gate": "recognized",
                    "test_id": "mock.valid",
                    "driver": "mock.driver",
                    "input": "mock.fixture",
                    "preconditions": ["fixture.ready"],
                    "expected": ["output.exact"],
                    "recovery": null,
                    "oracle": null
                },
                {
                    "spec_version": CONFORMANCE_SPEC_VERSION_V1,
                    "row_id": "official:mock:0001",
                    "obligation_id": "invalid-form",
                    "gate": "validated",
                    "test_id": "mock.invalid",
                    "driver": "mock.driver",
                    "input": "mock.fixture",
                    "preconditions": ["fixture.ready"],
                    "expected": ["output.exact"],
                    "recovery": null,
                    "oracle": null
                }
            ]
        })
    }

    fn compile(value: &Value) -> Result<CompiledSpec, SpecProblem> {
        CompiledSpec::compile_json(
            CATALOG_DIGEST,
            catalog(),
            &serde_json::to_vec(value).unwrap(),
            limits(),
        )
    }

    struct Echo;
    impl ConformanceDriver for Echo {
        fn execute(&self, _fixture: &FixtureRef) -> Result<DriverOutput, String> {
            DriverOutput::new(b"ok".to_vec(), limits()).map_err(|error| error.to_string())
        }
    }
    struct GenericSuccessMutant;
    impl ConformanceDriver for GenericSuccessMutant {
        fn execute(&self, _fixture: &FixtureRef) -> Result<DriverOutput, String> {
            DriverOutput::new(b"generic-success".to_vec(), limits())
                .map_err(|error| error.to_string())
        }
    }
    struct OversizedFailure;
    impl ConformanceDriver for OversizedFailure {
        fn execute(&self, _fixture: &FixtureRef) -> Result<DriverOutput, String> {
            Err(format!("{}\nsecret", "x".repeat(100_000)))
        }
    }
    struct Ready;
    impl ConformancePredicate for Ready {
        fn evaluate(&self, _fixture: &FixtureRef) -> Result<bool, String> {
            Ok(true)
        }
    }
    struct Exact;
    impl ConformanceObservation for Exact {
        fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
            ObservationCheck::new(
                output.bytes() == b"ok",
                "bytes=6f6b",
                format!("bytes={}", hex(output.bytes())),
                limits(),
            )
            .map_err(|error| error.to_string())
        }
    }
    static ECHO: Echo = Echo;
    static GENERIC_SUCCESS: GenericSuccessMutant = GenericSuccessMutant;
    static OVERSIZED_FAILURE: OversizedFailure = OversizedFailure;
    static READY: Ready = Ready;
    static EXACT: Exact = Exact;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn runtime<'a>(spec: &CompiledSpec, driver: &'a dyn ConformanceDriver) -> RuntimeRegistry<'a> {
        RuntimeRegistry::new(
            spec,
            vec![(DriverRef::new("mock.driver", limits()).unwrap(), driver)],
            vec![(
                PredicateRef::new("fixture.ready", limits()).unwrap(),
                &READY,
            )],
            vec![(
                ObservationRef::new("output.exact", limits()).unwrap(),
                &EXACT,
            )],
            limits(),
        )
        .unwrap()
    }

    fn selection() -> RunnerSelection {
        RunnerSelection::focused("mock", None, None, limits()).unwrap()
    }

    fn context() -> RunnerContext {
        RunnerContext::new(CANDIDATE_DIGEST, "local", limits()).unwrap()
    }

    #[test]
    fn spec_compiler_closes_rows_obligations_bindings_and_registries() {
        let spec = compile(&document()).unwrap();
        assert_eq!(spec.rows().count(), 1);
        assert_eq!(spec.obligations().count(), 2);
        assert_eq!(spec.cases().count(), 2);
        assert!(!spec.expected_shards().is_empty());
    }

    #[test]
    fn spec_compiler_rejects_unknown_stale_duplicate_and_missing_bindings() {
        let mut unknown = document();
        unknown["rows"][0]["row_id"] = json!("official:mock:missing");
        assert!(matches!(compile(&unknown), Err(SpecProblem::UnknownRow(_))));

        let mut stale = document();
        stale["cases"][0]["spec_version"] = json!("mainframe-env.conformance-ir@0");
        assert!(matches!(
            compile(&stale),
            Err(SpecProblem::StaleSpecVersion(_))
        ));

        let mut duplicate = document();
        let case = duplicate["cases"][0].clone();
        duplicate["cases"].as_array_mut().unwrap().push(case);
        assert!(matches!(
            compile(&duplicate),
            Err(SpecProblem::DuplicateBinding(_))
        ));

        let mut missing = document();
        missing["cases"].as_array_mut().unwrap().pop();
        assert!(matches!(
            compile(&missing),
            Err(SpecProblem::MissingBinding(_))
        ));

        let mut incompatible = document();
        incompatible["obligations"][0]["applicable_gates"] = json!(["executed"]);
        assert!(matches!(
            compile(&incompatible),
            Err(SpecProblem::IncompatibleGate(_))
        ));

        let mut manual_count = document();
        manual_count["pass_count"] = json!(2);
        assert!(matches!(
            compile(&manual_count),
            Err(SpecProblem::MalformedDocument(_))
        ));
    }

    #[test]
    fn runner_emits_replayable_verdicts_and_ledger_is_event_derived() {
        let spec = compile(&document()).unwrap();
        let runner = ConformanceRunner::new(&spec, runtime(&spec, &ECHO), limits());
        let report = runner.run(&selection(), &context()).unwrap();
        assert_eq!(
            report
                .batches
                .iter()
                .flat_map(|batch| &batch.events)
                .filter(|event| event.verdict == Verdict::Pass)
                .count(),
            2
        );
        assert!(
            report
                .batches
                .iter()
                .flat_map(|batch| &batch.events)
                .all(|event| event
                    .replay
                    .starts_with("cargo xtask conformance --replay mock.")
                    && event.source_locator == "official-page:1")
        );
        let first = &report.batches[0].events[0];
        let projection: Value = serde_json::from_slice(&first.canonical_json().unwrap()).unwrap();
        assert_eq!(
            projection["schema_version"],
            json!(CONFORMANCE_VERDICT_CONTRACT)
        );
        assert_eq!(first.identity().unwrap().len(), 71);
        let ledger: Value =
            serde_json::from_slice(&report.ledger.canonical_json().unwrap()).unwrap();
        assert_eq!(ledger["schema_version"], json!(CONFORMANCE_LEDGER_CONTRACT));
        assert_eq!(report.ledger.counts[&CoverageGate::Recognized].pass, 1);
        assert_eq!(report.ledger.counts[&CoverageGate::Validated].pass, 1);
    }

    #[test]
    fn generic_success_and_byte_mutants_are_killed_by_independent_observation() {
        let spec = compile(&document()).unwrap();
        let runner = ConformanceRunner::new(&spec, runtime(&spec, &GENERIC_SUCCESS), limits());
        let report = runner.run(&selection(), &context()).unwrap();
        assert!(
            report
                .batches
                .iter()
                .flat_map(|batch| &batch.events)
                .all(|event| event.verdict == Verdict::Fail
                    && event.actual.contains("67656e65726963"))
        );
        assert_eq!(report.ledger.counts[&CoverageGate::Recognized].fail, 1);
        assert_eq!(report.ledger.counts[&CoverageGate::Validated].fail, 1);
    }

    #[test]
    fn runtime_failures_still_emit_bounded_replayable_events() {
        let spec = compile(&document()).unwrap();
        let runner = ConformanceRunner::new(&spec, runtime(&spec, &OVERSIZED_FAILURE), limits());
        let report = runner.run(&selection(), &context()).unwrap();
        assert!(
            report
                .batches
                .iter()
                .flat_map(|batch| &batch.events)
                .all(|event| {
                    event.verdict == Verdict::Fail
                        && event.actual.len() <= limits().max_observation_bytes
                        && !event.actual.contains('\n')
                        && event.replay.starts_with("cargo xtask conformance --replay")
                })
        );
    }

    #[test]
    fn omitted_shards_and_verdicts_fail_closed() {
        let spec = compile(&document()).unwrap();
        let runner = ConformanceRunner::new(&spec, runtime(&spec, &ECHO), limits());
        let mut report = runner.run(&selection(), &context()).unwrap();
        report.batches.pop();
        assert_eq!(
            validate_verdict_batches(&spec, &selection(), &report.batches),
            Err(SpecProblem::IncompleteShardSet)
        );
        assert!(matches!(
            DerivedConformanceLedger::derive_complete(&spec, Vec::new()),
            Err(SpecProblem::MissingVerdict(_))
        ));
    }

    #[test]
    fn runtime_registry_must_close_every_declared_handler() {
        let spec = compile(&document()).unwrap();
        let problem = RuntimeRegistry::new(
            &spec,
            Vec::new(),
            vec![(
                PredicateRef::new("fixture.ready", limits()).unwrap(),
                &READY as &dyn ConformancePredicate,
            )],
            vec![(
                ObservationRef::new("output.exact", limits()).unwrap(),
                &EXACT as &dyn ConformanceObservation,
            )],
            limits(),
        );
        assert!(matches!(
            problem,
            Err(SpecProblem::RuntimeRegistryIncomplete(_))
        ));
    }

    #[test]
    fn cache_identity_changes_when_any_execution_identity_changes() {
        let spec = compile(&document()).unwrap();
        let (shard, keys) = spec.expected_shards().first_key_value().unwrap();
        let binding = keys.first().unwrap();
        let base = CacheIdentityInput {
            candidate_digest: CANDIDATE_DIGEST,
            catalog_digest: spec.catalog_digest(),
            spec_digest: spec.spec_digest(),
            runner_version: CONFORMANCE_RUNNER_VERSION_V1,
            fixture_digest: FIXTURE_DIGEST,
            oracle_digest: None,
            environment_class: "local",
            shard,
            binding,
        };
        let original = CacheIdentity::new(base.clone(), limits()).unwrap();
        let changed = CacheIdentity::new(
            CacheIdentityInput {
                candidate_digest:
                    "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                ..base
            },
            limits(),
        )
        .unwrap();
        assert_ne!(original, changed);

        let mut wrong_shard = shard.clone();
        wrong_shard.bucket = (wrong_shard.bucket + 1) % wrong_shard.bucket_count;
        assert!(matches!(
            CacheIdentity::new(
                CacheIdentityInput {
                    candidate_digest: CANDIDATE_DIGEST,
                    catalog_digest: spec.catalog_digest(),
                    spec_digest: spec.spec_digest(),
                    runner_version: CONFORMANCE_RUNNER_VERSION_V1,
                    fixture_digest: FIXTURE_DIGEST,
                    oracle_digest: None,
                    environment_class: "local",
                    shard: &wrong_shard,
                    binding,
                },
                limits(),
            ),
            Err(SpecProblem::UnsafeCacheKey(_))
        ));
    }

    #[test]
    fn differential_pass_requires_oracle_receipt() {
        let key = BindingKey {
            row_id: OfficialRowId::new("official:mock:0001", limits()).unwrap(),
            obligation_id: ObligationId::new("oracle", limits()).unwrap(),
            gate: CoverageGate::Differential,
        };
        assert_eq!(
            VerdictEvent::new(
                CONFORMANCE_SPEC_VERSION_V1,
                key,
                TestId::new("oracle.test", limits()).unwrap(),
                Verdict::Pass,
                CATALOG_DIGEST,
                CANDIDATE_DIGEST,
                "official-page:1",
                DriverRef::new("mock.driver", limits()).unwrap(),
                FixtureRef::new("mock.fixture", limits()).unwrap(),
                "expected",
                "actual",
                None,
                limits(),
            ),
            Err(SpecProblem::OracleReceiptRequired)
        );
    }
}
