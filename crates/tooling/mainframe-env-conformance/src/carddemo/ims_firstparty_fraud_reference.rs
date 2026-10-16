//! Source-authored whole-row/whole-child/COMMAREA/named-BMS comparisons.
use super::*;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reference {
    pub(super) schema: String,
    pub(super) authority: String,
    pub(super) source_commit: String,
    pub(super) root: Vec<u8>,
    pub(super) original_child: Vec<u8>,
    pub(super) unchanged_neighbor: Vec<u8>,
    pub(super) expected_children: BTreeMap<String, Vec<u8>>,
    pub(super) columns: Vec<String>,
    pub(super) local_raw_rows: BTreeMap<String, Vec<Vec<u8>>>,
    pub(super) commareas: BTreeMap<String, Vec<u8>>,
    pub(super) field_lengths: BTreeMap<String, usize>,
    pub(super) fields: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    #[serde(rename = "COBOL_current_date")]
    pub(super) cobol_current_date: String,
    #[serde(rename = "CICS_reference_UTC_date")]
    pub(super) cics_reference_utc_date: String,
    #[serde(rename = "CICS_reference_epoch_day")]
    pub(super) cics_reference_epoch_day: u64,
    #[serde(rename = "CICS_MMDDYY")]
    pub(super) cics_mmddyy: String,
    #[serde(rename = "raw_SQL_CURRENT_DATE_is_not_a_license_parity_claim")]
    pub(super) raw_date_is_not_license_parity: bool,
    pub(super) known_current_provider_unheld_replace_status: String,
    pub(super) successful_source_acceptance_authorized: bool,
    pub(super) application_or_issue_credit: u64,
}
impl Reference {
    pub(super) fn independent() -> Result<Self, CorpusProblem> {
        let bytes = include_bytes!("fixtures/ims-fraud-source-reference.json");
        need(
            format!("{:x}", Sha256::digest(bytes))
                == "27e7c330b7a051809452daf229cc20934ee59fe152b8c09926ecbf862ff6d81a",
            "reference bytes differ",
        )?;
        let value: Self = serde_json::from_slice(bytes).map_err(|_| problem("reference shape"))?;
        value.validate()?;
        Ok(value)
    }
    pub(super) fn validate(&self) -> Result<(), CorpusProblem> {
        need(
            self.schema == "external.carddemo-original-fraud-source-reference@1"
                && self.source_commit == "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
                && !self.authority.is_empty()
                && self.root.len() == 100
                && self.original_child.len() == 200
                && self.unchanged_neighbor.len() == 200
                && self.original_child[..8] != self.unchanged_neighbor[..8]
                && self.columns.len() == 26
                && self.field_lengths.len() == 27
                && self.expected_children.len() == 2
                && self.local_raw_rows.len() == 2
                && self.commareas.len() == 2
                && self.fields.len() == 5
                && self.cobol_current_date == "2022070600000000+0000"
                && self.cics_reference_utc_date == "2026-10-16"
                && self.cics_reference_epoch_day == 20742
                && self.cics_mmddyy == "10/16/26"
                && self.raw_date_is_not_license_parity
                && self.known_current_provider_unheld_replace_status == "DJ"
                && !self.successful_source_acceptance_authorized
                && self.application_or_issue_credit == 0,
            "reference contract differs",
        )?;
        for action in ["F", "R"] {
            let child = self
                .expected_children
                .get(action)
                .ok_or_else(|| problem("child absent"))?;
            let row = self
                .local_raw_rows
                .get(action)
                .ok_or_else(|| problem("row absent"))?;
            let area = self
                .commareas
                .get(action)
                .ok_or_else(|| problem("COMMAREA absent"))?;
            let mut expected = self.original_child.clone();
            expected[174] = action.as_bytes()[0];
            expected[175..183].copy_from_slice(self.cics_mmddyy.as_bytes());
            need(
                *child == expected
                    && row.len() == 26
                    && row[22] == action.as_bytes()
                    && row[23] == self.cics_reference_utc_date.as_bytes()
                    && area.len() == 272
                    && area[20..220] == *child
                    && area[220..221] == *action.as_bytes()
                    && area[221] == b'S',
                "full source record contract differs",
            )?;
        }
        for fields in self.fields.values() {
            need(
                fields.len() == 27
                    && fields
                        .iter()
                        .all(|(name, bytes)| self.field_lengths.get(name) == Some(&bytes.len())),
                "full BMS field contract differs",
            )?;
        }
        Ok(())
    }
    pub(super) fn row(&self, action: &str, actual: &[Vec<u8>]) -> Result<(), CorpusProblem> {
        need(
            self.local_raw_rows
                .get(action)
                .is_some_and(|expected| expected == actual),
            "full row differs",
        )
    }
    pub(super) fn child_and_neighbor(
        &self,
        action: &str,
        actual: &[u8],
        neighbor: &[u8],
    ) -> Result<(), CorpusProblem> {
        need(
            self.expected_children
                .get(action)
                .is_some_and(|expected| expected == actual)
                && neighbor == self.unchanged_neighbor,
            "full child or unchanged neighbor differs",
        )
    }
    pub(super) fn commarea(&self, action: &str, actual: &[u8]) -> Result<(), CorpusProblem> {
        need(
            self.commareas
                .get(action)
                .is_some_and(|expected| expected == actual),
            "full COMMAREA differs",
        )
    }
    pub(super) fn screen(
        &self,
        phase: &str,
        actual: &BTreeMap<String, Vec<u8>>,
    ) -> Result<(), CorpusProblem> {
        need(
            self.fields
                .get(phase)
                .is_some_and(|expected| expected == actual),
            "full BMS screen differs",
        )
    }
}
