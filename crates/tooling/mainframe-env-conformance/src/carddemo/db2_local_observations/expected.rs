//! Frozen first-party metadata and local carrier expectations, never product output.
//! SQL type family/precision/scale/CCSID/index/grants are outside this model.
use super::*;

pub(super) const TABLE_NAMES: &[&str] = &[
    "CARDDEMO.TRANSACTION_TYPE",
    "CARDDEMO.TRANSACTION_TYPE_CATEGORY",
    "CARDDEMO.AUTHFRDS",
];

pub(super) const SOURCE_INPUTS: &[(&str, &str)] = &[
    (
        "app/app-transaction-type-db2/jcl/CREADB21.jcl",
        "9ca64a0007f94cee65cdc2c4817ae433a72818831d654bf192613e6c6e7c4cc8",
    ),
    (
        "app/app-transaction-type-db2/jcl/TRANEXTR.jcl",
        "11649702d1a495e52c47493a9fc618c383e2932ea81444525b3e89b69edce598",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2CREAT.ctl",
        "e6f3b60dc712f29f72f40168c8566ee713ef816a8ea90462daba5c9f71da106b",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2TIAD1.ctl",
        "4fcd0636295fded6a443b7908fc838fb6b6ec6fa6e803c9d2601ba64481e1101",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2TEP41.ctl",
        "703636f7e6e5781f7c92f502e2923c839ae6d02a6be4cb8c1478433ca8f20f73",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2FREE.ctl",
        "042337a66816194c12594d9d940371f1ea413bab687455fd00a8216472bbdcaf",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTTYP.ctl",
        "65d7c9b86b82d6b85bf50bfa18740c40353104148d1c3ce33dafc1a1a114fd6f",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTCAT.ctl",
        "1e0d56c17d52a71b36e7e52c261cb992394ae6f66be35e0f223b030ae5354602",
    ),
    (
        "app/app-transaction-type-db2/ddl/TRNTYPE.ddl",
        "a30d00de6818c0fd5dbbaccebd05a81bad7832c4a1dc17805accb7c965511877",
    ),
    (
        "app/app-transaction-type-db2/ddl/TRNTYCAT.ddl",
        "705282e92ca4476f469382ede19f7e477ac9dc142be5353885562ff2455d930b",
    ),
    (
        "app/app-transaction-type-db2/ddl/XTRNTYPE.ddl",
        "794f587eb2f32ce24baaf3ad4481054b82a742c22912df003b673028259be7b7",
    ),
    (
        "app/app-transaction-type-db2/ddl/XTRNTYCAT.ddl",
        "da6bf619058d51614f79675f95b68cf95d32799e854bdeac23b3128e3ae81f65",
    ),
    (
        "app/app-authorization-ims-db2-mq/ddl/AUTHFRDS.ddl",
        "1634f898360955f32872bd9c35533513e2879135018681b92caa51c787abaeaf",
    ),
];

const TYPE_ROWS: &[&[&[u8]]] = &[
    &[b"01", b"PURCHASE"],
    &[b"02", b"PAYMENT"],
    &[b"03", b"CREDIT"],
    &[b"04", b"AUTHORIZATION"],
    &[b"05", b"REFUND"],
    &[b"06", b"REVERAL"],
    &[b"07", b"ADJUSTMENT"],
];

const CATEGORY_ROWS: &[&[&[u8]]] = &[
    &[b"01", b"0001", b"REGULAR SALES DRAFT"],
    &[b"01", b"0002", b"REGULAR CASH ADVANCE"],
    &[b"01", b"0003", b"CONVENIENCE CHECK DEBIT"],
    &[b"01", b"0004", b"ATM CASH ADVANCE"],
    &[b"01", b"0005", b"INTEREST AMOUNT"],
    &[b"02", b"0001", b"CASH PAYMENT"],
    &[b"02", b"0002", b"ELECTRONIC PAYMENT"],
    &[b"02", b"0003", b"CHECK PAYMENT"],
    &[b"03", b"0001", b"CREDIT TO ACCOUNT"],
    &[b"03", b"0002", b"CREDIT TO PURCHASE BALANCE"],
    &[b"03", b"0003", b"CREDIT TO CASH BALANCE"],
    &[b"04", b"0001", b"ZERO DOLLAR AUTHORIZATION"],
    &[b"04", b"0002", b"ONLINE PURCHASE AUTHORIZATION"],
    &[b"04", b"0003", b"TRAVEL BOOKING AUTHORIZATION"],
    &[b"05", b"0001", b"REFUND CREDIT"],
    &[b"06", b"0001", b"FRAUD REVERSAL"],
    &[b"06", b"0002", b"NON FRAUD REVERSAL"],
    &[b"07", b"0001", b"SALES DRAFT CREDIT ADJUSTMENT"],
];

fn column(
    name: &str,
    nullable: bool,
    max_bytes: usize,
    encoding: Db2ResultEncoding,
) -> Db2ColumnDefinition {
    Db2ColumnDefinition {
        name: name.into(),
        nullable,
        max_bytes,
        result_encoding: encoding,
        default_value: None,
    }
}

fn definitions() -> Vec<Db2TableDefinition> {
    vec![
        Db2TableDefinition {
            name: "CARDDEMO.TRANSACTION_TYPE".into(),
            columns: vec![
                column("TR_TYPE", false, 2, Db2ResultEncoding::Raw),
                column("TR_DESCRIPTION", false, 50, Db2ResultEncoding::Varchar),
            ],
            primary_key: vec!["TR_TYPE".into()],
            foreign_keys: vec![],
            extract: Some(Db2ExtractLayout {
                fields: vec![
                    Db2ExtractField {
                        column: "TR_TYPE".into(),
                        width: 2,
                    },
                    Db2ExtractField {
                        column: "TR_DESCRIPTION".into(),
                        width: 50,
                    },
                ],
                trailer: b"00000000".to_vec(),
            }),
        },
        Db2TableDefinition {
            name: "CARDDEMO.TRANSACTION_TYPE_CATEGORY".into(),
            columns: vec![
                column("TRC_TYPE_CODE", false, 2, Db2ResultEncoding::Raw),
                column("TRC_TYPE_CATEGORY", false, 4, Db2ResultEncoding::Raw),
                column("TRC_CAT_DATA", false, 50, Db2ResultEncoding::Varchar),
            ],
            primary_key: vec!["TRC_TYPE_CODE".into(), "TRC_TYPE_CATEGORY".into()],
            foreign_keys: vec![Db2ForeignKeyDefinition {
                columns: vec!["TRC_TYPE_CODE".into()],
                referenced_table: "CARDDEMO.TRANSACTION_TYPE".into(),
                referenced_columns: vec!["TR_TYPE".into()],
                delete_restrict: true,
            }],
            extract: Some(Db2ExtractLayout {
                fields: vec![
                    Db2ExtractField {
                        column: "TRC_TYPE_CODE".into(),
                        width: 2,
                    },
                    Db2ExtractField {
                        column: "TRC_TYPE_CATEGORY".into(),
                        width: 4,
                    },
                    Db2ExtractField {
                        column: "TRC_CAT_DATA".into(),
                        width: 50,
                    },
                ],
                trailer: b"0000".to_vec(),
            }),
        },
        Db2TableDefinition {
            name: "CARDDEMO.AUTHFRDS".into(),
            columns: vec![
                // These are the declared local carrier capacities, not IBM SQL
                // type widths. TIMESTAMP/SMALLINT/DATE retain the existing 256 cap.
                column("CARD_NUM", false, 16, Db2ResultEncoding::Raw),
                column("AUTH_TS", false, 256, Db2ResultEncoding::Raw),
                column("AUTH_TYPE", true, 4, Db2ResultEncoding::Raw),
                column("CARD_EXPIRY_DATE", true, 4, Db2ResultEncoding::Raw),
                column("MESSAGE_TYPE", true, 6, Db2ResultEncoding::Raw),
                column("MESSAGE_SOURCE", true, 6, Db2ResultEncoding::Raw),
                column("AUTH_ID_CODE", true, 6, Db2ResultEncoding::Raw),
                column("AUTH_RESP_CODE", true, 2, Db2ResultEncoding::Raw),
                column("AUTH_RESP_REASON", true, 4, Db2ResultEncoding::Raw),
                column("PROCESSING_CODE", true, 6, Db2ResultEncoding::Raw),
                column("TRANSACTION_AMT", true, 12, Db2ResultEncoding::Raw),
                column("APPROVED_AMT", true, 12, Db2ResultEncoding::Raw),
                column("MERCHANT_CATAGORY_CODE", true, 4, Db2ResultEncoding::Raw),
                column("ACQR_COUNTRY_CODE", true, 3, Db2ResultEncoding::Raw),
                column("POS_ENTRY_MODE", true, 256, Db2ResultEncoding::Raw),
                column("MERCHANT_ID", true, 15, Db2ResultEncoding::Raw),
                column("MERCHANT_NAME", true, 22, Db2ResultEncoding::Varchar),
                column("MERCHANT_CITY", true, 13, Db2ResultEncoding::Raw),
                column("MERCHANT_STATE", true, 2, Db2ResultEncoding::Raw),
                column("MERCHANT_ZIP", true, 9, Db2ResultEncoding::Raw),
                column("TRANSACTION_ID", true, 15, Db2ResultEncoding::Raw),
                column("MATCH_STATUS", true, 1, Db2ResultEncoding::Raw),
                column("AUTH_FRAUD", true, 1, Db2ResultEncoding::Raw),
                column("FRAUD_RPT_DATE", true, 256, Db2ResultEncoding::Raw),
                column("ACCT_ID", true, 11, Db2ResultEncoding::Raw),
                column("CUST_ID", true, 9, Db2ResultEncoding::Raw),
            ],
            primary_key: vec!["CARD_NUM".into(), "AUTH_TS".into()],
            foreign_keys: Vec::new(),
            extract: None,
        },
    ]
}

pub(super) fn tables(phase: RowsPhase) -> LocalTables {
    let mut rows = vec![Vec::new(), Vec::new(), Vec::new()];
    if !matches!(phase, RowsPhase::Empty) {
        rows[0] = TYPE_ROWS
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(index, value)| {
                        if matches!(phase, RowsPhase::Maintained) && row[0] == b"02" && index == 1 {
                            b"BATCH PAYMENT".to_vec()
                        } else {
                            value.to_vec()
                        }
                    })
                    .collect()
            })
            .collect();
        rows[1] = CATEGORY_ROWS
            .iter()
            .map(|row| row.iter().map(|value| value.to_vec()).collect())
            .collect();
    }
    LocalTables(definitions().into_iter().zip(rows).collect())
}

pub(super) const CREATE_SPOOL: &[(&str, &[&[u8]])] = &[
    (
        "FREEPLN:SYSPRINT",
        &[
            b"IKJEFT01 SQLCODE=0",
            b"IKJEFT01 SQLCODE=0",
            b"IKJEFT01 SQLCODE=0",
        ],
    ),
    ("CRCRDDB:SYSPRINT", &[b"IKJEFT01 DSNTIAD SQLCODE=0 ROWS=0"]),
    ("LDTTYPE:SYSPRINT", &[b"IEFBR14"]),
    ("RUNTEP2:SYSPRINT", &[b"IKJEFT01 DSNTEP4 SQLCODE=0 ROWS=0"]),
    ("LDTCCAT:SYSPRINT", &[b"IKJEFT01 DSNTEP4 SQLCODE=0 ROWS=0"]),
    ("RUNTEP2:JOBLOG", &[b"RUNTEP2 IKJEFT01 RC=0000"]),
    ("LDTCCAT:JOBLOG", &[b"LDTCCAT IKJEFT01 RC=0000"]),
];

pub(super) const EXTRACT_SPOOL: &[(&str, &[&[u8]])] = &[
    ("STEP40:SYSPRINT", &[b"IKJEFT01 DSNTIAUL SQLCODE=0 ROWS=7"]),
    ("STEP50:SYSPRINT", &[b"IKJEFT01 DSNTIAUL SQLCODE=0 ROWS=18"]),
    ("STEP40:JOBLOG", &[b"STEP40 IKJEFT01 RC=0000"]),
    ("STEP50:JOBLOG", &[b"STEP50 IKJEFT01 RC=0000"]),
];
