#![no_main]

use libfuzzer_sys::fuzz_target;
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::BTreeMap;

// Input is the command body. The fixed program keeps mutations inside the EXEC CICS grammar.
fuzz_target!(|data: &[u8]| {
    let body = &data[..data.len().min(3072)];
    let prefix = b"IDENTIFICATION DIVISION. PROGRAM-ID. CICSFZ. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ";
    let suffix = b" END-EXEC. STOP RUN.";
    let mut source = Vec::with_capacity(prefix.len() + body.len() + suffix.len());
    source.extend_from_slice(prefix);
    source.extend_from_slice(body);
    source.extend_from_slice(suffix);
    let limits = SourceLimits {
        max_files: 1,
        max_file_bytes: 4096,
        max_total_bytes: 4096,
        max_path_bytes: 32,
        max_options: 4,
        max_option_bytes: 64,
        max_provenance_edges: 16,
    };
    let Ok(path) = LogicalPath::new("CICSFZ.cbl", limits.max_path_bytes) else {
        return;
    };
    let Ok(file) = SourceFile::input(
        "CICSFZ.cbl",
        source,
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    ) else {
        return;
    };
    let Ok(bundle) = SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits)
    else {
        return;
    };
    let _ = CobolCompiler::default().analyze(&bundle);
});
