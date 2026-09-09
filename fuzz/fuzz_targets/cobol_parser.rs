#![no_main]

use libfuzzer_sys::fuzz_target;
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::BTreeMap;

fuzz_target!(|data: &[u8]| {
    let (selector, source) = data.split_first().unwrap_or((&0, &[]));
    let format = match selector % 3 {
        0 => SourceFormat::Free,
        1 => SourceFormat::Fixed,
        _ => SourceFormat::Variable,
    };
    let encoding = if selector & 4 == 0 {
        SourceEncoding::Utf8
    } else {
        SourceEncoding::Ebcdic(37)
    };
    let limits = SourceLimits {
        max_files: 1,
        max_file_bytes: 4096,
        max_total_bytes: 4096,
        max_path_bytes: 32,
        max_options: 4,
        max_option_bytes: 64,
        max_provenance_edges: 16,
    };
    let Ok(path) = LogicalPath::new("FUZZ.cbl", limits.max_path_bytes) else {
        return;
    };
    let Ok(file) = SourceFile::input("FUZZ.cbl", source.to_vec(), format, encoding, limits) else {
        return;
    };
    let Ok(bundle) = SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits) else {
        return;
    };
    let _ = CobolCompiler::default().analyze(&bundle);
});
