/// One source-reviewed output admitted by the typed CICS `ASSIGN` subset.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CicsAssignOutput(u8);

/// Canonical names of every output admitted by the typed CICS `ASSIGN` subset.
pub const CICS_ASSIGN_OUTPUT_NAMES: &[&str] = &[
    "ABCODE",
    "ABDUMP",
    "ABOFFSET",
    "ABPROGRAM",
    "ACTIVITY",
    "ACTIVITYID",
    "ALTSCRNHT",
    "ALTSCRNWD",
    "APLKYBD",
    "APLTEXT",
    "APPLICATION",
    "APPLID",
    "ASRAINTRPT",
    "ASRAPSW",
    "ASRAPSW16",
    "ASRAREGS",
    "ASRAREGS64",
    "BRIDGE",
    "BTRANS",
    "CHANNEL",
    "CMDSEC",
    "COLOR",
    "CWALENG",
    "DEFSCRNHT",
    "DEFSCRNWD",
    "DESTID",
    "DESTIDLENG",
    "DS3270",
    "DSSCS",
    "ERRORMSG",
    "ERRORMSGLEN",
    "EWASUPP",
    "EXTDS",
    "FCI",
    "GMMI",
    "HILIGHT",
    "INITPARM",
    "INITPARMLEN",
    "KATAKANA",
    "LINKLEVEL",
    "LOCALCCSID",
    "MAJORVERSION",
    "MAPCOLUMN",
    "MAPHEIGHT",
    "MAPLINE",
    "MAPWIDTH",
    "MICROVERSION",
    "MINORVERSION",
    "MSRCONTROL",
    "NEXTTRANSID",
    "OPERATION",
    "OPERKEYS",
    "OPSECURITY",
    "ORGABCODE",
    "OUTLINE",
    "PARTNS",
    "PARTNSET",
    "PLATFORM",
    "PRINSYSID",
    "PROCESS",
    "PROCESSTYPE",
    "PROGRAM",
    "PS",
    "QNAME",
    "RESSEC",
    "RESTART",
    "SCRNHT",
    "SCRNWD",
    "SOSI",
    "SYSID",
    "TASKPRIORITY",
    "TCTUALENG",
    "TEXTKYBD",
    "TEXTPRINT",
    "TWALENG",
    "UNATTEND",
    "USERID",
    "VALIDATION",
    "DESTCOUNT",
    "LDCMNEM",
    "LDCNUM",
    "PAGENUM",
    "PARTNPAGE",
    "RETURNPROG",
];

impl CicsAssignOutput {
    /// Resolves one canonical output name without accepting aliases.
    pub fn from_name(name: &str) -> Option<Self> {
        CICS_ASSIGN_OUTPUT_NAMES
            .iter()
            .position(|candidate| *candidate == name)
            .and_then(|index| u8::try_from(index).ok())
            .map(Self)
    }

    /// Returns this output's canonical host-request name.
    pub const fn name(self) -> &'static str {
        CICS_ASSIGN_OUTPUT_NAMES[self.0 as usize]
    }

    pub(super) const fn tag(self) -> u8 {
        self.0
    }

    pub(super) fn from_tag(tag: u8) -> Option<Self> {
        (usize::from(tag) < CICS_ASSIGN_OUTPUT_NAMES.len()).then_some(Self(tag))
    }
}
