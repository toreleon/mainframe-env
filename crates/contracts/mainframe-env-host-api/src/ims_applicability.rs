//! Generated, source-reviewed IMS call-site applicability; this is validation, not execution.

use crate::{ImsExecutionContext, ImsPcbKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsCallSyntax {
    Call,
    Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsProcessingOptionClass {
    Read,
    Replace,
    Insert,
    Delete,
    All,
    ReadWithoutIntegrity,
}

/// Bounded SSA equivalence classes; parsing and command-code validation belong to `ims`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsSsaForm {
    Absent,
    Unqualified,
    Qualified,
    Path,
    SubsetPointer,
    ConcatenatedKey,
    RecordSearchArgument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsCallSite<'a> {
    pub official_row: String,
    pub name: &'a str,
    pub syntax: ImsCallSyntax,
    pub context: ImsExecutionContext,
    pub pcb_kind: Option<ImsPcbKind>,
    pub organization: Option<&'a str>,
    /// A class assigned only after the PCB's raw PROCOPT has been validated.
    pub processing_option: Option<ImsProcessingOptionClass>,
    /// A class assigned only after the SSA parser has accepted the raw bytes.
    pub ssa_form: ImsSsaForm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsCallVariant {
    pub profile: &'static str,
    pub contexts: &'static [ImsExecutionContext],
    pub pcb_kind: Option<ImsPcbKind>,
    pub organizations: &'static [&'static str],
    pub processing_options: &'static [ImsProcessingOptionClass],
    pub ssa_forms: &'static [ImsSsaForm],
    pub names: &'static [&'static str],
    pub source_topics: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsCallApplicabilityDescriptor {
    pub ordinal: u8,
    pub official_row: &'static str,
    pub call_names: &'static [&'static str],
    pub command_names: &'static [&'static str],
    pub variants: &'static [ImsCallVariant],
    /// Catalog and contract presence never earn execution coverage.
    pub executed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsApplicabilityProblem {
    UnknownFamily,
    WrongSpelling,
    ForbiddenContext,
    ForbiddenPcbKind,
    ForbiddenOrganization,
    ForbiddenProcessingOption,
    MissingPcbOrOrganization,
    ForbiddenSsaForm,
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
