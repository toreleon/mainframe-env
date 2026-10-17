//! Current @3 grammar: length-prefixed roles, records, counts and option presence.
//! Called only after resource preflight; historical @2 encoding stays in its owner.

use super::*;

struct Frame(Sha256);

impl Frame {
    fn leaf(&mut self, bytes: &[u8]) {
        digest_field(&mut self.0, bytes);
    }

    fn record(&mut self, role: &str) {
        self.leaf(b"record");
        self.leaf(role.as_bytes());
    }

    fn field(&mut self, role: &str, bytes: &[u8]) {
        self.leaf(b"field");
        self.leaf(role.as_bytes());
        self.leaf(bytes);
    }

    fn text(&mut self, role: &str, value: &str) {
        self.field(role, value.as_bytes());
    }

    fn number(&mut self, role: &str, value: u64) {
        self.field(role, &value.to_be_bytes());
    }

    fn sequence(&mut self, role: &str, count: usize) -> Result<(), InstallProblem> {
        let count = u64::try_from(count).map_err(|_| InstallProblem::LimitExceeded)?;
        self.leaf(b"sequence");
        self.leaf(role.as_bytes());
        self.leaf(&count.to_be_bytes());
        Ok(())
    }

    fn presence(&mut self, role: &str, present: bool) {
        self.leaf(b"option");
        self.leaf(role.as_bytes());
        self.leaf(&[u8::from(present)]);
    }

    fn optional_text(&mut self, role: &str, value: Option<&str>) {
        self.presence(role, value.is_some());
        if let Some(value) = value {
            self.text("value", value);
        }
    }

    fn optional_json<T: Serialize>(
        &mut self,
        role: &str,
        contract: &str,
        value: Option<&T>,
    ) -> Result<(), InstallProblem> {
        self.presence(role, value.is_some());
        if let Some(value) = value {
            self.text("contract", contract);
            self.field(
                "json",
                &serde_json::to_vec(value).map_err(|_| InstallProblem::InvalidIdentity)?,
            );
        }
        Ok(())
    }

    fn map(&mut self, role: &str, values: &BTreeMap<String, String>) -> Result<(), InstallProblem> {
        self.sequence(role, values.len())?;
        for (key, value) in values {
            self.record("pair");
            self.text("key", key);
            self.text("value", value);
        }
        Ok(())
    }
}

pub(super) fn identity(package: &ApplicationPackageV2) -> Result<String, InstallProblem> {
    let manifest = &package.base.manifest;
    for value in [&manifest.name, &manifest.version, &manifest.target_product] {
        validate_text(value)?;
    }
    let mut frame = Frame(Sha256::new());
    frame.leaf(APPLICATION_PACKAGE_V3_CONTRACT.as_bytes());
    frame.record("package");
    frame.number("generation", package.generation);
    frame.record("base_manifest");
    frame.text("contract", crate::APPLICATION_PACKAGE_CONTRACT);
    frame.text("name", &manifest.name);
    frame.text("version", &manifest.version);
    frame.text("target_product", &manifest.target_product);
    frame.sequence("entries", manifest.entries.len())?;
    for entry in &manifest.entries {
        frame.record("entry");
        frame.text("path", &entry.path);
        frame.text("kind", entry.kind.slug());
        frame.text("sha256", &entry.sha256);
        frame.number(
            "bytes",
            u64::try_from(entry.bytes).map_err(|_| InstallProblem::LimitExceeded)?,
        );
        frame.sequence("depends_on", entry.depends_on.len())?;
        for dependency in &entry.depends_on {
            frame.text("dependency", dependency);
        }
    }
    let sections = &package.sections;
    frame.record("sections");
    frame.text("schema_version", &sections.schema_version);
    frame.sequence("host_abi_libraries", sections.host_abi_libraries.len())?;
    for library in sorted_by(&sections.host_abi_libraries, |item| &item.id) {
        frame.record("abi_library");
        frame.text("id", &library.id);
        frame.text("subsystem", library.subsystem.slug());
        frame.text("version", &library.version);
        frame.sequence("members", library.members.len())?;
        for member in sorted_by(&library.members, |item| &item.name) {
            frame.record("abi_member");
            frame.text("name", &member.name);
            frame.text("blob_sha256", &member.blob_sha256);
        }
    }
    frame.sequence("sql_tables", sections.sql_tables.len())?;
    for table in sorted_by(&sections.sql_tables, |item| &item.name) {
        frame.record("sql_table");
        frame.text("name", &table.name);
        frame.sequence("columns", table.columns.len())?;
        for column in &table.columns {
            frame.record("sql_column");
            frame.text("name", &column.name);
            frame.field("nullable", &[u8::from(column.nullable)]);
        }
        frame.sequence("primary_key", table.primary_key.len())?;
        for key in &table.primary_key {
            frame.text("key_column", key);
        }
    }
    frame.sequence("sql_rows", sections.sql_rows.len())?;
    let mut sql_rows = sections.sql_rows.iter().collect::<Vec<_>>();
    sql_rows.sort_by(|left, right| {
        left.table
            .cmp(&right.table)
            .then(left.values.cmp(&right.values))
    });
    for row in sql_rows {
        frame.record("sql_row");
        frame.text("table", &row.table);
        frame.map("values", &row.values)?;
    }
    frame.sequence("ims_definitions", sections.ims_definitions.len())?;
    for definition in sorted_by(&sections.ims_definitions, |item| &item.name) {
        frame.record("ims_definition");
        frame.text("name", &definition.name);
        frame.sequence("segments", definition.segments.len())?;
        for segment in &definition.segments {
            frame.text("segment", segment);
        }
    }
    frame.sequence("ims_rows", sections.ims_rows.len())?;
    let mut ims_rows = sections.ims_rows.iter().collect::<Vec<_>>();
    ims_rows.sort_by(|left, right| {
        left.definition
            .cmp(&right.definition)
            .then(left.segment.cmp(&right.segment))
            .then(left.values.cmp(&right.values))
    });
    for row in ims_rows {
        frame.record("ims_row");
        frame.text("definition", &row.definition);
        frame.text("segment", &row.segment);
        frame.map("values", &row.values)?;
    }
    frame.optional_json(
        "ims_metadata",
        IMS_METADATA_SECTION_CONTRACT,
        sections.ims_metadata.as_ref(),
    )?;
    frame.optional_json("ims_tm", IMS_TM_SECTION_CONTRACT, sections.ims_tm.as_ref())?;
    frame.sequence("mq_resources", sections.mq_resources.len())?;
    for resource in sorted_by(&sections.mq_resources, |item| &item.name) {
        frame.record("mq_resource");
        frame.text("name", &resource.name);
        frame.text("kind", resource.kind.slug());
        frame.optional_text("target", resource.target.as_deref());
        frame.optional_text("controller", resource.controller.as_deref());
    }
    frame.sequence("batch_controllers", sections.batch_controllers.len())?;
    for controller in sorted_by(&sections.batch_controllers, |item| &item.name) {
        frame.record("batch_controller");
        frame.text("name", &controller.name);
        frame.text("program", &controller.program);
        frame.text("kind", controller.kind.slug());
        frame.map("properties", &controller.properties)?;
    }
    frame.sequence("security_resources", sections.security_resources.len())?;
    let mut security = sections.security_resources.iter().collect::<Vec<_>>();
    security.sort_by(|left, right| {
        left.class
            .cmp(&right.class)
            .then(left.profile.cmp(&right.profile))
            .then(left.owner.cmp(&right.owner))
    });
    for resource in security {
        frame.record("security_resource");
        frame.text("class", &resource.class);
        frame.text("profile", &resource.profile);
        frame.text("owner", &resource.owner);
    }
    Ok(format!("sha256:{:x}", frame.0.finalize()))
}
