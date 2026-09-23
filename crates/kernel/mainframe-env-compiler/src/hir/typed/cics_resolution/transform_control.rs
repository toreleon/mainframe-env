use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_value, complete_data_reference};
use crate::{CobolUsage, SemanticModel};

pub(super) struct TransformShape {
    pub(super) clauses: &'static [&'static str],
    pub(super) options: &'static [&'static str],
    pub(super) required: &'static [&'static str],
}

pub(super) fn shape(operation: HirCicsOperation) -> Option<TransformShape> {
    let (clauses, required): (&'static [&'static str], &'static [&'static str]) = match operation {
        HirCicsOperation::TransformDataToJson | HirCicsOperation::TransformJsonToData => (
            &[
                "CHANNEL",
                "INCONTAINER",
                "OUTCONTAINER",
                "TRANSFORMER",
                "RESP",
                "RESP2",
            ],
            &["CHANNEL", "INCONTAINER", "TRANSFORMER"],
        ),
        HirCicsOperation::TransformDataToXml => (
            &[
                "CHANNEL",
                "DATCONTAINER",
                "ELEMNAME",
                "ELEMNAMELEN",
                "ELEMNS",
                "ELEMNSLEN",
                "RESP",
                "RESP2",
                "TYPENAME",
                "TYPENAMELEN",
                "TYPENS",
                "TYPENSLEN",
                "XMLCONTAINER",
                "XMLTRANSFORM",
            ],
            &["CHANNEL", "DATCONTAINER", "XMLCONTAINER", "XMLTRANSFORM"],
        ),
        HirCicsOperation::TransformXmlToData => (
            &[
                "CHANNEL",
                "DATCONTAINER",
                "ELEMNAME",
                "ELEMNAMELEN",
                "ELEMNS",
                "ELEMNSLEN",
                "NSCONTAINER",
                "RESP",
                "RESP2",
                "TYPENAME",
                "TYPENAMELEN",
                "TYPENS",
                "TYPENSLEN",
                "XMLCONTAINER",
                "XMLTRANSFORM",
            ],
            &["CHANNEL", "XMLCONTAINER"],
        ),
        _ => return None,
    };
    Some(TransformShape {
        clauses,
        options: &["NOHANDLE"],
        required,
    })
}

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    let Some(shape) = shape(operation) else {
        return Ok(());
    };
    let label = match operation {
        HirCicsOperation::TransformDataToJson => "DATATOJSON",
        HirCicsOperation::TransformJsonToData => "JSONTODATA",
        HirCicsOperation::TransformDataToXml => "DATATOXML",
        HirCicsOperation::TransformXmlToData => "XMLTODATA",
        _ => unreachable!("transform operation was checked above"),
    };
    for required in shape.required {
        if !clauses.contains_key(*required) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS TRANSFORM {label} requires {required}"
            )));
        }
    }
    if operation == HirCicsOperation::TransformXmlToData
        && clauses.contains_key("XMLTRANSFORM")
        && !clauses.contains_key("DATCONTAINER")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS TRANSFORM XMLTODATA requires DATCONTAINER with XMLTRANSFORM".into(),
        ));
    }
    if matches!(
        operation,
        HirCicsOperation::TransformDataToXml | HirCicsOperation::TransformXmlToData
    ) {
        for (text, length) in metadata_clauses() {
            if clauses.contains_key(text) != clauses.contains_key(length) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS TRANSFORM {label} requires {text} and {length} together"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let text_operands = match operation {
        HirCicsOperation::TransformDataToJson | HirCicsOperation::TransformJsonToData => &[
            ("CHANNEL", HirCicsOperandName::Channel),
            ("INCONTAINER", HirCicsOperandName::InContainer),
            ("OUTCONTAINER", HirCicsOperandName::OutContainer),
            ("TRANSFORMER", HirCicsOperandName::Transformer),
        ][..],
        HirCicsOperation::TransformDataToXml => &[
            ("CHANNEL", HirCicsOperandName::Channel),
            ("DATCONTAINER", HirCicsOperandName::DataContainer),
            ("XMLCONTAINER", HirCicsOperandName::XmlContainer),
            ("XMLTRANSFORM", HirCicsOperandName::XmlTransform),
        ][..],
        HirCicsOperation::TransformXmlToData => &[
            ("CHANNEL", HirCicsOperandName::Channel),
            ("DATCONTAINER", HirCicsOperandName::DataContainer),
            ("NSCONTAINER", HirCicsOperandName::NsContainer),
            ("XMLCONTAINER", HirCicsOperandName::XmlContainer),
            ("XMLTRANSFORM", HirCicsOperandName::XmlTransform),
        ][..],
        _ => return Ok(Vec::new()),
    };
    let mut operands = text_operands
        .iter()
        .filter_map(|(name, identity)| clauses.get(*name).map(|value| (identity, value)))
        .map(|(name, value)| {
            Ok(HirCicsNamedOperand {
                name: *name,
                value: cics_value(value, semantic)?,
            })
        })
        .collect::<Resolution<Vec<_>>>()?;
    if operation == HirCicsOperation::TransformXmlToData {
        for (name, identity) in [
            ("ELEMNAME", HirCicsOperandName::ElementName),
            ("ELEMNS", HirCicsOperandName::ElementNamespace),
            ("TYPENAME", HirCicsOperandName::TypeName),
            ("TYPENS", HirCicsOperandName::TypeNamespace),
        ] {
            let Some(value) = clauses.get(name) else {
                continue;
            };
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
            operands.push(HirCicsNamedOperand {
                name: identity,
                value: HirCicsValue::Data(target),
            });
        }
    }
    if matches!(
        operation,
        HirCicsOperation::TransformDataToXml | HirCicsOperation::TransformXmlToData
    ) {
        let label = if operation == HirCicsOperation::TransformDataToXml {
            "DATATOXML"
        } else {
            "XMLTODATA"
        };
        for (name, identity) in [
            ("ELEMNAMELEN", HirCicsOperandName::ElementNameLength),
            ("ELEMNSLEN", HirCicsOperandName::ElementNamespaceLength),
            ("TYPENAMELEN", HirCicsOperandName::TypeNameLength),
            ("TYPENSLEN", HirCicsOperandName::TypeNamespaceLength),
        ] {
            let Some(value) = clauses.get(name) else {
                continue;
            };
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
            if !matches!(target.usage, CobolUsage::Binary | CobolUsage::NativeBinary)
                || target.length != 4
                || target.scale != 0
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS TRANSFORM {label} {name} requires writable fullword binary storage"
                )));
            }
            operands.push(HirCicsNamedOperand {
                name: identity,
                value: HirCicsValue::Data(target),
            });
        }
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !matches!(
        operation,
        HirCicsOperation::TransformDataToXml | HirCicsOperation::TransformXmlToData
    ) {
        return Ok(Vec::new());
    }
    let mut outputs = Vec::new();
    for (text, length, text_identity, length_identity) in [
        (
            "ELEMNAME",
            "ELEMNAMELEN",
            HirCicsOutputName::ElementName,
            HirCicsOutputName::ElementNameLength,
        ),
        (
            "ELEMNS",
            "ELEMNSLEN",
            HirCicsOutputName::ElementNamespace,
            HirCicsOutputName::ElementNamespaceLength,
        ),
        (
            "TYPENAME",
            "TYPENAMELEN",
            HirCicsOutputName::TypeName,
            HirCicsOutputName::TypeNameLength,
        ),
        (
            "TYPENS",
            "TYPENSLEN",
            HirCicsOutputName::TypeNamespace,
            HirCicsOutputName::TypeNamespaceLength,
        ),
    ] {
        let Some(text_value) = clauses.get(text) else {
            continue;
        };
        let text_target = complete_data_reference(text_value, semantic)?;
        let length_target = complete_data_reference(&clauses[length], semantic)?;
        require_writable(&text_target)?;
        require_writable(&length_target)?;
        outputs.extend([
            HirCicsOutputBinding {
                name: text_identity,
                target: text_target,
            },
            HirCicsOutputBinding {
                name: length_identity,
                target: length_target,
            },
        ]);
    }
    Ok(outputs)
}

fn metadata_clauses() -> [(&'static str, &'static str); 4] {
    [
        ("ELEMNAME", "ELEMNAMELEN"),
        ("ELEMNS", "ELEMNSLEN"),
        ("TYPENAME", "TYPENAMELEN"),
        ("TYPENS", "TYPENSLEN"),
    ]
}
