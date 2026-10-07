use super::*;

pub(super) fn online_screen_fields(
    terminal: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<u8>>, CorpusProblem> {
    let bytes =
        base64::engine::general_purpose::STANDARD
            .decode(terminal["screen_base64"].as_str().ok_or_else(|| {
                CorpusProblem::new("carddemo.online.response", "screen is missing")
            })?)
            .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    let mut at = 0usize;
    let mut fields = BTreeMap::new();
    while at < bytes.len() {
        let name_length = u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field name is truncated")
                })?
                .try_into()
                .map_err(|_| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field name is invalid")
                })?,
        ) as usize;
        at += 4;
        let name_end = at.checked_add(name_length).ok_or_else(|| {
            CorpusProblem::new("carddemo.online.screen_invalid", "field name is too large")
        })?;
        let name = String::from_utf8(
            bytes
                .get(at..name_end)
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field name is truncated")
                })?
                .to_vec(),
        )
        .map_err(|_| {
            CorpusProblem::new("carddemo.online.screen_invalid", "field name is invalid")
        })?;
        at = name_end;
        let value_length = u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field value is truncated")
                })?
                .try_into()
                .map_err(|_| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field value is invalid")
                })?,
        ) as usize;
        at += 4;
        let value_end = at.checked_add(value_length).ok_or_else(|| {
            CorpusProblem::new("carddemo.online.screen_invalid", "field value is too large")
        })?;
        let value = bytes
            .get(at..value_end)
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.online.screen_invalid", "field value is truncated")
            })?
            .to_vec();
        at = value_end;
        if fields.insert(name, value).is_some() || fields.len() > 512 {
            return Err(CorpusProblem::new(
                "carddemo.online.screen_invalid",
                "screen fields are duplicated or unbounded",
            ));
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, value: &[u8]) -> Vec<u8> {
        let mut bytes = (name.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value);
        bytes
    }

    fn screen(bytes: &[u8]) -> serde_json::Value {
        serde_json::json!({"screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
    }

    #[test]
    fn screen_fields_preserve_binary_values_and_empty_fields() {
        let mut bytes = field("NAME", b"A\0B");
        bytes.extend(field("EMPTY", b""));
        let decoded = online_screen_fields(&screen(&bytes)).unwrap();
        assert_eq!(decoded["NAME"], b"A\0B");
        assert!(decoded["EMPTY"].is_empty());
    }

    #[test]
    fn screen_fields_reject_every_truncated_boundary() {
        let bytes = field("NAME", b"DATA");
        for end in 1..bytes.len() {
            assert!(
                online_screen_fields(&screen(&bytes[..end])).is_err(),
                "prefix {end}"
            );
        }
    }

    #[test]
    fn screen_fields_reject_duplicates_and_invalid_base64() {
        let mut bytes = field("NAME", b"FIRST");
        bytes.extend(field("NAME", b"SECOND"));
        assert!(online_screen_fields(&screen(&bytes)).is_err());
        assert!(online_screen_fields(&serde_json::json!({"screen_base64":"!"})).is_err());
    }
}
