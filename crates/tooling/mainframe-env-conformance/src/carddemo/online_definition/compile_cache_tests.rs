//! Synthetic memo controls: no compiled application, IBM or journey credit.
use super::*;
use mainframe_env_source::{
    LogicalPath, ProvenanceEdgeInput, ProvenanceKind, SourceEncoding, SourceFile, SourceFormat,
    SourceLibrary, SourceLimits,
};
use std::cell::{Cell, RefCell};

fn source(variant: u8) -> SourceBundle {
    let limits = SourceLimits::default();
    let path = |name| LogicalPath::new(name, limits.max_path_bytes).unwrap();
    let primary = path(if variant == 3 {
        "other.cbl"
    } else {
        "fixture.cbl"
    });
    let a = path("first/BOOKA.cpy");
    let b = path("second/BOOKB.cpy");
    let format = if variant == 4 {
        SourceFormat::Fixed
    } else {
        SourceFormat::Free
    };
    let encoding = if variant == 5 {
        SourceEncoding::Ebcdic(37)
    } else {
        SourceEncoding::Utf8
    };
    let files = vec![
        SourceFile::input(
            primary.as_str(),
            if variant == 1 {
                b"OTHER".to_vec()
            } else {
                b"PRIMARY".to_vec()
            },
            format,
            encoding,
            limits,
        )
        .unwrap(),
        SourceFile::input(
            a.as_str(),
            if variant == 2 {
                b"CHANGED".to_vec()
            } else {
                b"COPYA".to_vec()
            },
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap(),
        SourceFile::input(
            b.as_str(),
            b"COPYB".to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap(),
    ];
    let mut libraries = vec![
        SourceLibrary::new(
            if variant == 9 { "renamed" } else { "first" },
            vec![a.clone()],
            limits,
        )
        .unwrap(),
        SourceLibrary::new("second", vec![b], limits).unwrap(),
    ];
    if variant == 6 {
        libraries.reverse();
    }
    let options = if variant == 7 {
        BTreeMap::from([("source-option".into(), "changed".into())])
    } else {
        BTreeMap::new()
    };
    let provenance = if variant == 8 {
        vec![ProvenanceEdgeInput {
            generated_path: primary.clone(),
            generated_bytes: 0..1,
            origin_path: a,
            origin_bytes: 0..1,
            kind: ProvenanceKind::Generated,
        }]
    } else {
        Vec::new()
    };
    SourceBundle::with_libraries(&primary, files, libraries, options, provenance, limits).unwrap()
}

fn key() -> Key {
    Key::selected(
        &CobolCompiler::default(),
        "FIXTURE",
        "fixture.cbl",
        &source(0),
    )
    .unwrap()
}

fn observed(
    memo: &mut Memo<Vec<u8>>,
    key: &Key,
    calls: &Cell<u8>,
    frames: &RefCell<Vec<CompilerRequest>>,
) -> Vec<u8> {
    memo.get_or_compile(key, || {
        calls.set(calls.get() + 1);
        frames.borrow_mut().push(key.request.clone());
        Ok::<_, ()>(vec![calls.get()])
    })
    .unwrap()
}

fn must_recompute(changed: Key) {
    let original = key();
    assert!(changed != original);
    let mut memo = Memo::new();
    let calls = Cell::new(0);
    let frames = RefCell::new(Vec::new());
    assert_eq!(observed(&mut memo, &original, &calls, &frames), vec![1]);
    assert_eq!(observed(&mut memo, &changed, &calls, &frames), vec![2]);
    assert_eq!(observed(&mut memo, &original, &calls, &frames), vec![1]);
    assert_eq!(calls.get(), 2);
    assert_eq!(*frames.borrow(), vec![original.request, changed.request]);
}

#[test]
fn exact_frames_compute_once_and_return_owned_clones() {
    let original = key();
    let mut offered = original.clone();
    let mut memo = Memo::new();
    let calls = Cell::new(0);
    let frames = RefCell::new(Vec::new());
    let mut result = observed(&mut memo, &offered, &calls, &frames);
    result[0] = 99;
    result.push(98);
    offered.name = "MUTATED".into();
    offered.request.source = source(1);
    assert_eq!(observed(&mut memo, &original, &calls, &frames), vec![1]);
    assert_eq!(calls.get(), 1);
    assert_eq!(*frames.borrow(), vec![original.request]);
    assert_eq!(memo.entries.len(), 1);
}

#[test]
fn all_source_bytes_metadata_and_ordered_libraries_recompute() {
    for variant in 1..=9 {
        let mut changed = key();
        changed.request.source = source(variant);
        must_recompute(changed);
    }
}

#[test]
fn name_primary_mode_target_and_compile_options_recompute() {
    for variant in 0..5 {
        let mut changed = key();
        match variant {
            0 => changed.name = "OTHER".into(),
            1 => changed.primary = "other.cbl".into(),
            2 => changed.request.mode = CompilationMode::Analyze,
            3 => changed.request.target = CompileTarget::new("other-reference").unwrap(),
            _ => {
                changed.request.options = CompileOptions::new(BTreeMap::from([(
                    "offered-option".into(),
                    "changed".into(),
                )]))
                .unwrap()
            }
        }
        must_recompute(changed);
    }
}

#[test]
fn every_compiler_limit_member_recomputes() {
    let changes: &[fn(&mut CobolCompilerLimits)] = &[
        |limits| limits.syntax.max_source_bytes -= 1,
        |limits| limits.syntax.max_tokens -= 1,
        |limits| limits.syntax.max_token_bytes -= 1,
        |limits| limits.syntax.max_lines -= 1,
        |limits| limits.syntax.max_expanded_bytes -= 1,
        |limits| limits.syntax.max_copy_depth -= 1,
        |limits| limits.syntax.max_directives -= 1,
        |limits| limits.syntax.max_directive_nesting -= 1,
        |limits| limits.syntax.max_compilation_variables -= 1,
        |limits| limits.syntax.max_replacements -= 1,
        |limits| limits.ir.max_regions -= 1,
        |limits| limits.ir.max_blocks -= 1,
        |limits| limits.ir.max_operations -= 1,
        |limits| limits.ir.max_values -= 1,
        |limits| limits.ir.max_storage_regions -= 1,
        |limits| limits.ir.max_storage_bytes -= 1,
        |limits| limits.ir.max_operands_per_operation -= 1,
        |limits| limits.ir.max_results_per_operation -= 1,
        |limits| limits.ir.max_attributes_per_operation -= 1,
        |limits| limits.ir.max_effects_per_operation -= 1,
        |limits| limits.ir.max_references_per_operation -= 1,
        |limits| limits.ir.max_string_bytes -= 1,
        |limits| limits.ir.max_attribute_bytes -= 1,
        |limits| limits.codec.max_envelope_bytes -= 1,
        |limits| limits.codec.max_payload_bytes -= 1,
        |limits| limits.codec.max_string_bytes -= 1,
        |limits| limits.codec.ir.max_regions -= 1,
        |limits| limits.codec.ir.max_blocks -= 1,
        |limits| limits.codec.ir.max_operations -= 1,
        |limits| limits.codec.ir.max_values -= 1,
        |limits| limits.codec.ir.max_storage_regions -= 1,
        |limits| limits.codec.ir.max_storage_bytes -= 1,
        |limits| limits.codec.ir.max_operands_per_operation -= 1,
        |limits| limits.codec.ir.max_results_per_operation -= 1,
        |limits| limits.codec.ir.max_attributes_per_operation -= 1,
        |limits| limits.codec.ir.max_effects_per_operation -= 1,
        |limits| limits.codec.ir.max_references_per_operation -= 1,
        |limits| limits.codec.ir.max_string_bytes -= 1,
        |limits| limits.codec.ir.max_attribute_bytes -= 1,
        |limits| limits.artifact.max_payload_bytes -= 1,
        |limits| limits.artifact.max_generation_bytes -= 1,
        |limits| limits.artifact.max_host_interfaces -= 1,
        |limits| limits.max_storage_bytes -= 1,
        |limits| limits.max_data_items -= 1,
        |limits| limits.max_diagnostics -= 1,
    ];
    for change in changes {
        let mut changed = key();
        change(&mut changed.limits);
        must_recompute(changed);
    }
}

#[test]
fn exact_catalogs_profile_and_contract_versions_recompute() {
    for variant in 0..10 {
        let mut changed = key();
        match variant {
            0 => changed.hir_catalog = OperationCatalog::default(),
            1 => changed.core_catalog = OperationCatalog::default(),
            2 => changed.profile.allowed_operations.clear(),
            3 => changed.profile.allowed_runtime_imports.clear(),
            4 => changed.compiler_generation = "other-compiler-generation",
            5 => changed.compiler_contract = "other.compiler@2",
            6 => changed.artifact_contract = "other.artifact@4",
            7 => changed.ir_contract = "other.ir@2",
            8 => changed.hir_dialects.push("unknown.hir@99"),
            _ => changed.core_dialect = "unknown.core@99",
        }
        must_recompute(changed);
    }
}

#[test]
fn failed_compilation_retains_neither_error_nor_stale_result() {
    let first = key();
    let mut different = first.clone();
    different.request.source = source(1);
    let mut memo: Memo<Vec<u8>> = Memo::new();
    let calls = Cell::new(0);
    let frames = RefCell::new(Vec::new());
    assert_eq!(observed(&mut memo, &first, &calls, &frames), vec![1]);
    for _ in 0..2 {
        let error = memo
            .get_or_compile(&different, || {
                calls.set(calls.get() + 1);
                Err::<Vec<u8>, _>("actual-compile-refusal")
            })
            .unwrap_err();
        assert_eq!(error, "actual-compile-refusal");
    }
    assert_eq!(calls.get(), 3);
    assert_eq!(memo.entries.len(), 1);
    assert_eq!(observed(&mut memo, &first, &calls, &frames), vec![1]);
    assert_eq!(observed(&mut memo, &different, &calls, &frames), vec![4]);
    assert_eq!(memo.entries.len(), 2);
}

#[test]
fn full_memo_preserves_actual_fallback_and_existing_hits() {
    let original = key();
    let mut memo = Memo::new();
    let calls = Cell::new(0);
    let frames = RefCell::new(Vec::new());
    for ordinal in 0..MAX_ENTRIES {
        let mut changed = original.clone();
        changed.name = format!("SLOT{ordinal}");
        assert_eq!(
            observed(&mut memo, &changed, &calls, &frames),
            vec![u8::try_from(ordinal + 1).unwrap()]
        );
    }
    assert_eq!(memo.entries.len(), MAX_ENTRIES);
    let mut extra = original.clone();
    extra.name = "NOT-RETAINED".into();
    assert_eq!(observed(&mut memo, &extra, &calls, &frames), vec![17]);
    assert_eq!(observed(&mut memo, &extra, &calls, &frames), vec![18]);
    assert_eq!(memo.entries.len(), MAX_ENTRIES);
    let mut stored = original;
    stored.name = "SLOT0".into();
    assert_eq!(observed(&mut memo, &stored, &calls, &frames), vec![1]);
    assert_eq!(calls.get(), 18);
}
