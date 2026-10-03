//! Generated, source-reviewed IMS call-site applicability; this is validation, not execution.

use crate::{ImsExecutionContext, ImsPcbKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source-level CALL versus command syntax selected before applicability lookup.
pub enum ImsCallSyntax {
    /// Use the source CALL spelling vocabulary.
    Call,
    /// Use the source command spelling vocabulary.
    Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Class of already validated PROCOPT, not a parser or permission grant.
pub enum ImsProcessingOptionClass {
    /// Validated read PROCOPT class.
    Read,
    /// Validated replace PROCOPT class.
    Replace,
    /// Validated insert PROCOPT class.
    Insert,
    /// Validated delete PROCOPT class.
    Delete,
    /// Validated all-operations PROCOPT class.
    All,
    /// Validated read-without-integrity class, distinct from ordinary read.
    ReadWithoutIntegrity,
}

/// Bounded SSA equivalence classes; parsing and command-code validation belong to `ims`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsSsaForm {
    /// No SSA operand.
    Absent,
    /// Parsed unqualified segment form.
    Unqualified,
    /// Parsed field-predicate qualification.
    Qualified,
    /// Parsed path command form.
    Path,
    /// Parsed subset-pointer command form.
    SubsetPointer,
    /// Parsed exact concatenated-key qualification.
    ConcatenatedKey,
    /// Parsed explicit record-search byte slice.
    RecordSearchArgument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Row-qualified call context; identical spellings can belong to different source families.
pub struct ImsCallSite<'a> {
    /// Exact source family identity; spelling alone can be ambiguous.
    pub official_row: String,
    /// CALL/command spelling checked case-insensitively within the selected family.
    pub name: &'a str,
    /// Select CALL versus command name vocabulary for this family.
    pub syntax: ImsCallSyntax,
    /// Explicit execution environment checked against the candidate variant.
    pub context: ImsExecutionContext,
    /// Optional PCB family; absence must match a PCB-free variant.
    pub pcb_kind: Option<ImsPcbKind>,
    /// Optional exact organization label checked against candidate membership.
    pub organization: Option<&'a str>,
    /// A class assigned only after the PCB's raw PROCOPT has been validated.
    pub processing_option: Option<ImsProcessingOptionClass>,
    /// A class assigned only after the SSA parser has accepted the raw bytes.
    pub ssa_form: ImsSsaForm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Generated admissible call-site combination; metadata membership is separate from execution support.
pub struct ImsCallVariant {
    /// Generated source applicability profile label.
    pub profile: &'static str,
    /// Exact permitted execution environments.
    pub contexts: &'static [ImsExecutionContext],
    /// Required PCB kind, or None for a PCB-free form.
    pub pcb_kind: Option<ImsPcbKind>,
    /// Exact permitted organization labels; empty means no organization operand.
    pub organizations: &'static [&'static str],
    /// Admitted classes of already validated PROCOPT.
    pub processing_options: &'static [ImsProcessingOptionClass],
    /// Admitted classes of already parsed SSAs.
    pub ssa_forms: &'static [ImsSsaForm],
    /// Optional variant-specific spellings, matched case-insensitively.
    pub names: &'static [&'static str],
    /// Pinned source topic locators for this applicability variant.
    pub source_topics: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One source family and its accepted names/variants; catalog presence earns no execution coverage.
pub struct ImsCallApplicabilityDescriptor {
    /// Generated source-family ordinal, not a runtime dispatch number.
    pub ordinal: u8,
    /// Exact official source-family identity.
    pub official_row: &'static str,
    /// Admitted CALL spellings within this source family.
    pub call_names: &'static [&'static str],
    /// Admitted command spellings within this source family.
    pub command_names: &'static [&'static str],
    /// Exact admissible combinations, not installed implementations.
    pub variants: &'static [ImsCallVariant],
    /// Catalog and contract presence never earn execution coverage.
    pub executed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Exact call-site admission failure; no fallback to spelling-only dispatch is implied.
pub enum ImsApplicabilityProblem {
    /// Official row identity is not in the generated catalog.
    UnknownFamily,
    /// CALL/command name does not belong to the selected row and syntax.
    WrongSpelling,
    /// No candidate permits the execution environment.
    ForbiddenContext,
    /// No candidate permits the supplied PCB family.
    ForbiddenPcbKind,
    /// No candidate permits the supplied organization.
    ForbiddenOrganization,
    /// No candidate permits the validated PROCOPT class.
    ForbiddenProcessingOption,
    /// Required PCB/organization selection was omitted.
    MissingPcbOrOrganization,
    /// Parsed SSA class or variant-specific name is not admitted.
    ForbiddenSsaForm,
    /// A PCB-free/organization-free family was supplied one.
    UnexpectedPcbOrOrganization,
}

include!("generated/ims_call_applicability.rs");

/// Validate a *row-qualified* call site. A spelling alone is intentionally ambiguous.
pub fn validate_ims_call_site(
    site: &ImsCallSite<'_>,
) -> Result<&'static ImsCallApplicabilityDescriptor, ImsApplicabilityProblem> {
    let family = IMS_CALL_APPLICABILITY
        .iter()
        .find(|family| family.official_row == site.official_row)
        .ok_or(ImsApplicabilityProblem::UnknownFamily)?;
    let names = match site.syntax {
        ImsCallSyntax::Call => family.call_names,
        ImsCallSyntax::Command => family.command_names,
    };
    if !names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(site.name))
    {
        return Err(ImsApplicabilityProblem::WrongSpelling);
    }

    let candidates = family
        .variants
        .iter()
        .filter(|variant| variant.pcb_kind == site.pcb_kind)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(if site.pcb_kind.is_none() {
            ImsApplicabilityProblem::MissingPcbOrOrganization
        } else if family
            .variants
            .iter()
            .all(|variant| variant.pcb_kind.is_none())
        {
            ImsApplicabilityProblem::UnexpectedPcbOrOrganization
        } else {
            ImsApplicabilityProblem::ForbiddenPcbKind
        });
    }
    let candidates = candidates
        .into_iter()
        .filter(|variant| variant.contexts.contains(&site.context))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(ImsApplicabilityProblem::ForbiddenContext);
    }
    let candidates = candidates
        .into_iter()
        .filter(|variant| match site.organization {
            Some(organization) => variant.organizations.contains(&organization),
            None => variant.organizations.is_empty(),
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(if site.organization.is_none() {
            ImsApplicabilityProblem::MissingPcbOrOrganization
        } else if family
            .variants
            .iter()
            .all(|variant| variant.organizations.is_empty())
        {
            ImsApplicabilityProblem::UnexpectedPcbOrOrganization
        } else {
            ImsApplicabilityProblem::ForbiddenOrganization
        });
    }
    let candidates = candidates
        .into_iter()
        .filter(|variant| match site.processing_option {
            Some(option) => variant.processing_options.contains(&option),
            None => variant.processing_options.is_empty(),
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(ImsApplicabilityProblem::ForbiddenProcessingOption);
    }
    if !candidates.iter().any(|variant| {
        variant.ssa_forms.contains(&site.ssa_form)
            && (variant.names.is_empty()
                || variant
                    .names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(site.name)))
    }) {
        return Err(ImsApplicabilityProblem::ForbiddenSsaForm);
    }
    Ok(family)
}
