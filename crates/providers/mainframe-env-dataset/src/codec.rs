use mainframe_env_host_api::{DatasetAttributes, DatasetOrganization, RecordFormat};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    pub attributes: DatasetAttributes,
    pub version: u64,
    pub records: Vec<Vec<u8>>,
    pub members: BTreeMap<String, Vec<Vec<u8>>>,
}

pub(crate) fn encode(entry: &Entry) -> Result<Vec<u8>, ()> {
    let mut out = b"MEDS1".to_vec();
    out.push(org(entry.attributes.organization));
    out.push(recfm(entry.attributes.record_format));
    u32v(&mut out, entry.attributes.logical_record_length);
    optional_u32(&mut out, entry.attributes.key_offset);
    optional_u32(&mut out, entry.attributes.key_length);
    optional_u16(&mut out, entry.attributes.ccsid);
    u64v(&mut out, entry.version);
    records(&mut out, &entry.records)?;
    u32v(
        &mut out,
        u32::try_from(entry.members.len()).map_err(|_| ())?,
    );
    for (name, value) in &entry.members {
        bytes(&mut out, name.as_bytes())?;
        records(&mut out, value)?;
    }
    Ok(out)
}
pub(crate) fn decode(
    bytes_in: &[u8],
    max_records: usize,
    max_record: usize,
    max_members: usize,
) -> Result<Entry, ()> {
    let mut r = Reader {
        bytes: bytes_in,
        at: 0,
    };
    if r.take(5)? != b"MEDS1" {
        return Err(());
    }
    let organization = org_back(r.byte()?)?;
    let record_format = recfm_back(r.byte()?)?;
    let logical_record_length = r.u32()?;
    let key_offset = r.optional_u32()?;
    let key_length = r.optional_u32()?;
    let ccsid = r.optional_u16()?;
    let version = r.u64()?;
    let records = r.records(max_records, max_record)?;
    let count = usize::try_from(r.u32()?).map_err(|_| ())?;
    if count > max_members {
        return Err(());
    }
    let mut members = BTreeMap::new();
    for _ in 0..count {
        let name = String::from_utf8(r.bytes(128)?).map_err(|_| ())?;
        if members
            .insert(name, r.records(max_records, max_record)?)
            .is_some()
        {
            return Err(());
        }
    }
    if r.at != bytes_in.len() {
        return Err(());
    }
    Ok(Entry {
        attributes: DatasetAttributes {
            organization,
            record_format,
            logical_record_length,
            key_offset,
            key_length,
            ccsid,
        },
        version,
        records,
        members,
    })
}
fn org(value: DatasetOrganization) -> u8 {
    match value {
        DatasetOrganization::Sequential => 0,
        DatasetOrganization::Partitioned => 1,
        DatasetOrganization::KeySequenced => 2,
    }
}
fn org_back(value: u8) -> Result<DatasetOrganization, ()> {
    match value {
        0 => Ok(DatasetOrganization::Sequential),
        1 => Ok(DatasetOrganization::Partitioned),
        2 => Ok(DatasetOrganization::KeySequenced),
        _ => Err(()),
    }
}
fn recfm(value: RecordFormat) -> u8 {
    match value {
        RecordFormat::Fixed => 0,
        RecordFormat::FixedBlocked => 1,
        RecordFormat::Variable => 2,
        RecordFormat::VariableBlocked => 3,
        RecordFormat::Undefined => 4,
        RecordFormat::Line => 5,
    }
}
fn recfm_back(value: u8) -> Result<RecordFormat, ()> {
    match value {
        0 => Ok(RecordFormat::Fixed),
        1 => Ok(RecordFormat::FixedBlocked),
        2 => Ok(RecordFormat::Variable),
        3 => Ok(RecordFormat::VariableBlocked),
        4 => Ok(RecordFormat::Undefined),
        5 => Ok(RecordFormat::Line),
        _ => Err(()),
    }
}
fn u16v(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes())
}
fn u32v(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes())
}
fn u64v(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes())
}
fn optional_u32(out: &mut Vec<u8>, v: Option<u32>) {
    out.push(u8::from(v.is_some()));
    if let Some(v) = v {
        u32v(out, v)
    }
}
fn optional_u16(out: &mut Vec<u8>, v: Option<u16>) {
    out.push(u8::from(v.is_some()));
    if let Some(v) = v {
        u16v(out, v)
    }
}
fn bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), ()> {
    u32v(out, u32::try_from(value.len()).map_err(|_| ())?);
    out.extend_from_slice(value);
    Ok(())
}
fn records(out: &mut Vec<u8>, values: &[Vec<u8>]) -> Result<(), ()> {
    u32v(out, u32::try_from(values.len()).map_err(|_| ())?);
    for value in values {
        bytes(out, value)?;
    }
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ()> {
        let end = self.at.checked_add(n).ok_or(())?;
        let out = self.bytes.get(self.at..end).ok_or(())?;
        self.at = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8, ()> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ()> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| ())?,
        ))
    }
    fn u32(&mut self) -> Result<u32, ()> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| ())?,
        ))
    }
    fn u64(&mut self) -> Result<u64, ()> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| ())?,
        ))
    }
    fn optional_u32(&mut self) -> Result<Option<u32>, ()> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.u32()?)),
            _ => Err(()),
        }
    }
    fn optional_u16(&mut self) -> Result<Option<u16>, ()> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.u16()?)),
            _ => Err(()),
        }
    }
    fn bytes(&mut self, max: usize) -> Result<Vec<u8>, ()> {
        let n = usize::try_from(self.u32()?).map_err(|_| ())?;
        if n > max {
            return Err(());
        }
        Ok(self.take(n)?.to_vec())
    }
    fn records(&mut self, max_count: usize, max_record: usize) -> Result<Vec<Vec<u8>>, ()> {
        let count = usize::try_from(self.u32()?).map_err(|_| ())?;
        if count > max_count {
            return Err(());
        }
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.bytes(max_record)?);
        }
        Ok(out)
    }
}
