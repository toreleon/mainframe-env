//! Independent complete COPY images and named BMS values, loaded from private evidence.
use super::*;
use std::io::Read;

pub(super) type SegmentImage = (String, Option<Vec<u8>>, Vec<u8>);
pub(super) type CallImage = (String, Vec<SegmentImage>);

const ORACLE_SHA256: &str = "6b71b5056f41ac4cf85886f854425bfa6f3fb2b3727aae1417b56f5764182ad6";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Oracle {
    pub(super) schema: String,
    pub(super) account: String,
    pub(super) root: Vec<u8>,
    pub(super) children: Vec<Vec<u8>>,
    pub(super) fields: BTreeMap<String, Vec<u8>>,
    field_lengths: BTreeMap<String, usize>,
    expected_ims_statuses: Vec<String>,
    whole_private_guest_storage_postimage_claimed: bool,
    authority: String,
}

impl Oracle {
    pub(super) fn read(path: &Path) -> Result<Self, CorpusProblem> {
        let parent = path.parent().ok_or_else(problem)?;
        let parent_meta = fs::symlink_metadata(parent).map_err(|_| problem())?;
        require_owned_directory(parent, (parent_meta.dev(), parent_meta.ino()))?;
        let before = fs::symlink_metadata(path).map_err(|_| problem())?;
        need(
            before.is_file()
                && !before.file_type().is_symlink()
                && before.nlink() == 1
                && before.permissions().mode() & 0o777 == 0o600
                && before.len() <= 65_536
                && path.canonicalize().map_err(|_| problem())? == path,
        )?;
        let mut file = fs::File::open(path).map_err(|_| problem())?;
        let held = file.metadata().map_err(|_| problem())?;
        need(
            (
                held.dev(),
                held.ino(),
                held.len(),
                held.mode(),
                held.nlink(),
            ) == (
                before.dev(),
                before.ino(),
                before.len(),
                before.mode(),
                before.nlink(),
            ),
        )?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| problem())?;
        let held_after = file.metadata().map_err(|_| problem())?;
        need(
            (
                held_after.dev(),
                held_after.ino(),
                held_after.len(),
                held_after.mode(),
                held_after.nlink(),
            ) == (
                before.dev(),
                before.ino(),
                before.len(),
                before.mode(),
                before.nlink(),
            ),
        )?;
        let after = fs::symlink_metadata(path).map_err(|_| problem())?;
        require_owned_directory(parent, (parent_meta.dev(), parent_meta.ino()))?;
        need(
            (
                after.dev(),
                after.ino(),
                after.len(),
                after.mode(),
                after.nlink(),
            ) == (
                before.dev(),
                before.ino(),
                before.len(),
                before.mode(),
                before.nlink(),
            ) && bytes.len() as u64 == before.len()
                && format!("{:x}", Sha256::digest(&bytes)) == ORACLE_SHA256,
        )?;
        let oracle: Self = serde_json::from_slice(&bytes).map_err(|_| problem())?;
        oracle.validate()?;
        Ok(oracle)
    }
    fn validate(&self) -> Result<(), CorpusProblem> {
        let lengths = FIELD_LENGTHS
            .iter()
            .map(|(name, width)| ((*name).into(), *width))
            .collect::<BTreeMap<String, usize>>();
        need(
            self.schema == "external.carddemo-copaus0-readonly-oracle@1"
                && self.account == "00000000050"
                && self.root.len() == 100
                && self.root[..6] == [0, 0, 0, 0, 5, 0x0c]
                && self.children.len() == 5
                && self.children.iter().all(|child| child.len() == 200)
                && !self.whole_private_guest_storage_postimage_claimed
                && !self.authority.is_empty()
                && self.authority.len() <= 512
                && self.field_lengths == lengths
                && self.fields.len() == lengths.len()
                && self
                    .fields
                    .iter()
                    .all(|(name, value)| lengths.get(name) == Some(&value.len()))
                && self.expected_ims_statuses == ["  ", "  ", "  ", "  ", "  ", "  ", "  ", "GE"],
        )
    }
    pub(super) fn image(&self) -> ImsLoadImage {
        ImsLoadImage {
            database: "DBPAUTP0".into(),
            roots: vec![ImsLoadRoot {
                data: self.root.clone(),
                children: self.children.clone(),
            }],
        }
    }
    pub(super) fn compare_segments(&self, segments: &[SegmentImage]) -> Result<(), CorpusProblem> {
        let mut expected = vec![("PAUTSUM0".into(), None, self.root.clone())];
        expected.extend(self.children.iter().map(|child| {
            (
                "PAUTDTL1".into(),
                Some(self.root[..6].to_vec()),
                child.clone(),
            )
        }));
        need(segments == expected)
    }
    pub(super) fn compare_fields(
        &self,
        fields: &BTreeMap<String, Vec<u8>>,
    ) -> Result<(), CorpusProblem> {
        need(fields == &self.fields)
    }
    pub(super) fn compare_results(&self, results: &[CallImage]) -> Result<(), CorpusProblem> {
        need(results.len() == 8)?;
        for (index, (status, segments)) in results.iter().enumerate() {
            need(status == &self.expected_ims_statuses[index])?;
            match index {
                0 | 7 => need(segments.is_empty())?,
                1 => need(segments == &[("PAUTSUM0".into(), None, self.root.clone())])?,
                2..=6 => need(
                    segments
                        == &[(
                            "PAUTDTL1".into(),
                            Some(self.root[..6].to_vec()),
                            self.children[index - 2].clone(),
                        )],
                )?,
                _ => return Err(problem()),
            }
        }
        Ok(())
    }
}

fn problem() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.ims.firstparty_readonly_oracle",
        "fixed independent oracle differs",
    )
}
fn need(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(problem()) }
}

pub(super) fn named_field(name: &str) -> bool {
    FIELD_LENGTHS.iter().any(|(expected, _)| *expected == name)
}

// Generated only from the independently inspected pinned COPAU00 DFHMDF text.
// No semantic model, parse_bms, installed map, response or guest value is used.
const FIELD_LENGTHS: &[(&str, usize)] = &[
    ("ACCSTAT", 1),
    ("ACCTID", 11),
    ("ADDR001", 25),
    ("ADDR002", 25),
    ("APPRAMT", 10),
    ("APPRCNT", 3),
    ("CASHBAL", 9),
    ("CASHLIM", 9),
    ("CNAME", 25),
    ("CREDBAL", 12),
    ("CREDLIM", 12),
    ("CURDATE", 8),
    ("CURTIME", 8),
    ("CUSTID", 9),
    ("DECLAMT", 10),
    ("DECLCNT", 3),
    ("ERRMSG", 78),
    ("PAMT001", 12),
    ("PAMT002", 12),
    ("PAMT003", 12),
    ("PAMT004", 12),
    ("PAMT005", 12),
    ("PAPRV01", 1),
    ("PAPRV02", 1),
    ("PAPRV03", 1),
    ("PAPRV04", 1),
    ("PAPRV05", 1),
    ("PDATE01", 8),
    ("PDATE02", 8),
    ("PDATE03", 8),
    ("PDATE04", 8),
    ("PDATE05", 8),
    ("PGMNAME", 8),
    ("PHONE1", 13),
    ("PSTAT01", 1),
    ("PSTAT02", 1),
    ("PSTAT03", 1),
    ("PSTAT04", 1),
    ("PSTAT05", 1),
    ("PTIME01", 8),
    ("PTIME02", 8),
    ("PTIME03", 8),
    ("PTIME04", 8),
    ("PTIME05", 8),
    ("PTYPE01", 4),
    ("PTYPE02", 4),
    ("PTYPE03", 4),
    ("PTYPE04", 4),
    ("PTYPE05", 4),
    ("SEL0001", 1),
    ("SEL0002", 1),
    ("SEL0003", 1),
    ("SEL0004", 1),
    ("SEL0005", 1),
    ("TITLE01", 40),
    ("TITLE02", 40),
    ("TRNID01", 16),
    ("TRNID02", 16),
    ("TRNID03", 16),
    ("TRNID04", 16),
    ("TRNID05", 16),
    ("TRNNAME", 4),
];

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Oracle {
        let mut root = vec![b'R'; 100];
        root[..6].copy_from_slice(&[0, 0, 0, 0, 5, 0x0c]);
        let children = (b'A'..=b'E').map(|value| vec![value; 200]).collect();
        let lengths = FIELD_LENGTHS
            .iter()
            .map(|(name, width)| ((*name).into(), *width))
            .collect::<BTreeMap<String, usize>>();
        Oracle {
            schema: "external.carddemo-copaus0-readonly-oracle@1".into(),
            account: "00000000050".into(),
            root,
            children,
            fields: lengths
                .iter()
                .map(|(name, width)| {
                    (
                        name.clone(),
                        vec![if name == "ACCSTAT" { b' ' } else { 0 }; *width],
                    )
                })
                .collect(),
            field_lengths: lengths,
            expected_ims_statuses: vec![
                "  ".into(),
                "  ".into(),
                "  ".into(),
                "  ".into(),
                "  ".into(),
                "  ".into(),
                "  ".into(),
                "GE".into(),
            ],
            whole_private_guest_storage_postimage_claimed: false,
            authority: "synthetic refusal control".into(),
        }
    }
    #[test]
    fn complete_copy_images_refuse_filler_parent_order_width_and_eof_drift() {
        let oracle = fixture();
        oracle.validate().unwrap();
        let mut segments = vec![("PAUTSUM0".into(), None, oracle.root.clone())];
        segments.extend(oracle.children.iter().map(|child| {
            (
                "PAUTDTL1".into(),
                Some(oracle.root[..6].to_vec()),
                child.clone(),
            )
        }));
        oracle.compare_segments(&segments).unwrap();
        for index in 0..segments.len() {
            let mut changed = segments.clone();
            *changed[index].2.last_mut().unwrap() ^= 1; // ignored filler is still compared
            assert!(oracle.compare_segments(&changed).is_err());
            let mut changed = segments.clone();
            changed[index].2.pop();
            assert!(oracle.compare_segments(&changed).is_err());
        }
        let mut changed = segments.clone();
        changed.swap(1, 2);
        assert!(oracle.compare_segments(&changed).is_err());
        let mut changed = segments.clone();
        changed[1].1 = Some(vec![0; 6]);
        assert!(oracle.compare_segments(&changed).is_err());
        let mut results = vec![
            ("  ".into(), vec![]),
            ("  ".into(), vec![segments[0].clone()]),
        ];
        results.extend(
            segments[1..]
                .iter()
                .map(|segment| ("  ".into(), vec![segment.clone()])),
        );
        results.push(("GE".into(), vec![]));
        oracle.compare_results(&results).unwrap();
        let mut changed = results.clone();
        changed[7].0 = "  ".into();
        assert!(oracle.compare_results(&changed).is_err());
        let mut changed = results.clone();
        changed[7].1.push(segments[5].clone());
        assert!(oracle.compare_results(&changed).is_err());
        results.pop();
        assert!(oracle.compare_results(&results).is_err());
    }
    #[test]
    fn complete_named_map_refuses_unknown_missing_binary_and_width_drift() {
        let oracle = fixture();
        oracle.compare_fields(&oracle.fields).unwrap();
        let mut changed = oracle.fields.clone();
        changed.insert("ACCSTAT".into(), vec![0]);
        assert!(oracle.compare_fields(&changed).is_err());
        for name in [
            "PGMNAME", "TRNNAME", "ACCTID", "ERRMSG", "PAPRV01", "PSTAT04", "PAMT005", "ACCSTAT",
        ] {
            let mut changed = oracle.fields.clone();
            changed.get_mut(name).unwrap()[0] ^= 1;
            assert!(oracle.compare_fields(&changed).is_err());
            let mut changed = oracle.fields.clone();
            changed.remove(name);
            assert!(oracle.compare_fields(&changed).is_err());
        }
        let mut changed = oracle.fields.clone();
        changed.insert("FOREIGN".into(), vec![]);
        assert!(oracle.compare_fields(&changed).is_err());
        let mut changed = oracle.fields.clone();
        changed.get_mut("TRNID05").unwrap().pop();
        assert!(oracle.compare_fields(&changed).is_err());
    }
}
