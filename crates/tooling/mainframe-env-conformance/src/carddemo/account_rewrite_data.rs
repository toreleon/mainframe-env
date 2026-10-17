//! Complete immediate source delta; reopened equality is separate retention evidence.
use super::*;
use mainframe_env_host_api::CatalogEntryKind;

pub(super) const ACCOUNT: &str = "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS";
pub(super) const CUSTOMER: &str = "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS";

pub(super) struct Expected {
    accounts: Vec<Vec<u8>>,
    customers: Vec<Vec<u8>>,
}

fn source_rows(corpus: &Path, name: &str, width: usize) -> Result<Vec<Vec<u8>>, CorpusProblem> {
    let bytes = read_corpus_file(corpus, &corpus.join(name))?;
    require(bytes.len() == width * 50)?;
    Ok(bytes.chunks_exact(width).map(<[u8]>::to_vec).collect())
}

impl Expected {
    #[cfg(test)]
    pub(super) fn fixture(accounts: Vec<Vec<u8>>, customers: Vec<Vec<u8>>) -> Self {
        Self {
            accounts,
            customers,
        }
    }

    pub(super) fn load(corpus: &Path) -> Result<Self, CorpusProblem> {
        for (path, expected) in oracles::SOURCES {
            let bytes = read_corpus_file(corpus, &corpus.join(path))?;
            require(format!("{:x}", Sha256::digest(bytes)) == *expected)?;
        }
        let result = Self {
            accounts: source_rows(corpus, "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCTDATA.PS", 300)?,
            customers: source_rows(corpus, "app/data/EBCDIC/AWS.M2.CARDDEMO.CUSTDATA.PS", 500)?,
        };
        require(
            result.accounts[48].as_slice() == oracles::ACCOUNT_BEFORE
                && result.customers[48].as_slice() == oracles::CUSTOMER_BEFORE,
        )?;
        let xref = source_rows(corpus, "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS", 50)?;
        require(
            xref.iter()
                .filter(|row| row.as_slice() == oracles::XREF_SELECTED)
                .count()
                == 1
                && oracles::XREF_SELECTED[16..25] == oracles::CUSTOMER_BEFORE[..9]
                && oracles::XREF_SELECTED[25..36] == oracles::ACCOUNT_BEFORE[..11],
        )?;
        Ok(result)
    }

    fn rows(&self, name: &str, rewritten: bool) -> Result<Vec<Vec<u8>>, CorpusProblem> {
        let (mut rows, replacement) = match name {
            ACCOUNT => (self.accounts.clone(), oracles::ACCOUNT_REWRITE.as_slice()),
            CUSTOMER => (self.customers.clone(), oracles::CUSTOMER_REWRITE.as_slice()),
            _ => return Err(refused()),
        };
        if rewritten {
            rows[48] = replacement.to_vec();
        }
        Ok(rows)
    }

    pub(super) fn compare(&self, state: &State, rewritten: bool) -> Result<(), CorpusProblem> {
        for name in [ACCOUNT, CUSTOMER] {
            let actual = state.get(name).ok_or_else(refused)?;
            let expected = self.rows(name, rewritten)?;
            let width = if name == ACCOUNT { 300 } else { 500 };
            let key = if name == ACCOUNT { 11 } else { 9 };
            require(
                actual.records == expected
                    && actual.identities
                        == expected
                            .iter()
                            .map(|row| row[..key].to_vec())
                            .collect::<Vec<_>>(),
            )?;
            require_metadata(actual, name, width, key as u32)?;
        }
        Ok(())
    }
}

fn require_metadata(actual: &Rows, name: &str, width: u32, key: u32) -> Result<(), CorpusProblem> {
    let attributes = DatasetAttributes {
        organization: DatasetOrganization::KeySequenced,
        record_format: RecordFormat::Fixed,
        logical_record_length: width,
        key_offset: Some(0),
        key_length: Some(key),
        ccsid: Some(37),
    };
    require(
        actual.attributes == attributes
            && actual.version > 0
            && actual.attributes_version == actual.version
            && actual.catalog.version == actual.version
            && actual.catalog.name.as_str() == name
            && actual.catalog.kind == CatalogEntryKind::Dataset
            && actual.catalog.related.is_none(),
    )
}

pub(super) fn immediate_delta(
    expected: &Expected,
    before: &State,
    after: &State,
) -> Result<(), CorpusProblem> {
    require(inserted::names_match(before) && inserted::names_match(after))?;
    expected.compare(before, false)?;
    expected.compare(after, true)?;
    for (name, old) in before {
        let new = after.get(name).ok_or_else(refused)?;
        if [ACCOUNT, CUSTOMER].contains(&name.as_str()) {
            let version = old.version.checked_add(1).ok_or_else(refused)?;
            let mut catalog = old.catalog.clone();
            catalog.version = version;
            require(
                new.version == version
                    && new.attributes_version == version
                    && new.catalog == catalog
                    && new.attributes == old.attributes,
            )?;
        } else {
            require(new == old)?;
        }
    }
    Ok(())
}

// After later independent journey mutations, only the selected literals remain
// an independent byte oracle. The complete final snapshot is separately retained
// solely for exact equality across reopen; it never revises these literals.
pub(super) fn selected_after(state: &State) -> Result<(), CorpusProblem> {
    for (name, expected, key) in [
        (ACCOUNT, oracles::ACCOUNT_REWRITE.as_slice(), 11usize),
        (CUSTOMER, oracles::CUSTOMER_REWRITE.as_slice(), 9usize),
    ] {
        let rows = state.get(name).ok_or_else(refused)?;
        let selected = rows
            .identities
            .iter()
            .enumerate()
            .filter(|(_, actual)| actual.as_slice() == &expected[..key])
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        require(rows.records.len() == 50 && rows.identities.len() == 50 && selected.len() == 1)?;
        require(rows.records.get(selected[0]).map(Vec::as_slice) == Some(expected))?;
        require_metadata(rows, name, expected.len() as u32, key as u32)?;
    }
    Ok(())
}
