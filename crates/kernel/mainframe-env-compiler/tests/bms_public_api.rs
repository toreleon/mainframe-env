use mainframe_env_compiler::{BmsMapset, BmsSymbolicProblem, CobolCompiler};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use std::collections::BTreeMap;

fn bundle(member_path: &str, member: &str) -> SourceBundle {
    let limits = SourceLimits::default();
    let primary = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
    let path = LogicalPath::new(member_path, limits.max_path_bytes).unwrap();
    let program = "IDENTIFICATION DIVISION. PROGRAM-ID. MAPCOPY.\n\
                   DATA DIVISION. WORKING-STORAGE SECTION. COPY TESTSET IN MAPS.\n\
                   PROCEDURE DIVISION. MOVE 1 TO FIELDL. MOVE 'A' TO FIELDO.\n\
                   DISPLAY FIELDI. STOP RUN.\n";
    let files = [(primary.as_str(), program), (path.as_str(), member)]
        .into_iter()
        .map(|(name, text)| {
            SourceFile::input(
                name,
                text.as_bytes().to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap()
        })
        .collect();
    SourceBundle::with_libraries(
        &primary,
        files,
        vec![SourceLibrary::new("MAPS", vec![path], limits).unwrap()],
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .unwrap()
}

#[test]
fn public_bms_mapset_copy_resolution_preserves_layout() {
    let source = "TESTSET DFHMSD TYPE=DSECT,LANG=COBOL2,STORAGE=AUTO,MODE=INOUT\n\
                  SCREEN DFHMDI SIZE=(24,80)\n\
                  FIELD DFHMDF LENGTH=1\n\
                   DFHMSD TYPE=FINAL\n";
    let mapset = BmsMapset::parse(source).unwrap();
    assert_eq!(mapset.name(), "TESTSET");
    assert_eq!(mapset.clone(), mapset);
    let copybook = mapset.symbolic_copybook().unwrap();
    assert_eq!(mapset.symbolic_copybook().unwrap(), copybook);
    let compiler = CobolCompiler::default();
    let from_bms = compiler.analyze(&bundle("maps/terminal.bms", source));
    let from_copybook = compiler.analyze(&bundle("maps/TESTSET.cpy", &copybook));
    assert!(
        from_bms.diagnostics.is_empty(),
        "{:?}",
        from_bms.diagnostics
    );
    assert!(
        from_copybook.diagnostics.is_empty(),
        "{:?}",
        from_copybook.diagnostics
    );
    assert_eq!(from_bms.completeness, from_copybook.completeness);
    let mut bms_layouts = from_bms.semantic.unwrap().layouts;
    let mut copybook_layouts = from_copybook.semantic.unwrap().layouts;
    // Provenance names the respective input; every storage property must agree.
    for layout in bms_layouts.iter_mut().chain(copybook_layouts.iter_mut()) {
        layout.source.clear();
    }
    assert_eq!(bms_layouts, copybook_layouts);
    let syntax = from_bms.syntax.unwrap();
    assert_eq!(syntax.expansions().len(), 1);
    assert_eq!(syntax.expansions()[0].source.as_str(), "maps/terminal.bms");
    assert!(syntax.semantic_origins().iter().any(|origin| {
        origin.source.as_str() == "maps/terminal.bms"
            && origin.source_start == 0
            && origin.source_end == source.len()
    }));
    assert!(matches!(
        BmsMapset::parse(&source.replace("LENGTH=1", "LENGTH=1,PICIN='9'")),
        Err(BmsSymbolicProblem::UnverifiedFeature(_))
    ));
}
