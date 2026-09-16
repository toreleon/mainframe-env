use crate::DdPlan;

pub(crate) fn is_program_library_dd(dd: &DdPlan) -> bool {
    dd.name.eq_ignore_ascii_case("STEPLIB")
        || dd.name.eq_ignore_ascii_case("JOBLIB")
        || dd.name.eq_ignore_ascii_case("DBRMLIB")
        || dd.name.eq_ignore_ascii_case("DFSRESLB")
        || dd.name.eq_ignore_ascii_case("IMS")
        || dd.name.eq_ignore_ascii_case("DFSVSAMP")
        || dd.name.eq_ignore_ascii_case("PROCLIB")
        || dd.name.eq_ignore_ascii_case("DFSSEL")
        || dd.name.to_ascii_uppercase().starts_with("DDPAUT")
}

/// True for DD names whose flattened `DdPlan::inline_data` bytes are still
/// read after `hydrate_dds` runs, so the flattened copy must be kept
/// alongside `dd_records` instead of relying on `dd_records` alone. Every
/// other dataset-backed DD carries its hydrated records exactly once, in
/// `dd_records` (#182). Known post-hydration readers of `inline_data`:
/// - `mainframe-env-server/src/cobol.rs` (~501): `SYSIN` becomes COBOL
///   terminal input (`cobol.terminal.input`).
/// - `mainframe-env-server/src/cobol.rs` `source_bundle`: `SYSIN` is the
///   compile-on-run primary source, and every `SYSLIB*` DD's `inline_data`
///   is a copybook file.
/// - `mainframe-env-batch/src/program.rs` (~361-366): the IDCAMS builtin
///   reads control statements from `SYSIN`.
pub(crate) fn retains_flattened_dataset_bytes(name: &str) -> bool {
    name.eq_ignore_ascii_case("SYSIN") || name.to_ascii_uppercase().starts_with("SYSLIB")
}
