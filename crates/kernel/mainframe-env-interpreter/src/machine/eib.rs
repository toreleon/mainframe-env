use super::{CobolValue, Decimal, MachineProblem};
use std::collections::BTreeMap;

pub(super) fn implicit_values(
    commarea_len: Option<usize>,
    aid: Option<u8>,
    transaction: Option<&[u8]>,
) -> Result<BTreeMap<String, CobolValue>, MachineProblem> {
    Ok(BTreeMap::from([
        (
            "EIBRESP".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        (
            "EIBRESP2".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        (
            "EIBDATE".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        (
            "EIBTIME".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        ("EIBFN".into(), CobolValue::Bytes(vec![0, 0])),
        (
            "EIBCALEN".into(),
            CobolValue::Decimal(Decimal {
                coefficient: i128::try_from(commarea_len.unwrap_or(0))
                    .map_err(|_| MachineProblem::InvalidOperation)?,
                scale: 0,
            }),
        ),
        ("EIBAID".into(), CobolValue::Bytes(vec![aid.unwrap_or(0)])),
        (
            "EIBTRNID".into(),
            CobolValue::Bytes(transaction.unwrap_or_default().to_vec()),
        ),
    ]))
}
