//! Real compilation of signed raw sources and an explicit local request commitment.
//! Generic signed Resource bytes authenticate metadata identity through the actual SourceId.
use super::*;
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompilerRequest, CompilerResult, CompilerService,
};
use mainframe_env_source::SourceLimits;

/// One exact typed request and its complete signed raw Source-entry mapping.
/// Format, encoding, options, provenance and COPY libraries are supplied as a typed
/// SourceBundle and must match the signed local Resource identity commitment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSourceCompilation {
    /// Program definition whose entire real publisher result must match.
    pub program: String,
    /// Immutable full source closure and actual executable compiler request.
    pub request: CompilerRequest,
    /// Every logical source file path mapped to its signed Source manifest entry.
    pub source_entries: BTreeMap<String, String>,
}

const SOURCE_COMMITMENT: &str = "mainframe-env.application-local-source-commitment@1";

#[derive(Serialize)]
struct SourceCommitment<'a> {
    contract: &'static str,
    program: String,
    source_id: String,
    mode: &'static str,
    target: &'a str,
    options: &'a BTreeMap<String, String>,
    source_entries: &'a BTreeMap<String, String>,
}

impl LocalSourceCompilation {
    /// Exact generic Resource entry path required by this finite local profile.
    pub fn resource_path(&self) -> Result<String, HostProblem> {
        Ok(format!(
            "source-commitment/{}",
            normalize_online_name(&self.program, 128)?
        ))
    }

    /// Canonical bounded bytes to sign as that Resource in the genuine package.
    /// This authenticates the actual SourceId and full request declaration;
    /// reconstructing a SourceBundle still requires the supplied typed input.
    pub fn resource_bytes(&self) -> Result<Vec<u8>, HostProblem> {
        source_input_bytes(std::slice::from_ref(self))?;
        encode_commitment(self)
    }
}

fn encode_commitment(input: &LocalSourceCompilation) -> Result<Vec<u8>, HostProblem> {
    if input.request.mode != CompilationMode::Executable {
        return Err(HostProblem::Unsupported);
    }
    let bytes = serde_json::to_vec(&SourceCommitment {
        contract: SOURCE_COMMITMENT,
        program: normalize_online_name(&input.program, 128)?,
        source_id: input.request.source.id().to_hex(),
        mode: "executable",
        target: input.request.target.as_str(),
        options: input.request.options.values(),
        source_entries: &input.source_entries,
    })
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if bytes.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(bytes)
}

pub(super) fn source_input_bytes(inputs: &[LocalSourceCompilation]) -> Result<usize, HostProblem> {
    let limits = SourceLimits::default();
    let mut charged = 0usize;
    for input in inputs {
        let source = &input.request.source;
        if input.program.len() > 128
            || source.files().is_empty()
            || source.files().len() > limits.max_files
            || source.total_bytes() > limits.max_total_bytes
            || source.options().len() > limits.max_options
            || source.provenance().len() > limits.max_provenance_edges
            || source.libraries().len() > limits.max_files
            || input.source_entries.len() != source.files().len()
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut charge = |bytes: usize| -> Result<(), HostProblem> {
            charged = charged
                .checked_add(bytes)
                .ok_or(HostProblem::ResourceExhausted)?;
            if charged > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
                return Err(HostProblem::ResourceExhausted);
            }
            Ok(())
        };
        charge(input.program.len())?;
        charge(input.request.target.as_str().len())?;
        for file in source.files() {
            if file.path().as_str().len() > limits.max_path_bytes
                || file.bytes().len() > limits.max_file_bytes
            {
                return Err(HostProblem::ResourceExhausted);
            }
            charge(file.path().as_str().len())?;
            charge(file.bytes().len())?;
        }
        for (key, value) in source.options() {
            if key.len() > limits.max_option_bytes || value.len() > limits.max_option_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
            charge(key.len())?;
            charge(value.len())?;
        }
        for (key, value) in input.request.options.values() {
            charge(key.len())?;
            charge(value.len())?;
        }
        charge(
            source
                .provenance()
                .len()
                .checked_mul(64)
                .ok_or(HostProblem::ResourceExhausted)?,
        )?;
        for library in source.libraries() {
            if library.name().len() > limits.max_path_bytes
                || library.members().len() > limits.max_files
            {
                return Err(HostProblem::ResourceExhausted);
            }
            charge(library.name().len())?;
            for member in library.members() {
                charge(member.as_str().len())?;
            }
        }
        for (logical, entry) in &input.source_entries {
            if logical.len() > limits.max_path_bytes || entry.len() > limits.max_path_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
            charge(logical.len())?;
            charge(entry.len())?;
        }
        // The additional canonical Resource copy remains inside the original
        // aggregate source/seed/program admission; no budget is increased.
        charge(encode_commitment(input)?.len())?;
    }
    Ok(charged)
}

pub(super) fn compile_sources(
    package: &ApplicationPackageV2,
    programs: &[OnlineProgramDefinition],
    inputs: Vec<LocalSourceCompilation>,
    used: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, String>, HostProblem> {
    if inputs.len() != programs.len() || inputs.is_empty() {
        return Err(HostProblem::Malformed);
    }
    let mut compiled = BTreeMap::new();
    for input in inputs {
        let name = normalize_online_name(&input.program, 128)?;
        let offered = programs
            .iter()
            .find(|program| program.name.eq_ignore_ascii_case(&name))
            .ok_or(HostProblem::NotFound)?;
        if compiled.contains_key(&name) || input.request.mode != CompilationMode::Executable {
            return Err(HostProblem::IdempotencyConflict);
        }
        let program_path = format!("program/{name}");
        let source_paths = source_dependencies(package, &program_path)?;
        let mapped = input
            .source_entries
            .values()
            .cloned()
            .collect::<BTreeSet<_>>();
        let commitment_path = input.resource_path()?;
        let program_entry = package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| entry.path == program_path && entry.kind == EntryKind::Program)
            .ok_or(HostProblem::NotFound)?;
        let commitment_entry = package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| entry.path == commitment_path && entry.kind == EntryKind::Resource)
            .ok_or(HostProblem::NotFound)?;
        if !program_entry.depends_on.contains(&commitment_path)
            || commitment_entry.depends_on != mapped.iter().cloned().collect::<Vec<_>>()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let commitment = input.resource_bytes()?;
        bind_entry(
            package,
            EntryKind::Resource,
            &commitment_path,
            &commitment,
            used,
        )?;
        if mapped.len() != input.source_entries.len() || mapped != source_paths {
            return Err(HostProblem::IdempotencyConflict);
        }
        for file in input.request.source.files() {
            let path = input
                .source_entries
                .get(file.path().as_str())
                .ok_or(HostProblem::NotFound)?;
            let entry = package
                .base
                .manifest
                .entries
                .iter()
                .find(|entry| entry.path == *path && entry.kind == EntryKind::Source)
                .ok_or(HostProblem::NotFound)?;
            if entry.bytes != file.bytes().len()
                || entry.sha256 != format!("sha256:{:x}", Sha256::digest(file.bytes()))
                || package.base.blobs.get(&entry.sha256).map(Vec::as_slice) != Some(file.bytes())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            // An identical signed COPY input may participate in several programs.
            // Its bytes and dependency membership are checked for every request.
            used.insert(path.clone());
        }
        let source_id = input.request.source.id().to_hex();
        // CompilerRequest remains owned and unmodified. The actual compiler
        // verifies the exact SourceId before lowering and publishing it.
        let result = CobolCompiler::default()
            .compile(input.request)
            .map_err(|_| HostProblem::Malformed)?;
        let CompilerResult::Published { artifact, .. } = result else {
            return Err(HostProblem::Unsupported);
        };
        if OnlineProgramDefinition::current(&offered.name, &artifact) != *offered {
            return Err(HostProblem::IdempotencyConflict);
        }
        compiled.insert(name, source_id);
    }
    Ok(compiled)
}

fn source_dependencies(
    package: &ApplicationPackageV2,
    program: &str,
) -> Result<BTreeSet<String>, HostProblem> {
    let mut pending = vec![program];
    let mut visited = BTreeSet::new();
    let mut sources = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if !visited.insert(path) {
            continue;
        }
        let entry = package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .ok_or(HostProblem::NotFound)?;
        if entry.kind == EntryKind::Source {
            sources.insert(entry.path.clone());
        }
        pending.extend(entry.depends_on.iter().map(String::as_str));
    }
    if sources.is_empty() {
        return Err(HostProblem::NotFound);
    }
    Ok(sources)
}

// Local selected-source persistence uses the original provider row values. The public
// store record remains codec-neutral; serialization borrows payloads and decoding moves them.
#[derive(Serialize)]
struct SelectedProviderRowRef<'a> {
    namespace: &'a str,
    key: &'a str,
    version: u64,
    payload: &'a [u8],
}

#[derive(Deserialize)]
struct SelectedProviderRow {
    namespace: String,
    key: String,
    version: u64,
    payload: Vec<u8>,
}

pub(super) fn serialize_provider_rows<S>(
    rows: &[ProviderStateRecord],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.collect_seq(rows.iter().map(|row| SelectedProviderRowRef {
        namespace: &row.namespace,
        key: &row.key,
        version: row.version,
        payload: &row.payload,
    }))
}

pub(super) fn deserialize_provider_rows<'de, D>(
    deserializer: D,
) -> Result<Vec<ProviderStateRecord>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<SelectedProviderRow>::deserialize(deserializer).map(|rows| {
        rows.into_iter()
            .map(|row| ProviderStateRecord {
                namespace: row.namespace,
                key: row.key,
                version: row.version,
                payload: row.payload,
            })
            .collect()
    })
}
