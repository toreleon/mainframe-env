use super::*;

pub(super) fn carddemo_base_maps(
    corpus_dir: &Path,
    semantic_models: &[SemanticModel],
) -> Result<Vec<BmsMapDefinition>, CorpusProblem> {
    carddemo_maps(corpus_dir, &["app/bms"], semantic_models)
}

pub(super) fn carddemo_maps(
    corpus_dir: &Path,
    roots: &[&str],
    semantic_models: &[SemanticModel],
) -> Result<Vec<BmsMapDefinition>, CorpusProblem> {
    let mut maps = Vec::new();
    for relative in collect_paths(corpus_dir, roots, "bms")? {
        maps.push(carddemo_map(corpus_dir, &relative, semantic_models)?);
    }
    Ok(maps)
}

pub(super) fn carddemo_map(
    corpus_dir: &Path,
    relative: &str,
    semantic_models: &[SemanticModel],
) -> Result<BmsMapDefinition, CorpusProblem> {
    let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
        .map_err(|_| CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS is not UTF-8"))?;
    let parsed = parse_bms(&source).map_err(package_problem)?;
    let input_root_name = format!("{}I", parsed.name);
    let output_root_name = format!("{}O", parsed.name);
    let symbolic = if semantic_models.is_empty() {
        None
    } else {
        Some(
            semantic_models
                .iter()
                .find_map(|semantic| {
                    let input = semantic
                        .layouts
                        .iter()
                        .find(|layout| layout.name.eq_ignore_ascii_case(&input_root_name))?;
                    let output = semantic
                        .layouts
                        .iter()
                        .find(|layout| layout.name.eq_ignore_ascii_case(&output_root_name))?;
                    Some((semantic, input, output))
                })
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.terminal.bms_layout_missing",
                        format!("{output_root_name} symbolic output layout is missing"),
                    )
                })?,
        )
    };
    let (rows, columns) = parsed.size.ok_or_else(|| {
        CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS size is missing")
    })?;
    let mut definitions = Vec::new();
    for field in &parsed.fields {
        let Some(name) = field.name.clone() else {
            continue;
        };
        let (row, column) = field.position.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.terminal.bms_invalid",
                format!("{name} position is missing"),
            )
        })?;
        let length = field.length.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.terminal.bms_invalid",
                format!("{name} length is missing"),
            )
        })?;
        let attributes = field
            .attributes
            .iter()
            .map(|value| value.to_ascii_uppercase())
            .collect::<BTreeSet<_>>();
        let output_name = format!("{name}O");
        let output_offset = symbolic
            .map(|(semantic, _, output_root)| {
                let output = semantic
                    .layouts
                    .iter()
                    .find(|layout| {
                        layout.name.eq_ignore_ascii_case(&output_name)
                            && layout.offset >= output_root.offset
                            && layout.offset.saturating_add(layout.length)
                                <= output_root.offset.saturating_add(output_root.length)
                    })
                    .ok_or_else(|| {
                        CorpusProblem::new(
                            "carddemo.terminal.bms_layout_missing",
                            format!("{output_name} is missing from {output_root_name}"),
                        )
                    })?;
                if output.length != length {
                    return Err(CorpusProblem::new(
                        "carddemo.terminal.bms_layout_mismatch",
                        format!("{output_name} length differs from BMS"),
                    ));
                }
                u32::try_from(output.offset - output_root.offset).map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.terminal.bms_layout_invalid",
                        format!("{output_name} offset is too large"),
                    )
                })
            })
            .transpose()?;
        let attribute_name = format!("{name}A");
        let attribute_offset = symbolic
            .map(|(semantic, input_root, _)| {
                let attribute = semantic
                    .layouts
                    .iter()
                    .find(|layout| {
                        layout.name.eq_ignore_ascii_case(&attribute_name)
                            && layout.offset >= input_root.offset
                            && layout.offset.saturating_add(layout.length)
                                <= input_root.offset.saturating_add(input_root.length)
                    })
                    .ok_or_else(|| {
                        CorpusProblem::new(
                            "carddemo.terminal.bms_layout_missing",
                            format!("{attribute_name} is missing from {input_root_name}"),
                        )
                    })?;
                u32::try_from(attribute.offset - input_root.offset).map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.terminal.bms_layout_invalid",
                        format!("{attribute_name} offset is too large"),
                    )
                })
            })
            .transpose()?;
        definitions.push(BmsFieldDefinition {
            name,
            row: u16::try_from(row).map_err(|_| {
                CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS row is too large")
            })?,
            column: u16::try_from(column).map_err(|_| {
                CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS column is too large")
            })?,
            length: u16::try_from(length).map_err(|_| {
                CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS field is too large")
            })?,
            initial: field.initial.clone().unwrap_or_default().into_bytes(),
            color: parsed.field_color(field).map(str::to_owned),
            highlight: parsed.field_highlight(field).map(str::to_owned),
            protected: attributes.contains("PROT") || attributes.contains("ASKIP"),
            secret: attributes.contains("DRK"),
            fset: attributes.contains("FSET"),
            justify_right: field
                .justify
                .iter()
                .any(|value| value.eq_ignore_ascii_case("RIGHT")),
            fill_zero: field
                .justify
                .iter()
                .any(|value| value.eq_ignore_ascii_case("ZERO")),
            output_offset,
            attribute_offset,
        });
    }
    Ok(BmsMapDefinition {
        mapset: parsed.mapset,
        map: parsed.name,
        line: u16::try_from(parsed.line.unwrap_or(1)).map_err(|_| {
            CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS line is too large")
        })?,
        column: u16::try_from(parsed.column.unwrap_or(1)).map_err(|_| {
            CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS column is too large")
        })?,
        rows: u16::try_from(rows).map_err(|_| {
            CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS rows are too large")
        })?,
        columns: u16::try_from(columns).map_err(|_| {
            CorpusProblem::new("carddemo.terminal.bms_invalid", "BMS columns are too large")
        })?,
        fields: definitions,
    })
}

#[cfg(all(test, target_os = "linux"))]
mod metadata_tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};

    #[test]
    fn actual_adapter_preserves_explicit_extended_metadata_without_default_resolution() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "carddemo-bms-declared-options-{}-{nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let before = fs::symlink_metadata(&root).unwrap();
        let identity = (before.dev(), before.ino());
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("synthetic.bms"))
            .unwrap();
        file.write_all(b"SET DFHMSD COLOR=BLUE,HILIGHT=BLINK,EXTATT=YES\nMAP DFHMDI SIZE=(24,80),COLOR=RED,HILIGHT=REVERSE,DSATTS=(COLOR,HILIGHT),MAPATTS=(COLOR,HILIGHT)\nFIRST DFHMDF LENGTH=2,POS=(1,1)\nSECOND DFHMDF LENGTH=2,POS=(2,1),COLOR=DEFAULT,HILIGHT=OFF").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let actual = carddemo_map(&root, "synthetic.bms", &[]).unwrap();
        assert_eq!(actual.fields.len(), 2);
        assert_eq!(actual.fields[0].color.as_deref(), Some("RED"));
        assert_eq!(actual.fields[0].highlight.as_deref(), Some("REVERSE"));
        assert_eq!(actual.fields[1].color.as_deref(), Some("DEFAULT"));
        assert_eq!(actual.fields[1].highlight.as_deref(), Some("OFF"));
        // This synthetic parser/adapter control emits no application observations.
        let after = fs::symlink_metadata(&root).unwrap();
        assert!(after.is_dir());
        assert_eq!((after.dev(), after.ino()), identity);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            fs::symlink_metadata(&root).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
