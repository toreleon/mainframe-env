//! Owned source-level compatibility contracts for reached external copybooks.

use mainframe_env_source::{
    LibraryProblem, LogicalPath, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits, SourceProblem,
};
use std::fmt;

pub const COMPATIBILITY_COPYBOOK_CONTRACT: &str = "mainframe-env.cobol-compatibility-copybooks@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompatibilityCopybook {
    pub name: &'static str,
    pub behavior: &'static str,
    source: &'static str,
}

impl CompatibilityCopybook {
    #[must_use]
    pub const fn source(self) -> &'static str {
        self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompatibilityProblem {
    Source(SourceProblem),
    Library(LibraryProblem),
}

impl fmt::Display for CompatibilityProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "compatibility copybook failed: {self:?}")
    }
}

impl std::error::Error for CompatibilityProblem {}

#[must_use]
pub const fn compatibility_copybooks() -> &'static [CompatibilityCopybook] {
    &COPYBOOKS
}

pub fn owned_compatibility_library(
    limits: SourceLimits,
) -> Result<(Vec<SourceFile>, SourceLibrary), CompatibilityProblem> {
    let mut files = Vec::with_capacity(COPYBOOKS.len());
    let mut members = Vec::with_capacity(COPYBOOKS.len());
    for copybook in COPYBOOKS {
        let path = format!("compatibility/{}.cpy", copybook.name);
        let logical =
            LogicalPath::new(&path, limits.max_path_bytes).map_err(CompatibilityProblem::Source)?;
        let file = SourceFile::input(
            path,
            copybook.source.as_bytes().to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .map_err(CompatibilityProblem::Source)?;
        members.push(logical);
        files.push(file);
    }
    let library = SourceLibrary::new("owned-compatibility", members, limits)
        .map_err(CompatibilityProblem::Library)?;
    Ok((files, library))
}

const COPYBOOKS: [CompatibilityCopybook; 9] = [
    CompatibilityCopybook {
        name: "DFHAID",
        behavior: "Reached 3270 attention identifiers represented as exact one-byte values.",
        source: DFHAID,
    },
    CompatibilityCopybook {
        name: "DFHBMSCA",
        behavior: "Reached BMS field attributes and extended colors represented as exact bytes.",
        source: DFHBMSCA,
    },
    CompatibilityCopybook {
        name: "SQLCA",
        behavior: "136-byte SQL communication area with reached SQLCODE, SQLERRM, and SQLSTATE fields.",
        source: SQLCA,
    },
    CompatibilityCopybook {
        name: "CMQGMOV",
        behavior: "Reached MQGMO options and wait interval in a bounded version-4 layout.",
        source: CMQGMOV,
    },
    CompatibilityCopybook {
        name: "CMQMDV",
        behavior: "364-byte MQMD version-2 layout with reached message and correlation fields.",
        source: CMQMDV,
    },
    CompatibilityCopybook {
        name: "CMQODV",
        behavior: "400-byte MQOD version-4 layout with reached object names and type.",
        source: CMQODV,
    },
    CompatibilityCopybook {
        name: "CMQPMOV",
        behavior: "Reached MQPMO options in a bounded version-3 layout.",
        source: CMQPMOV,
    },
    CompatibilityCopybook {
        name: "CMQTML",
        behavior: "Reached 684-byte MQ trigger-message layout.",
        source: CMQTML,
    },
    CompatibilityCopybook {
        name: "CMQV",
        behavior: "Reached MQ completion, reason, option, format, and identifier constants.",
        source: CMQV,
    },
];

const DFHAID: &str = "       01 DFHAID-VALUES.\n          02 DFHENTER PIC X VALUE X'7D'.\n          02 DFHCLEAR PIC X VALUE X'6D'.\n          02 DFHPA1 PIC X VALUE X'6C'.\n          02 DFHPA2 PIC X VALUE X'6E'.\n          02 DFHPF1 PIC X VALUE X'F1'.\n          02 DFHPF2 PIC X VALUE X'F2'.\n          02 DFHPF3 PIC X VALUE X'F3'.\n          02 DFHPF4 PIC X VALUE X'F4'.\n          02 DFHPF5 PIC X VALUE X'F5'.\n          02 DFHPF6 PIC X VALUE X'F6'.\n          02 DFHPF7 PIC X VALUE X'F7'.\n          02 DFHPF8 PIC X VALUE X'F8'.\n          02 DFHPF9 PIC X VALUE X'F9'.\n          02 DFHPF10 PIC X VALUE X'7A'.\n          02 DFHPF11 PIC X VALUE X'7B'.\n          02 DFHPF12 PIC X VALUE X'7C'.\n          02 DFHPF13 PIC X VALUE X'C1'.\n          02 DFHPF14 PIC X VALUE X'C2'.\n          02 DFHPF15 PIC X VALUE X'C3'.\n          02 DFHPF16 PIC X VALUE X'C4'.\n          02 DFHPF17 PIC X VALUE X'C5'.\n          02 DFHPF18 PIC X VALUE X'C6'.\n          02 DFHPF19 PIC X VALUE X'C7'.\n          02 DFHPF20 PIC X VALUE X'C8'.\n          02 DFHPF21 PIC X VALUE X'C9'.\n          02 DFHPF22 PIC X VALUE X'4A'.\n          02 DFHPF23 PIC X VALUE X'4B'.\n          02 DFHPF24 PIC X VALUE X'4C'.\n";

const DFHBMSCA: &str = "       01 DFHBMSCA-VALUES.\n          02 DFHBMUNP PIC X VALUE X'C0'.\n          02 DFHBMPRO PIC X VALUE X'F0'.\n          02 DFHBMBRY PIC X VALUE X'C8'.\n          02 DFHBMDAR PIC X VALUE X'CC'.\n          02 DFHBMFSE PIC X VALUE X'C1'.\n          02 DFHBMPRF PIC X VALUE X'F1'.\n          02 DFHBMASF PIC X VALUE X'F1'.\n          02 DFHBMASB PIC X VALUE X'F8'.\n          02 DFHBLUE PIC X VALUE X'F1'.\n          02 DFHRED PIC X VALUE X'F2'.\n          02 DFHGREEN PIC X VALUE X'F4'.\n          02 DFHNEUTR PIC X VALUE X'F7'.\n          02 DFHDFCOL PIC X VALUE X'00'.\n";

const SQLCA: &str = "       01 SQLCA.\n          05 SQLCAID PIC X(8) VALUE 'SQLCA   '.\n          05 SQLCABC PIC S9(9) COMP-5 VALUE +136.\n          05 SQLCODE PIC S9(9) COMP-5.\n          05 SQLERRM.\n             49 SQLERRML PIC S9(4) COMP-5.\n             49 SQLERRMC PIC X(70).\n          05 SQLERRP PIC X(8).\n          05 SQLERRD OCCURS 6 TIMES PIC S9(9) COMP-5.\n          05 SQLWARN.\n             10 SQLWARN0 PIC X.\n             10 SQLWARN1 PIC X.\n             10 SQLWARN2 PIC X.\n             10 SQLWARN3 PIC X.\n             10 SQLWARN4 PIC X.\n             10 SQLWARN5 PIC X.\n             10 SQLWARN6 PIC X.\n             10 SQLWARN7 PIC X.\n             10 SQLWARN8 PIC X.\n             10 SQLWARN9 PIC X.\n             10 SQLWARNA PIC X.\n          05 SQLSTATE PIC X(5).\n";

const CMQGMOV: &str = "          05 MQGMO-STRUCID PIC X(4) VALUE 'GMO '.\n          05 MQGMO-VERSION PIC S9(9) BINARY VALUE 4.\n          05 MQGMO-OPTIONS PIC S9(9) BINARY VALUE 0.\n          05 MQGMO-WAITINTERVAL PIC S9(9) BINARY VALUE 0.\n          05 FILLER PIC X(96) VALUE LOW-VALUES.\n";

const CMQMDV: &str = "          05 MQMD-STRUCID PIC X(4) VALUE 'MD  '.\n          05 MQMD-VERSION PIC S9(9) BINARY VALUE 2.\n          05 MQMD-REPORT PIC S9(9) BINARY VALUE 0.\n          05 MQMD-MSGTYPE PIC S9(9) BINARY VALUE 8.\n          05 MQMD-EXPIRY PIC S9(9) BINARY VALUE -1.\n          05 MQMD-FEEDBACK PIC S9(9) BINARY VALUE 0.\n          05 MQMD-ENCODING PIC S9(9) BINARY VALUE 0.\n          05 MQMD-CODEDCHARSETID PIC S9(9) BINARY VALUE 0.\n          05 MQMD-FORMAT PIC X(8) VALUE SPACES.\n          05 MQMD-PRIORITY PIC S9(9) BINARY VALUE -1.\n          05 MQMD-PERSISTENCE PIC S9(9) BINARY VALUE 2.\n          05 MQMD-MSGID PIC X(24) VALUE LOW-VALUES.\n          05 MQMD-CORRELID PIC X(24) VALUE LOW-VALUES.\n          05 MQMD-BACKOUTCOUNT PIC S9(9) BINARY VALUE 0.\n          05 MQMD-REPLYTOQ PIC X(48) VALUE SPACES.\n          05 MQMD-REPLYTOQMGR PIC X(48) VALUE SPACES.\n          05 MQMD-USERIDENTIFIER PIC X(12) VALUE SPACES.\n          05 MQMD-ACCOUNTINGTOKEN PIC X(32) VALUE LOW-VALUES.\n          05 MQMD-APPLIDENTITYDATA PIC X(32) VALUE SPACES.\n          05 MQMD-PUTAPPLTYPE PIC S9(9) BINARY VALUE 0.\n          05 MQMD-PUTAPPLNAME PIC X(28) VALUE SPACES.\n          05 MQMD-PUTDATE PIC X(8) VALUE SPACES.\n          05 MQMD-PUTTIME PIC X(8) VALUE SPACES.\n          05 MQMD-APPLORIGINDATA PIC X(4) VALUE SPACES.\n          05 MQMD-GROUPID PIC X(24) VALUE LOW-VALUES.\n          05 MQMD-MSGSEQNUMBER PIC S9(9) BINARY VALUE 1.\n          05 MQMD-OFFSET PIC S9(9) BINARY VALUE 0.\n          05 MQMD-MSGFLAGS PIC S9(9) BINARY VALUE 0.\n          05 MQMD-ORIGINALLENGTH PIC S9(9) BINARY VALUE -1.\n";

const CMQODV: &str = "          05 MQOD-STRUCID PIC X(4) VALUE 'OD  '.\n          05 MQOD-VERSION PIC S9(9) BINARY VALUE 4.\n          05 MQOD-OBJECTTYPE PIC S9(9) BINARY VALUE 1.\n          05 MQOD-OBJECTNAME PIC X(48) VALUE SPACES.\n          05 MQOD-OBJECTQMGRNAME PIC X(48) VALUE SPACES.\n          05 MQOD-DYNAMICQNAME PIC X(48) VALUE 'AMQ.*'.\n          05 MQOD-ALTERNATEUSERID PIC X(12) VALUE SPACES.\n          05 FILLER PIC X(232) VALUE LOW-VALUES.\n";

const CMQPMOV: &str = "          05 MQPMO-STRUCID PIC X(4) VALUE 'PMO '.\n          05 MQPMO-VERSION PIC S9(9) BINARY VALUE 3.\n          05 MQPMO-OPTIONS PIC S9(9) BINARY VALUE 0.\n          05 FILLER PIC X(164) VALUE LOW-VALUES.\n";

const CMQTML: &str = "          05 MQTM.\n             10 MQTM-STRUCID PIC X(4) VALUE 'TM  '.\n             10 MQTM-VERSION PIC S9(9) BINARY VALUE 1.\n             10 MQTM-QNAME PIC X(48) VALUE SPACES.\n             10 MQTM-PROCESSNAME PIC X(48) VALUE SPACES.\n             10 MQTM-TRIGGERDATA PIC X(64) VALUE SPACES.\n             10 MQTM-APPLTYPE PIC S9(9) BINARY VALUE 0.\n             10 MQTM-APPLID PIC X(256) VALUE SPACES.\n             10 MQTM-ENVDATA PIC X(128) VALUE SPACES.\n             10 MQTM-USERDATA PIC X(128) VALUE SPACES.\n";

const CMQV: &str = "          05 MQCC-OK PIC S9(9) BINARY VALUE 0.\n          05 MQRC-NO-MSG-AVAILABLE PIC S9(9) BINARY VALUE 2033.\n          05 MQCO-NONE PIC S9(9) BINARY VALUE 0.\n          05 MQOT-Q PIC S9(9) BINARY VALUE 1.\n          05 MQOO-INPUT-SHARED PIC S9(9) BINARY VALUE 2.\n          05 MQOO-OUTPUT PIC S9(9) BINARY VALUE 16.\n          05 MQOO-SAVE-ALL-CONTEXT PIC S9(9) BINARY VALUE 128.\n          05 MQOO-PASS-ALL-CONTEXT PIC S9(9) BINARY VALUE 512.\n          05 MQOO-FAIL-IF-QUIESCING PIC S9(9) BINARY VALUE 8192.\n          05 MQGMO-WAIT PIC S9(9) BINARY VALUE 1.\n          05 MQGMO-SYNCPOINT PIC S9(9) BINARY VALUE 2.\n          05 MQGMO-NO-SYNCPOINT PIC S9(9) BINARY VALUE 4.\n          05 MQGMO-FAIL-IF-QUIESCING PIC S9(9) BINARY VALUE 8192.\n          05 MQGMO-CONVERT PIC S9(9) BINARY VALUE 16384.\n          05 MQPMO-SYNCPOINT PIC S9(9) BINARY VALUE 2.\n          05 MQPMO-NO-SYNCPOINT PIC S9(9) BINARY VALUE 4.\n          05 MQPMO-DEFAULT-CONTEXT PIC S9(9) BINARY VALUE 32.\n          05 MQPMO-FAIL-IF-QUIESCING PIC S9(9) BINARY VALUE 8192.\n          05 MQMT-REPLY PIC S9(9) BINARY VALUE 2.\n          05 MQPER-NOT-PERSISTENT PIC S9(9) BINARY VALUE 0.\n          05 MQCCSI-Q-MGR PIC S9(9) BINARY VALUE 0.\n          05 MQFMT-STRING PIC X(8) VALUE 'MQSTR   '.\n          05 MQMI-NONE PIC X(24) VALUE LOW-VALUES.\n          05 MQCI-NONE PIC X(24) VALUE LOW-VALUES.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_catalog_is_complete_unique_and_behavioral() {
        let names = compatibility_copybooks()
            .iter()
            .map(|copybook| copybook.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), 9);
        assert!(compatibility_copybooks().iter().all(|copybook| {
            !copybook.behavior.is_empty()
                && !copybook.source().contains("placeholder")
                && copybook.source().contains("PIC")
        }));
        let (files, library) = owned_compatibility_library(SourceLimits::default()).unwrap();
        assert_eq!(files.len(), 9);
        assert_eq!(library.members().len(), 9);
    }

    #[test]
    fn reached_constants_and_layout_lengths_are_owned() {
        assert!(DFHAID.contains("DFHENTER PIC X VALUE X'7D'"));
        assert!(DFHBMSCA.contains("DFHRED PIC X VALUE X'F2'"));
        assert!(SQLCA.contains("SQLCABC PIC S9(9) COMP-5 VALUE +136"));
        assert!(CMQMDV.contains("MQMD-CORRELID PIC X(24)"));
        assert!(CMQTML.contains("MQTM-QNAME PIC X(48)"));
        assert!(CMQV.contains("MQRC-NO-MSG-AVAILABLE PIC S9(9) BINARY VALUE 2033"));
    }
}
