//! Allocation-bounded JSON preflight and borrowed retained-state serialization.
//! Serde owns JSON syntax and typed decoding; this pass retains only bounded keys and counters.
use super::*;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeSeq, SerializeStruct};
use std::borrow::Cow;
use std::fmt;
use std::io::{self, Write};

struct CappedWriter {
    bytes: Vec<u8>,
    count: usize,
    maximum: usize,
    exceeded: bool,
    retain: bool,
}

impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(count) = self
            .count
            .checked_add(bytes.len())
            .filter(|n| *n <= self.maximum)
        else {
            self.exceeded = true;
            return Err(io::Error::other("package output limit exceeded"));
        };
        self.count = count;
        if self.retain {
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode<T: Serialize + ?Sized>(
    value: &T,
    maximum: usize,
    retain: bool,
) -> Result<CappedWriter, InstallProblem> {
    let mut writer = CappedWriter {
        bytes: Vec::new(),
        count: 0,
        maximum,
        exceeded: false,
        retain,
    };
    if serde_json::to_writer(&mut writer, value).is_err() {
        return Err(if writer.exceeded {
            InstallProblem::LimitExceeded
        } else {
            InstallProblem::InvalidIdentity
        });
    }
    Ok(writer)
}

pub(super) fn json_size<T: Serialize + ?Sized>(
    value: &T,
    maximum: usize,
) -> Result<usize, InstallProblem> {
    Ok(encode(value, maximum, false)?.count)
}

struct StateRef<'a>(&'a BTreeMap<String, InstalledApplication>);
struct ApplicationsRef<'a>(&'a BTreeMap<String, InstalledApplication>);
struct ApplicationRef<'a>(&'a str, &'a InstalledApplication);
struct GenerationsRef<'a>(&'a InstalledApplication);

impl Serialize for StateRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("ApplicationInstallerState", 2)?;
        state.serialize_field("schema_version", APPLICATION_INSTALLER_STATE_CONTRACT)?;
        state.serialize_field("applications", &ApplicationsRef(self.0))?;
        state.end()
    }
}
impl Serialize for ApplicationsRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for (name, installed) in self.0 {
            sequence.serialize_element(&ApplicationRef(name, installed))?;
        }
        sequence.end()
    }
}
impl Serialize for ApplicationRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut application = serializer.serialize_struct("RetainedApplication", 3)?;
        application.serialize_field("application", self.0)?;
        application.serialize_field("selected", &self.1.selected)?;
        application.serialize_field("generations", &GenerationsRef(self.1))?;
        application.end()
    }
}
impl Serialize for GenerationsRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.packages.len()))?;
        for (generation, package) in &self.0.packages {
            let record = self
                .0
                .generations
                .get(generation)
                .ok_or_else(|| serde::ser::Error::custom("missing retained generation"))?;
            #[derive(Serialize)]
            struct RetainedRef<'a> {
                package: &'a ApplicationPackageV2,
                state: InstallState,
            }
            sequence.serialize_element(&RetainedRef {
                package,
                state: record.state,
            })?;
        }
        sequence.end()
    }
}

pub(super) fn export(
    applications: &BTreeMap<String, InstalledApplication>,
    maximum: usize,
) -> Result<Vec<u8>, InstallProblem> {
    // Preserve the former missing-generation error before starting serialization.
    if applications.values().any(|app| {
        app.packages
            .keys()
            .any(|key| !app.generations.contains_key(key))
    }) {
        return Err(InstallProblem::UnknownStage);
    }
    Ok(encode(&StateRef(applications), maximum, true)?.bytes)
}

#[derive(Clone, Copy)]
enum Shape {
    Object(&'static str),
    Array(&'static str),
    Text(usize, bool),
    Label,
    Scalar,
}

#[derive(Default)]
struct Count {
    items: usize,
    text: usize,
    blobs: usize,
    json: usize,
    label: Option<String>,
}

fn add<E: de::Error>(left: usize, right: usize) -> Result<usize, E> {
    left.checked_add(right)
        .ok_or_else(|| E::custom("package counter overflow"))
}
impl Count {
    fn merge<E: de::Error>(&mut self, other: Self) -> Result<(), E> {
        self.items = add(self.items, other.items)?;
        self.text = add(self.text, other.text)?;
        self.blobs = add(self.blobs, other.blobs)?;
        Ok(())
    }
    fn check<E: de::Error>(&self, limits: PackageLimits) -> Result<(), E> {
        if self.items > limits.max_total_nested_items
            || self.text > limits.max_total_section_bytes
            || self.blobs > limits.max_total_blob_bytes
        {
            return Err(E::custom("package limit exceeded"));
        }
        Ok(())
    }
    fn footprint<E: de::Error>(&self) -> Result<usize, E> {
        add(
            add(self.text, self.blobs)?,
            self.items
                .checked_mul(256)
                .ok_or_else(|| E::custom("package counter overflow"))?,
        )
    }
}

// Structural roles select existing bounds; they do not replace the typed schemas.
fn child(role: &str, key: &str, limits: PackageLimits) -> Option<Shape> {
    use Shape::*;
    let text = Text(
        256,
        !matches!(
            role,
            "metadata"
                | "database"
                | "segment"
                | "field"
                | "index"
                | "relation"
                | "psb"
                | "pcb"
                | "sensitive"
                | "tm"
                | "transaction"
                | "alternate"
                | "destination"
        ),
    );
    Some(match (role, key) {
        ("state", "schema_version") | ("application", "application") => Text(256, false),
        ("state", "applications") => Array("applications"),
        ("application", "selected") | ("retained", "state") | ("package", "generation") => Scalar,
        ("application", "generations") => Array("generations"),
        ("retained", "package") => Object("package"),
        ("package", "base") => Object("base"),
        ("package", "sections") => Object("sections"),
        ("package", "signature") => Object("signature"),
        ("base", "manifest") => Object("manifest"),
        ("base", "blobs") => Object("blobs"),
        ("manifest", "entries") => Array("entries"),
        ("entry", "depends_on") => Array("dependencies"),
        ("entry", "path") | ("controller", "program") => Text(4096, true),
        ("entry", "sha256") | ("member", "blob_sha256") => Text(71, true),
        ("entry", "kind" | "bytes")
        | ("abi", "subsystem")
        | ("column", "nullable")
        | ("mq" | "controller", "kind") => Scalar,
        ("sections", "host_abi_libraries") => Array("abi"),
        ("sections", "sql_tables") => Array("tables"),
        ("sections", "sql_rows") => Array("sql_rows"),
        ("sections", "ims_definitions") => Array("definitions"),
        ("sections", "ims_rows") => Array("ims_rows"),
        ("sections", "mq_resources") => Array("mq"),
        ("sections", "batch_controllers") => Array("controllers"),
        ("sections", "security_resources") => Array("security"),
        ("sections", "ims_metadata") => Object("metadata"),
        ("sections", "ims_tm") => Object("tm"),
        ("abi", "members") => Array("members"),
        ("table", "columns") => Array("columns"),
        ("table", "primary_key") => Array("keys"),
        ("definition", "segments") => Array("segments"),
        ("sql_row" | "ims_row", "values") => Object("values"),
        ("controller", "properties") => Object("properties"),
        ("blobs", _) => Array("blob"),
        ("values" | "properties", _) => Text(limits.max_value_bytes, true),
        ("sql_row", "table") | ("ims_row", "definition") => Label,
        ("metadata", "databases") => Array("databases"),
        ("metadata", "psbs") => Array("psbs"),
        ("database", "gsam_format") => Object("gsam_format"),
        (
            "gsam_format",
            "version" | "record_format" | "access_method" | "block_size" | "control",
        ) => Scalar,
        ("database", "segments") => Array("metadata_segments"),
        ("database", "secondary_indexes") => Array("indexes"),
        ("database", "logical_relationships") => Array("relationships"),
        ("segment", "fields") => Array("fields"),
        ("index", "source_fields") => Array("source_fields"),
        ("psb", "pcbs") => Array("pcbs"),
        ("pcb", "sensitive_segments") => Array("sensitive"),
        ("tm", "transactions") => Array("transactions"),
        ("transaction", "alternate_pcbs") => Array("alternates"),
        ("alternate", "destination") => Object("destination"),
        ("destination", "fixed") => text,
        ("database", "version" | "organization")
        | ("segment", "min_length" | "max_length")
        | ("field", "offset" | "length" | "sequence" | "unique")
        | ("relation", "paired")
        | ("psb", "database_level")
        | (
            "pcb",
            "kind" | "database_version" | "modifiable" | "express" | "same_terminal"
            | "response_mode",
        )
        | (
            "transaction",
            "context" | "priority" | "timeout_ticks" | "conversational" | "spa_size",
        )
        | ("alternate", "express") => Scalar,
        ("manifest", "name" | "version" | "target_product")
        | ("sections", "schema_version")
        | ("signature", "algorithm" | "key_id" | "value")
        | ("abi", "id" | "version")
        | ("member" | "column" | "table" | "definition", "name")
        | ("ims_row", "segment")
        | ("mq", "name" | "target" | "controller")
        | ("controller", "name")
        | ("security", "class" | "profile" | "owner")
        | ("metadata", "schema_version")
        | ("database", "name")
        | ("segment", "name" | "parent")
        | ("field", "name")
        | ("index", "name" | "target_segment" | "source_segment")
        | ("relation", "parent_database" | "parent_segment" | "child_database" | "child_segment")
        | ("psb", "name")
        | ("pcb", "name" | "database" | "secondary_index" | "processing_options" | "destination")
        | ("sensitive", "name" | "parent" | "processing_options")
        | (
            "transaction",
            "code" | "psb" | "program_selector" | "artifact" | "required_generation",
        )
        | ("alternate", "name") => text,
        _ => return None,
    })
}

fn array(role: &str, limits: PackageLimits) -> (usize, Shape, usize) {
    use Shape::*;
    let metadata = ImsMetadataLimits::default();
    let tm = TmLimits::default();
    match role {
        "applications" => (limits.max_applications, Object("application"), 0),
        "generations" => (limits.max_retained_generations, Object("retained"), 0),
        "entries" => (limits.max_manifest_entries, Object("entry"), 0),
        "dependencies" => (limits.max_dependencies_per_entry, Text(4096, true), 1),
        "abi" => (limits.max_items_per_section, Object("abi"), 0),
        "tables" => (limits.max_items_per_section, Object("table"), 0),
        "sql_rows" => (limits.max_items_per_section, Object("sql_row"), 0),
        "definitions" => (limits.max_items_per_section, Object("definition"), 0),
        "ims_rows" => (limits.max_items_per_section, Object("ims_row"), 0),
        "mq" => (limits.max_items_per_section, Object("mq"), 0),
        "controllers" => (limits.max_items_per_section, Object("controller"), 0),
        "security" => (limits.max_items_per_section, Object("security"), 0),
        "members" => (limits.max_members_per_abi_library, Object("member"), 0),
        "columns" => (limits.max_columns_per_sql_table, Object("column"), 0),
        "keys" => (limits.max_key_columns_per_sql_table, Text(256, true), 1),
        "segments" => (limits.max_segments_per_ims_definition, Text(256, true), 1),
        "blob" => (limits.max_total_blob_bytes, Scalar, 0),
        "databases" => (metadata.max_databases, Object("database"), 0),
        "psbs" => (metadata.max_psbs, Object("psb"), 0),
        "metadata_segments" => (metadata.max_segments_per_database, Object("segment"), 0),
        "fields" => (metadata.max_fields_per_segment, Object("field"), 0),
        "indexes" => (metadata.max_fields_per_database, Object("index"), 0),
        "relationships" => (
            metadata.max_relationships_per_database,
            Object("relation"),
            0,
        ),
        "source_fields" => (metadata.max_fields_per_segment, Text(256, false), 0),
        "pcbs" => (metadata.max_pcbs_per_psb, Object("pcb"), 0),
        "sensitive" => (
            metadata.max_sensitive_segments_per_psb,
            Object("sensitive"),
            0,
        ),
        "transactions" => (tm.max_transactions, Object("transaction"), 0),
        "alternates" => (tm.max_alternate_pcbs, Object("alternate"), 0),
        _ => unreachable!("private array role"),
    }
}

struct Seed<'a> {
    shape: Shape,
    limits: PackageLimits,
    problem: &'a mut Option<InstallProblem>,
    package: bool,
    optional: bool,
    allowed: bool,
}
impl Seed<'_> {
    fn fail<E: de::Error>(&mut self, problem: InstallProblem) -> E {
        *self.problem = Some(problem);
        E::custom("package preflight refused input")
    }
    fn check<E: de::Error>(&mut self, count: &Count) -> Result<(), E> {
        if self.package && count.check::<E>(self.limits).is_err()
            || self.optional && count.json > self.limits.max_total_section_bytes
        {
            return Err(self.fail(InstallProblem::LimitExceeded));
        }
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Count;
    fn deserialize<D: de::Deserializer<'de>>(mut self, deserializer: D) -> Result<Count, D::Error> {
        if !self.allowed {
            return Err(self.fail(InstallProblem::LimitExceeded));
        }
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Count;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded retained package JSON")
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Count, A::Error> {
        let Shape::Object(role) = self.shape else {
            return Err(de::Error::custom("unexpected object"));
        };
        if role == "package" && self.limits.max_sections < APPLICATION_SECTION_COUNT {
            return Err(self.fail(InstallProblem::LimitExceeded));
        }
        let maximum = match role {
            "blobs" => self.limits.max_manifest_entries,
            "values" => self.limits.max_fields_per_record,
            "properties" => self
                .limits
                .max_fields_per_record
                .min(self.limits.max_properties_per_controller),
            _ => self.limits.max_total_nested_items.max(32),
        };
        let weight = match role {
            "entry" | "abi" | "member" | "table" | "column" | "sql_row" | "definition"
            | "ims_row" | "controller" | "metadata" | "database" | "segment" | "field"
            | "index" | "relation" | "psb" | "pcb" | "sensitive" | "tm" | "transaction"
            | "alternate" => 1,
            "mq" | "security" => 3,
            "package" => 16,
            _ => 0,
        };
        let mut count = Count {
            items: weight,
            json: 2,
            ..Count::default()
        };
        let mut fields = BTreeSet::new();
        let mut json_fields = 0usize;
        while let Some(key) = map.next_key_seed(TextSeed(if role == "blobs" { 71 } else { 256 }))? {
            if fields.len() >= maximum {
                return Err(self.fail(InstallProblem::LimitExceeded));
            }
            if !fields.insert(key.clone()) {
                return Err(self.fail(InstallProblem::InvalidIdentity));
            }
            let Some(shape) = child(role, &key, self.limits) else {
                map.next_value::<IgnoredAny>()?;
                continue;
            };
            let optional = self.optional || matches!(shape, Shape::Object("metadata" | "tm"));
            let package = self.package || matches!(shape, Shape::Object("package"));
            let mut value = map.next_value_seed(Seed {
                shape,
                limits: self.limits,
                problem: self.problem,
                package,
                optional,
                allowed: true,
            })?;
            if matches!(role, "blobs" | "values" | "properties") {
                value.items = add(value.items, 1)?;
                value.text = add(value.text, key.len())?;
            }
            if matches!(
                (role, key.as_ref()),
                ("sql_row", "table") | ("ims_row", "definition")
            ) {
                count.label = value.label.take();
            }
            // Null additive fields are omitted by the historical writer.
            let skipped = value.json == 4
                && matches!(
                    (role, key.as_ref()),
                    ("sections", "ims_metadata" | "ims_tm")
                        | ("database", "gsam_format")
                        | ("pcb", "secondary_index")
                );
            if !skipped {
                count.json = add(
                    count.json,
                    add(
                        json_string_size::<A::Error>(&key)?,
                        add(value.json, 1 + usize::from(json_fields > 0))?,
                    )?,
                )?;
                json_fields += 1;
            }
            if role == "sections"
                && matches!(key.as_ref(), "ims_metadata" | "ims_tm")
                && value.items != 0
            {
                if self.limits.max_items_per_section == 0 {
                    return Err(self.fail(InstallProblem::LimitExceeded));
                }
                value.text = add(value.text, value.json)?;
            }
            count.merge(value)?;
            self.check(&count)?;
        }
        // Missing ordinary Option fields deserialize as None and serialize as null.
        let defaults: &[&str] = match role {
            "segment" => &["parent"],
            "field" => &["name"],
            "pcb" if fields.contains("database") => &["database_version"],
            "pcb" => &["destination"],
            "sensitive" => &["parent", "processing_options"],
            _ => &[],
        };
        for key in defaults.iter().filter(|key| !fields.contains(**key)) {
            count.json = add(count.json, key.len() + 7 + usize::from(json_fields > 0))?;
            json_fields += 1;
        }
        self.check(&count)?;
        Ok(count)
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<Count, A::Error> {
        let Shape::Array(role) = self.shape else {
            return Err(de::Error::custom("unexpected array"));
        };
        let (maximum, shape, weight) = array(role, self.limits);
        let mut count = Count {
            json: 2,
            ..Count::default()
        };
        let mut length = 0usize;
        let mut rows = BTreeMap::<String, usize>::new();
        while let Some(mut value) = seq.next_element_seed(Seed {
            shape,
            limits: self.limits,
            problem: self.problem,
            package: self.package,
            optional: self.optional,
            allowed: length < maximum,
        })? {
            value.items = add(value.items, weight)?;
            if role == "blob" {
                value.blobs = add(value.blobs, 1)?;
            }
            count.json = add(count.json, add(value.json, usize::from(length > 0))?)?;
            length = add(length, 1)?;
            if let Some(label) = value.label.take()
                && matches!(role, "sql_rows" | "ims_rows")
            {
                let tally = rows.entry(label.to_ascii_uppercase()).or_default();
                *tally = add(*tally, 1)?;
                let cap = if role == "sql_rows" {
                    self.limits.max_rows_per_sql_table
                } else {
                    self.limits.max_rows_per_ims_definition
                };
                if *tally > cap {
                    return Err(self.fail(InstallProblem::LimitExceeded));
                }
            }
            count.merge(value)?;
            self.check(&count)?;
            let (byte_cap, item_cap) = match role {
                "generations" => (
                    self.limits.max_retained_package_bytes,
                    self.limits.max_retained_nested_items,
                ),
                "applications" => (
                    self.limits.max_total_retained_package_bytes,
                    self.limits.max_total_retained_nested_items,
                ),
                _ => (usize::MAX, usize::MAX),
            };
            if count.items > item_cap || count.footprint::<A::Error>()? > byte_cap {
                return Err(self.fail(InstallProblem::LimitExceeded));
            }
        }
        Ok(count)
    }
    fn visit_str<E: de::Error>(mut self, value: &str) -> Result<Count, E> {
        let (maximum, counted) = match self.shape {
            Shape::Text(maximum, counted) => (maximum, counted),
            Shape::Label => (256, true),
            Shape::Scalar | Shape::Object("destination") => (256, false),
            _ => return Err(E::custom("unexpected string")),
        };
        if value.len() > maximum {
            return Err(self.fail(InstallProblem::LimitExceeded));
        }
        let mut count = Count {
            text: if counted { value.len() } else { 0 },
            json: json_string_size::<E>(value)?,
            ..Count::default()
        };
        self.check(&count)?;
        if matches!(self.shape, Shape::Label) {
            count.label = Some(value.to_owned());
        }
        Ok(count)
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Count, E> {
        self.scalar(json_size(&value, usize::MAX).map_err(|_| E::custom("invalid number"))?)
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Count, E> {
        self.scalar(json_size(&value, usize::MAX).map_err(|_| E::custom("invalid number"))?)
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Count, E> {
        self.scalar(json_size(&value, usize::MAX).map_err(|_| E::custom("invalid number"))?)
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Count, E> {
        self.scalar(if value { 4 } else { 5 })
    }
    fn visit_unit<E: de::Error>(self) -> Result<Count, E> {
        Ok(Count {
            json: 4,
            ..Count::default()
        })
    }
}
impl Seed<'_> {
    fn scalar<E: de::Error>(self, json: usize) -> Result<Count, E> {
        if !matches!(self.shape, Shape::Scalar) {
            return Err(E::custom("unexpected scalar"));
        }
        Ok(Count {
            json,
            ..Count::default()
        })
    }
}
fn json_string_size<E: de::Error>(value: &str) -> Result<usize, E> {
    json_size(value, usize::MAX).map_err(|_| E::custom("invalid JSON text"))
}

struct TextSeed(usize);
impl<'de> DeserializeSeed<'de> for TextSeed {
    type Value = Cow<'de, str>;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_str(self)
    }
}
impl<'de> Visitor<'de> for TextSeed {
    type Value = Cow<'de, str>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a bounded JSON member name")
    }
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        if v.len() > self.0 {
            return Err(E::custom("package key limit exceeded"));
        }
        Ok(Cow::Borrowed(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        if v.len() > self.0 {
            return Err(E::custom("package key limit exceeded"));
        }
        Ok(Cow::Owned(v.to_owned()))
    }
}

pub(super) fn preflight(payload: &[u8], limits: PackageLimits) -> Result<(), InstallProblem> {
    if payload.len() > limits.max_total_retained_package_bytes {
        return Err(InstallProblem::LimitExceeded);
    }
    let mut problem = None;
    let mut decoder = serde_json::Deserializer::from_slice(payload);
    Seed {
        shape: Shape::Object("state"),
        limits,
        problem: &mut problem,
        package: false,
        optional: false,
        allowed: true,
    }
    .deserialize(&mut decoder)
    .map_err(|error| {
        problem.unwrap_or_else(|| {
            if error.to_string().starts_with("package key limit exceeded") {
                InstallProblem::LimitExceeded
            } else {
                InstallProblem::InvalidIdentity
            }
        })
    })?;
    decoder.end().map_err(|_| InstallProblem::InvalidIdentity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct ObservedSequence<'a>(&'a Cell<usize>);
    impl Serialize for ObservedSequence<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(Some(1000))?;
            for _ in 0..1000 {
                self.0.set(self.0.get() + 1);
                sequence.serialize_element("abcdefghij")?;
            }
            sequence.end()
        }
    }

    #[test]
    fn package_bounds_writer_stops_visiting_at_output_cap_and_counter_retains_no_bytes() {
        let visited = Cell::new(0);
        assert!(matches!(
            encode(&ObservedSequence(&visited), 14, true),
            Err(InstallProblem::LimitExceeded)
        ));
        assert_eq!(visited.get(), 2);
        let exact = encode(&["abcdefghij"], 14, true).unwrap();
        assert_eq!(exact.bytes, b"[\"abcdefghij\"]");
        assert_eq!(exact.count, 14);
        let counted = encode(&["abcdefghij"], 14, false).unwrap();
        assert_eq!(counted.count, 14);
        assert!(counted.bytes.is_empty());
        assert!(matches!(
            encode(&["abcdefghij"], 13, true),
            Err(InstallProblem::LimitExceeded)
        ));
    }

    #[test]
    fn package_bounds_stream_refuses_excess_before_parsing_next_application() {
        let limits = PackageLimits {
            max_applications: 0,
            ..PackageLimits::default()
        };
        // A typed decode would report malformed JSON inside the application.
        let bytes = br#"{"schema_version":"mainframe-env.application-installer@1","applications":[{"malformed":!!!}]}"#;
        assert_eq!(preflight(bytes, limits), Err(InstallProblem::LimitExceeded));
        assert_eq!(
            preflight(bytes, PackageLimits::default()),
            Err(InstallProblem::InvalidIdentity)
        );
    }
}
