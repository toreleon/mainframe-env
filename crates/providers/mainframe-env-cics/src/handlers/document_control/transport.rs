use super::*;

const RETRIEVED_MAGIC: &[u8; 8] = b"MECRTV01";

pub(super) fn encode_retrieved(
    document: &DocumentRecord,
    limits: CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    let tagged = !document.bookmarks.is_empty()
        || document
            .segments
            .iter()
            .any(|segment| segment.binary || segment.host_code_page != 37);
    if !tagged {
        let mut plain = Vec::new();
        for segment in &document.segments {
            append_bounded(&mut plain, &segment.bytes, limits.max_screen_bytes)?;
        }
        return Ok(plain);
    }
    let mut out = RETRIEVED_MAGIC.to_vec();
    out.extend_from_slice(
        &u32::try_from(document.segments.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for segment in &document.segments {
        out.push(u8::from(segment.binary));
        out.extend_from_slice(&segment.host_code_page.to_be_bytes());
        field(&mut out, &segment.bytes)?;
    }
    out.extend_from_slice(
        &u32::try_from(document.bookmarks.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, position) in &document.bookmarks {
        field(&mut out, name.as_bytes())?;
        out.extend_from_slice(
            &u64::try_from(*position)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
    }
    if out.len() > limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(out)
}

pub(super) fn decode_from_buffer(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<Option<(Vec<DocumentSegment>, BTreeMap<String, usize>)>, HostProblem> {
    if !bytes.starts_with(RETRIEVED_MAGIC) {
        return Ok(None);
    }
    parse(bytes, limits)
        .map(Some)
        .map_err(|_| HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 1,
        })
}

fn parse(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<(Vec<DocumentSegment>, BTreeMap<String, usize>), HostProblem> {
    if bytes.len() > limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut reader = Reader {
        bytes,
        at: RETRIEVED_MAGIC.len(),
    };
    let count = reader_u32(&mut reader)? as usize;
    if count > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut segments = Vec::with_capacity(count);
    let mut size = 0usize;
    for _ in 0..count {
        let binary = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::Malformed),
        };
        let host_code_page = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        );
        let content = reader.field(limits.max_screen_bytes)?;
        size = size
            .checked_add(content.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if host_code_page == 0 || size > limits.max_screen_bytes {
            return Err(HostProblem::Malformed);
        }
        segments.push(DocumentSegment {
            bytes: content,
            binary,
            host_code_page,
        });
    }
    let count = reader_u32(&mut reader)? as usize;
    if count > limits.max_document_bookmarks {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bookmarks = BTreeMap::new();
    for _ in 0..count {
        let name = String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::Malformed)?;
        let position =
            usize::try_from(reader_u64(&mut reader)?).map_err(|_| HostProblem::Malformed)?;
        if name.is_empty()
            || name.len() > 16
            || !name.bytes().all(|byte| byte.is_ascii_graphic())
            || position > size
            || bookmarks.insert(name, position).is_some()
        {
            return Err(HostProblem::Malformed);
        }
    }
    if reader.at != bytes.len() {
        return Err(HostProblem::Malformed);
    }
    Ok((segments, bookmarks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieved_tagged_content_restores_binary_blocks_and_bookmarks() {
        let document = DocumentRecord {
            token: [0; 16],
            owner_execution: "exec".into(),
            owner_run_unit: "run".into(),
            transaction: "MENU".into(),
            segments: vec![
                DocumentSegment {
                    bytes: b"TEXT".to_vec(),
                    binary: false,
                    host_code_page: 37,
                },
                DocumentSegment {
                    bytes: vec![0, 255],
                    binary: true,
                    host_code_page: 37,
                },
            ],
            symbols: BTreeMap::new(),
            bookmarks: BTreeMap::from([("Mark".into(), 4)]),
            version: 1,
        };
        let limits = CicsLimits::default();
        let encoded = encode_retrieved(&document, limits).unwrap();
        assert!(encoded.len() > document.retrieval_size());
        assert_eq!(
            decode_from_buffer(&encoded, limits).unwrap(),
            Some((document.segments, document.bookmarks))
        );
        let mut malformed = encoded;
        malformed.pop();
        assert_eq!(
            decode_from_buffer(&malformed, limits),
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 1
            })
        );
    }
}
