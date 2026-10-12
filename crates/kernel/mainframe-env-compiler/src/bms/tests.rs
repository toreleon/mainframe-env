use super::*;
use crate::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use std::path::Path;

fn source(options: &str, maps: &str) -> String {
    format!(
        "TESTSET DFHMSD TYPE=DSECT,LANG=COBOL2,STORAGE=AUTO,{options}\n{maps}\n DFHMSD TYPE=FINAL\n"
    )
}

fn bundle(program: &str, libraries: &[(&str, &[(&str, &str)])]) -> SourceBundle {
    let limits = SourceLimits::default();
    let primary = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
    let mut files = vec![
        SourceFile::input(
            "main.cbl",
            program.as_bytes().to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap(),
    ];
    let mut source_libraries = Vec::new();
    for (name, members) in libraries {
        let mut paths = Vec::new();
        for (path, content) in *members {
            files.push(
                SourceFile::input(
                    *path,
                    content.as_bytes().to_vec(),
                    SourceFormat::Free,
                    SourceEncoding::Utf8,
                    limits,
                )
                .unwrap(),
            );
            paths.push(LogicalPath::new(*path, limits.max_path_bytes).unwrap());
        }
        source_libraries.push(SourceLibrary::new(*name, paths, limits).unwrap());
    }
    SourceBundle::with_libraries(
        &primary,
        files,
        source_libraries,
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .unwrap()
}

fn program(copy: &str, procedure: &str) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. MAPCOPY.\nDATA DIVISION. WORKING-STORAGE SECTION.\nCOPY {copy}.\nPROCEDURE DIVISION. {procedure} STOP RUN.\n"
    )
}

#[test]
fn named_fields_have_length_flag_attribute_overlay_and_data_in_order() {
    let bms = source(
        "MODE=INOUT,TIOAPFX=YES",
        "SCREEN DFHMDI SIZE=(24,80)\nUSERID DFHMDF LENGTH=8,ATTRB=(UNPROT,IC,FSET)\n DFHMDF LENGTH=5,INITIAL='Text'\n DFHMDF LENGTH=0\n",
    );
    let mapset = BmsMapset::parse(&bms).unwrap();
    assert_eq!(mapset.name(), "TESTSET");
    let text = mapset.symbolic_copybook().unwrap();
    assert_eq!(
        text,
        "       01 SCREENI.\n           02 FILLER PIC X(12).\n           02 USERIDL COMP PIC S9(4).\n           02 USERIDF PIC X.\n           02 FILLER REDEFINES USERIDF.\n             03 USERIDA PIC X.\n           02 USERIDI PIC X(8).\n       01 SCREENO REDEFINES SCREENI.\n           02 FILLER PIC X(12).\n           02 FILLER PIC X(3).\n           02 USERIDO PIC X(8).\n"
    );
    assert_eq!(mapset.symbolic_copybook().unwrap(), text);
}

#[test]
fn mapset_options_inherit_and_map_prefix_extensions_override() {
    let bms = source(
        "MODE=INOUT,EXTATT=YES",
        "FIRST DFHMDI SIZE=(24,80)\nONE DFHMDF LENGTH=2,ATTRB=(ASKIP,NORM)\nSECOND DFHMDI SIZE=(24,80),TIOAPFX=NO,EXTATT=MAPONLY\nTWO DFHMDF LENGTH=3,ATTRB=(PROT,BRT)\n",
    );
    let text = BmsMapset::parse(&bms).unwrap().symbolic_copybook().unwrap();
    assert_eq!(text.matches("PIC X(12)").count(), 2);
    assert!(text.contains("01 SECONDI."));
    assert!(text.contains("01 SECONDO REDEFINES SECONDI."));
    let positions =
        ['C', 'P', 'H', 'V'].map(|suffix| text.find(&format!("ONE{suffix} PIC X.")).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!text.contains("TWOC"));
    let explicit = source(
        "MODE=INOUT,DSATTS=(VALIDN,HILIGHT,PS,COLOR),MAPATTS=(COLOR,PS,HILIGHT,VALIDN)",
        "FIRST DFHMDI SIZE=(24,80)\nONE DFHMDF LENGTH=2\n",
    );
    let text = BmsMapset::parse(&explicit)
        .unwrap()
        .symbolic_copybook()
        .unwrap();
    assert!(text.contains("02 FILLER PIC X(4)."));
    assert!(text.contains("02 ONEV PIC X."));
}

#[test]
fn input_and_output_modes_and_explicit_no_prefix() {
    for mode in ["IN", "OUT"] {
        let bms = source(
            &format!("MODE={mode},TIOAPFX=NO"),
            "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=1,ATTRB=(NUM,DRK)\n",
        );
        let text = BmsMapset::parse(&bms).unwrap().symbolic_copybook().unwrap();
        assert!(!text.contains("PIC X(12)"));
        assert!(!text.contains("REDEFINES SCREENI"));
        assert_eq!(text.contains("01 SCREENI."), mode == "IN");
        assert_eq!(text.contains("01 SCREENO."), mode == "OUT");
        assert!(text.contains("FIELDA PIC X."));
    }
}

#[test]
fn verified_numeric_and_edited_pictures_are_preserved_other_pictures_fail_closed() {
    let bms = source(
        "MODE=INOUT,TIOAPFX=YES",
        "SCREEN DFHMDI SIZE=(24,80)\nNUMBER DFHMDF LENGTH=11,PICIN='99999999999'\nAMOUNT DFHMDF LENGTH=15,PICOUT='+ZZZ,ZZZ,ZZZ.99'\n",
    );
    let text = BmsMapset::parse(&bms).unwrap().symbolic_copybook().unwrap();
    assert!(text.contains("NUMBERI PIC 99999999999."));
    assert!(text.contains("NUMBERO PIC X(11)."));
    assert!(text.contains("AMOUNTI PIC X(15)."));
    assert!(text.contains("AMOUNTO PIC +ZZZ,ZZZ,ZZZ.99."));
    for option in [
        "PICIN='9(11)'",
        "PICOUT='Z(10)9'",
        "PICOUT='+ZZZ,ZZZ,ZZZ.99'",
        "SOSI=YES",
        "OUTLINE=BOX",
        "TRANSP=YES",
        "DSATTS=(COLOR)",
    ] {
        let bms = source(
            "MODE=INOUT",
            &format!("SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=11,{option}\n"),
        );
        let error = BmsMapset::parse(&bms).unwrap_err();
        assert!(
            matches!(error, BmsSymbolicProblem::UnverifiedFeature(_)),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .contains(option.split('=').next().unwrap()),
            "{error}"
        );
    }
    for option in [
        "DSATTS=(OUTLINE)",
        "MAPATTS=(SOSI)",
        "DSECT=ADSL",
        "DATA=BLOCK",
        "BASE=OTHER",
        "LANG=ASM",
        "EXTATT=INVALID",
    ] {
        let bms = source(
            &format!("MODE=INOUT,{option}"),
            "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=1\n",
        );
        assert!(BmsMapset::parse(&bms).is_err(), "{option}");
    }
}

#[test]
fn fixed_column_continuations_literals_and_assembler_remarks_are_parsed() {
    let bms = format!(
        "{:<71}X\n TIOAPFX=YES\n* comment\nSCREEN DFHMDI SIZE=(24,80)\n{:<71}-\n ***'\n{:<71}*\n ATTRB=(UNPROT)\n DFHMDF LENGTH=4,INITIAL='Text'    'remark\n DFHMSD TYPE=FINAL\n",
        "TESTSET DFHMSD TYPE=MAP,LANG=COBOL,MODE=INOUT,STORAGE=AUTO,",
        " DFHMDF LENGTH=6,INITIAL='***",
        "DFHUSER DFHMDF LENGTH=8,",
    );
    let text = BmsMapset::parse(&bms).unwrap().symbolic_copybook().unwrap();
    assert!(text.contains("DFHUSERI PIC X(8)."));
    assert_eq!(text.matches("COMP PIC S9(4)").count(), 1);
}

#[test]
fn malformed_sources_and_inconsistent_extensions_are_rejected() {
    for maps in [
        "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=0",
        "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=1\nFIELD DFHMDF LENGTH=2",
        "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=BAD",
        "SCREEN DFHMDI SIZE=(24,80),DSATTS=(COLOR)",
        "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=1,PICIN='unterminated",
    ] {
        assert!(BmsMapset::parse(&source("MODE=INOUT", maps)).is_err());
    }
}

#[test]
fn mapset_name_discovery_does_not_parse_unrelated_operands() {
    let unrelated = "OTHER DFHMSD TYPE=MAP\nX MYMAC 1,2\n DFHMSD TYPE=FINAL\n";
    assert_eq!(
        source_mapset_name(unrelated).unwrap().as_deref(),
        Some("OTHER")
    );
}

#[test]
fn copy_requested_mapset_ignores_unsupported_unrelated_member() {
    let requested = source(
        "MODE=INOUT",
        "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=1\n",
    );
    let unrelated = "OTHER DFHMSD TYPE=MAP\nX MYMAC 1,2\n DFHMSD TYPE=FINAL\n";
    let main = program("TESTSET", "MOVE 'A' TO FIELDO.");
    let analysis = CobolCompiler::default().analyze(&bundle(
        &main,
        &[(
            "MAPS",
            &[("requested.bms", &requested), ("unrelated.bms", unrelated)],
        )],
    ));
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis.syntax.unwrap().semantic_text().contains("SCREENI"));
}

#[test]
fn copy_selected_mapset_with_unsupported_operands_fails_closed() {
    let selected = "TESTSET DFHMSD TYPE=MAP\nX MYMAC 1,2\n DFHMSD TYPE=FINAL\n";
    let main = program("TESTSET", "");
    let analysis = CobolCompiler::default()
        .analyze(&bundle(&main, &[("MAPS", &[("selected.bms", selected)])]));
    assert!(analysis.syntax.is_none());
    assert_eq!(analysis.diagnostics.len(), 1);
    assert!(
        analysis.diagnostics[0]
            .public_message()
            .contains("invalid BMS symbolic map: operand 1")
    );
}

#[test]
fn copybook_precedence_library_qualification_ambiguity_and_diagnostics() {
    let bms = source(
        "MODE=INOUT",
        "SCREEN DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=1\n",
    );
    let main = program("TESTSET IN MAPS", "MOVE 'A' TO FIELDO.");
    let analysis =
        CobolCompiler::default().analyze(&bundle(&main, &[("MAPS", &[("unrelated.bms", &bms)])]));
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let main = program("TESTSET", "MOVE 'B' TO EXISTING.");
    let analysis = CobolCompiler::default().analyze(&bundle(
        &main,
        &[(
            "MAPS",
            &[
                ("terminal.bms", &bms),
                ("TESTSET.cpy", "01 EXISTING PIC X."),
            ],
        )],
    ));
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(!analysis.syntax.unwrap().semantic_text().contains("SCREENI"));
    let main = program("TESTSET", "");
    let analysis = CobolCompiler::default().analyze(&bundle(
        &main,
        &[("MAPS", &[("one.bms", &bms), ("two.bms", &bms)])],
    ));
    assert!(
        analysis.diagnostics[0]
            .public_message()
            .contains("AmbiguousCopy")
    );
    let bad = bms.replace("LENGTH=1", "LENGTH=1,PICIN='9'");
    let analysis =
        CobolCompiler::default().analyze(&bundle(&main, &[("MAPS", &[("TESTSET.bms", &bad)])]));
    assert!(
        analysis.diagnostics[0]
            .public_message()
            .contains("unverified BMS symbolic-map feature: PICIN")
    );
    let main = program("TESTSET IN OTHER", "");
    let analysis =
        CobolCompiler::default().analyze(&bundle(&main, &[("MAPS", &[("one.bms", &bms)])]));
    assert!(
        analysis.diagnostics[0]
            .public_message()
            .contains("CopyNotFound")
    );
}

// Canonical descriptions retain every layout-bearing property and its order.
// Comparing tokens alone would miss COMP ordering differences or decimal dots.
#[derive(Debug, Eq, PartialEq)]
struct Description {
    level: u8,
    name: String,
    picture: Option<String>,
    usage: String,
    redefines: Option<String>,
}

fn descriptions(text: &str) -> Vec<Description> {
    text.lines()
        .filter(|line| !line.as_bytes().get(6).is_some_and(|b| *b == b'*'))
        .filter_map(|line| {
            let words = line
                .trim()
                .trim_end_matches('.')
                .split_whitespace()
                .collect::<Vec<_>>();
            let level = words.first()?.parse::<u8>().ok()?;
            let operand = |key: &str| {
                words
                    .iter()
                    .position(|word| *word == key)
                    .map(|index| words[index + 1].to_string())
            };
            Some(Description {
                level,
                name: words[1].into(),
                picture: operand("PIC").or_else(|| operand("PICTURE")),
                usage: if words.contains(&"COMP") {
                    "COMP"
                } else {
                    "DISPLAY"
                }
                .into(),
                redefines: operand("REDEFINES"),
            })
        })
        .collect()
}

#[test]
#[ignore = "requires CARDDEMO_CORPUS_DIR; run explicitly with --ignored; shipped-copybook comparison earns no conformance credit"]
fn carddemo_all_shipped_symbolic_copybooks_match_structurally() {
    let corpus = std::env::var_os("CARDDEMO_CORPUS_DIR").expect("CARDDEMO_CORPUS_DIR required");
    let root = Path::new(&corpus).join("app");
    let mut sources = std::fs::read_dir(root.join("bms"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bms"))
        .collect::<Vec<_>>();
    sources.sort();
    assert!(!sources.is_empty(), "no CardDemo BMS sources");
    let mut count = 0;
    for path in sources {
        let paired = root.join("cpy-bms").join(format!(
            "{}.CPY",
            path.file_stem().unwrap().to_str().unwrap()
        ));
        let mapset = BmsMapset::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let generated = mapset.symbolic_copybook().unwrap();
        let shipped = std::fs::read_to_string(&paired).unwrap();
        assert_eq!(
            descriptions(&generated),
            descriptions(&shipped),
            "{}",
            path.display()
        );
        count += 1;
    }
    eprintln!(
        "CardDemo: {count} BMS/CPY pairs structurally matched (names, levels, PIC/USAGE, order, REDEFINES)"
    );
}

#[test]
#[ignore = "requires GENAPP_CORPUS_DIR; run explicitly with --ignored; external ssmap compile earns no conformance credit"]
fn genapp_ssmap_library_source_compiles_and_publishes_copy_ssmap() {
    let corpus = std::env::var_os("GENAPP_CORPUS_DIR").expect("GENAPP_CORPUS_DIR required");
    let bms = std::fs::read_to_string(Path::new(&corpus).join("base/src/ssmap.bms")).unwrap();
    let mapset = BmsMapset::parse(&bms).unwrap();
    assert_eq!(mapset.name(), "SSMAP");
    let generated = mapset.symbolic_copybook().unwrap();
    assert!(generated.contains("01 SSMAPC1I."));
    assert!(generated.contains("01 SSMAPP5O REDEFINES SSMAPP5I."));
    let main = program(
        "SSMAP",
        "MOVE 10 TO ENT1CNOL. MOVE '0000000001' TO ENT1CNOO. DISPLAY ENT1CNOI.",
    );
    let source = bundle(&main, &[("MAPS", &[("maps/ssmap.bms", &bms)])]);
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap();
    assert!(
        matches!(result, CompilerResult::Published { .. }),
        "{result:?}"
    );
    eprintln!(
        "GenApp: generated SSMAP symbolic copybook and published executable using COPY SSMAP from BMS library input"
    );
}
