//! Frozen source/seed literals; never inferred from an observed screen.

pub(super) const INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/COCRDLIC.cbl",
        "d6a9210ad3062bd657881fc3f0d2a7368db3ff4869594e0d6a878d8f410d87b6",
    ),
    (
        "app/cpy/CVACT02Y.cpy",
        "9f1c62ef31b9d541712f1ed28f79e011ea7338611f5e0f30f2e1cbf624181223",
    ),
    (
        "app/cpy-bms/COCRDLI.CPY",
        "f5029dc12e672443e8fa33b764f60bf7db2e7ecfbc57d733289d70b382d995af",
    ),
    (
        "app/bms/COCRDLI.bms",
        "b73a1ebb2ec327db220a4df272a6766ddd809ebf0db378dcc9a3c83ec2506343",
    ),
    (
        "app/jcl/CARDFILE.jcl",
        "0240faee5aec80d976ce0b15a85f101fa0f901b09b72d64545daa09858ddcc0c",
    ),
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDDATA.PS",
        "b5d968b6865bd48fb45928ef6cacce9b6aacdaec7b51cfcc9819b9fafcaed3e5",
    ),
];

pub(super) const FIRST: &[(&str, &[u8])] = &[
    ("ACCTNO1", b"00000000050"),
    ("CRDNUM1", b"0500024453765740"),
    ("CRDSTS1", b"Y"),
    ("CRDSEL1", b" "),
    ("ACCTNO2", b"00000000027"),
    ("CRDNUM2", b"0683586198171516"),
    ("CRDSTS2", b"Y"),
    ("CRDSEL2", b" "),
    ("ACCTNO3", b"00000000002"),
    ("CRDNUM3", b"0923877193247330"),
    ("CRDSTS3", b"Y"),
    ("CRDSEL3", b" "),
    ("ACCTNO4", b"00000000020"),
    ("CRDNUM4", b"0927987108636232"),
    ("CRDSTS4", b"Y"),
    ("CRDSEL4", b" "),
    ("ACCTNO5", b"00000000012"),
    ("CRDNUM5", b"0982496213629795"),
    ("CRDSTS5", b"Y"),
    ("CRDSEL5", b" "),
    ("ACCTNO6", b"00000000044"),
    ("CRDNUM6", b"1014086565224350"),
    ("CRDSTS6", b"Y"),
    ("CRDSEL6", b" "),
    ("ACCTNO7", b"00000000037"),
    ("CRDNUM7", b"1142167692878931"),
    ("CRDSTS7", b"Y"),
    ("CRDSEL7", b" "),
    ("PAGENO", b"1  "),
    ("INFOMSG", b"TYPE S FOR DETAIL, U TO UPDATE ANY RECORD    "),
    (
        "ERRMSG",
        b"                                                                              ",
    ),
];

pub(super) const NEXT: &[(&str, &[u8])] = &[
    ("ACCTNO1", b"00000000035"),
    ("CRDNUM1", b"1561409106491600"),
    ("CRDSTS1", b"Y"),
    ("CRDSEL1", b" "),
    ("ACCTNO2", b"00000000039"),
    ("CRDNUM2", b"2745303720002090"),
    ("CRDSTS2", b"Y"),
    ("CRDSEL2", b" "),
    ("ACCTNO3", b"00000000024"),
    ("CRDNUM3", b"2760836797107565"),
    ("CRDSTS3", b"Y"),
    ("CRDSEL3", b" "),
    ("ACCTNO4", b"00000000006"),
    ("CRDNUM4", b"2871968252812490"),
    ("CRDSTS4", b"Y"),
    ("CRDSEL4", b" "),
    ("ACCTNO5", b"00000000022"),
    ("CRDNUM5", b"2940139362300449"),
    ("CRDSTS5", b"Y"),
    ("CRDSEL5", b" "),
    ("ACCTNO6", b"00000000004"),
    ("CRDNUM6", b"2988091353094312"),
    ("CRDSTS6", b"Y"),
    ("CRDSEL6", b" "),
    ("ACCTNO7", b"00000000010"),
    ("CRDNUM7", b"3260763612337560"),
    ("CRDSTS7", b"Y"),
    ("CRDSEL7", b" "),
    ("PAGENO", b"2  "),
    ("INFOMSG", b"TYPE S FOR DETAIL, U TO UPDATE ANY RECORD    "),
    (
        "ERRMSG",
        b"                                                                              ",
    ),
];

pub(super) const BACK_ERROR: &[u8] =
    b"NO PREVIOUS PAGES TO DISPLAY                                                  ";

// COCRDLIC 669-670 clears info; PF7/first-page 901-904 sets only error.
pub(super) const BACK_INFO: &[u8] = b"                                             ";
