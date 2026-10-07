use super::MachineProblem;
use mainframe_env_encoding::CodePage;

pub(super) fn encode_dataset_record(
    ccsid: Option<u16>,
    record: &[u8],
) -> Result<Vec<u8>, MachineProblem> {
    match ccsid {
        None | Some(1208) => Ok(record.to_vec()),
        Some(37) => CodePage::Cp037
            .encode(
                std::str::from_utf8(record).map_err(|_| MachineProblem::DataException)?,
                record.len().saturating_mul(4).max(1),
            )
            .map_err(|_| MachineProblem::DataException),
        Some(_) => Err(MachineProblem::UnsupportedForm),
    }
}

pub(super) fn decode_dataset_record(
    ccsid: Option<u16>,
    record: &[u8],
) -> Result<Vec<u8>, MachineProblem> {
    match ccsid {
        None | Some(1208) => Ok(record.to_vec()),
        Some(37) => CodePage::Cp037
            .decode(record, record.len().saturating_mul(4).max(1))
            .map(String::into_bytes)
            .map_err(|_| MachineProblem::DataException),
        Some(_) => Err(MachineProblem::UnsupportedForm),
    }
}

pub(super) fn dataset_file_status(name: &str, response: i32) -> String {
    match (name, response) {
        ("NOTFND", _) => "23",
        ("DUPREC" | "DUPKEY", _) => "22",
        ("ENDFILE", _) => "10",
        ("LENGERR", _) => "44",
        ("INVREQ", _) => "39",
        _ => "30",
    }
    .into()
}
