use super::*;
use mainframe_env_ir::CicsAssignOutput;

pub(super) fn validate_output(
    layout: &LayoutMetadata,
    output: CicsAssignOutput,
) -> Result<(), MachineProblem> {
    let name = output.name();
    if matches!(
        name,
        "ABOFFSET" | "LOCALCCSID" | "MAJORVERSION" | "MICROVERSION" | "MINORVERSION"
    ) && (layout.category != LayoutCategory::Binary || layout.length != 4 || layout.scale != 0)
    {
        return Err(invalid_plan("ASSIGN fullword output has the wrong layout"));
    }
    if matches!(
        name,
        "ALTSCRNHT"
            | "ALTSCRNWD"
            | "CWALENG"
            | "DEFSCRNHT"
            | "DEFSCRNWD"
            | "DESTIDLENG"
            | "DESTCOUNT"
            | "ERRORMSGLEN"
            | "INITPARMLEN"
            | "LINKLEVEL"
            | "MAPCOLUMN"
            | "MAPHEIGHT"
            | "MAPLINE"
            | "MAPWIDTH"
            | "PAGENUM"
            | "SCRNHT"
            | "SCRNWD"
            | "TASKPRIORITY"
            | "TCTUALENG"
            | "TERMPRIORITY"
            | "TWALENG"
    ) && (layout.category != LayoutCategory::Binary || layout.length != 2 || layout.scale != 0)
    {
        return Err(invalid_plan("ASSIGN halfword output has the wrong layout"));
    }
    let exact_length = match name {
        "ABCODE" | "BRIDGE" | "NEXTTRANSID" | "ORGABCODE" | "PRINSYSID" | "QNAME" => Some(4),
        "ABDUMP" | "APLKYBD" | "APLTEXT" | "BTRANS" | "COLOR" | "DS3270" | "DSSCS" | "EWASUPP"
        | "EXTDS" | "FCI" | "GMMI" | "HILIGHT" | "KATAKANA" | "MSRCONTROL" | "OUTLINE"
        | "LDCNUM" | "PARTNS" | "PS" | "RESTART" | "SOSI" | "TEXTKYBD" | "TEXTPRINT"
        | "UNATTEND" | "VALIDATION" | "CMDSEC" | "RESSEC" => Some(1),
        "ABPROGRAM" | "ASRAINTRPT" | "ASRAPSW" | "DESTID" | "OPERKEYS" | "PROGRAM"
        | "PROCESSTYPE" | "RETURNPROG" => Some(8),
        "ACTIVITY" | "ASRAPSW16" => Some(16),
        "ACTIVITYID" => Some(52),
        "ASRAREGS" => Some(64),
        "ASRAREGS64" => Some(128),
        "ERRORMSG" => Some(500),
        "INITPARM" => Some(60),
        "OPSECURITY" => Some(3),
        "LDCMNEM" | "PARTNPAGE" => Some(2),
        "PARTNSET" => Some(6),
        "PROCESS" => Some(36),
        _ => None,
    };
    if exact_length.is_some_and(|expected| layout.length != expected) {
        return Err(invalid_plan("ASSIGN output has the wrong exact width"));
    }
    Ok(())
}
