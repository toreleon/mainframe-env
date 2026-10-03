//! Generated, source-reviewed IMS call-site applicability; this is validation, not execution.

use crate::{ImsExecutionContext, ImsPcbKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Caller syntax category used to select the row-qualified spelling set.
pub enum ImsCallSyntax {
    /// DL/I CALL spelling from the selected row.
    Call,
    /// Command spelling from the selected row.
    Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Validated processing-option class; raw PROCOPT validation remains with metadata.
pub enum ImsProcessingOptionClass {
    /// Read-capable processing-option class.
    Read,
    /// Replace-capable processing-option class.
    Replace,
    /// Insert-capable processing-option class.
    Insert,
    /// Delete-capable processing-option class.
    Delete,
    /// Combined processing-option capability class.
    All,
    /// Trusted metadata class for reads without integrity.
    ReadWithoutIntegrity,
}

/// Bounded SSA equivalence classes; parsing and command-code validation belong to `ims`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsSsaForm {
    /// No SSA or record-search operand is supplied.
    Absent,
    /// SSA names a segment without a predicate.
    Unqualified,
    /// SSA includes a comparative predicate.
    Qualified,
    /// SSA uses a parsed path form.
    Path,
    /// SSA uses a parsed subset-pointer form.
    SubsetPointer,
    /// SSA carries a parsed concatenated-key operand.
    ConcatenatedKey,
    /// GSAM record-search argument class, distinct from hierarchical SSA.
    RecordSearchArgument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Row-qualified call description checked against one complete generated applicability profile.
pub struct ImsCallSite<'a> {
    /// Exact catalog row identity disambiguating repeated call spellings.
    pub official_row: String,
    /// Caller spelling matched case-insensitively within the chosen syntax.
    pub name: &'a str,
    /// CALL or command spelling authority.
    pub syntax: ImsCallSyntax,
    /// Execution context being checked, without granting execution admission.
    pub context: ImsExecutionContext,
    /// Selected PCB kind; absent only where the profile permits no PCB.
    pub pcb_kind: Option<ImsPcbKind>,
    /// Exact organization label; absent where no database organization applies.
    pub organization: Option<&'a str>,
    /// A class assigned only after the PCB's raw PROCOPT has been validated.
    pub processing_option: Option<ImsProcessingOptionClass>,
    /// A class assigned only after the SSA parser has accepted the raw bytes.
    pub ssa_form: ImsSsaForm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One indivisible applicability profile; members of different profiles cannot be combined.
pub struct ImsCallVariant {
    /// Stable identity of this applicability profile.
    pub profile: &'static str,
    /// Contexts admitted by this profile.
    pub contexts: &'static [ImsExecutionContext],
    /// Required PCB kind, or None when the profile takes no PCB.
    pub pcb_kind: Option<ImsPcbKind>,
    /// Exact admitted organization labels; empty requires no organization operand.
    pub organizations: &'static [&'static str],
    /// Admitted validated classes; empty requires no class operand.
    pub processing_options: &'static [ImsProcessingOptionClass],
    /// Operand-form classes admitted by this profile.
    pub ssa_forms: &'static [ImsSsaForm],
    /// Optional spelling restriction; empty admits the family's matching spellings.
    pub names: &'static [&'static str],
    /// Pinned topic locators supporting this profile's applicability review.
    pub source_topics: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Generated row identity and validation profiles, without runtime or coverage authority.
pub struct ImsCallApplicabilityDescriptor {
    /// Catalog ordinal identifying the row within the generated family list.
    pub ordinal: u8,
    /// Exact immutable catalog row identity.
    pub official_row: &'static str,
    /// CALL spellings associated with this row.
    pub call_names: &'static [&'static str],
    /// Command spellings associated with this row.
    pub command_names: &'static [&'static str],
    /// Complete profiles that can validate a call site.
    pub variants: &'static [ImsCallVariant],
    /// Catalog and contract presence never earn execution coverage.
    pub executed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Reason a row-qualified site fails the generated applicability matrix.
pub enum ImsApplicabilityProblem {
    /// The exact official row has no registered family.
    UnknownFamily,
    /// The name is not a spelling for this row and syntax.
    WrongSpelling,
    /// No remaining profile admits the context.
    ForbiddenContext,
    /// No profile admits the supplied PCB kind.
    ForbiddenPcbKind,
    /// No remaining profile admits the organization.
    ForbiddenOrganization,
    /// No remaining profile admits the validated option class.
    ForbiddenProcessingOption,
    /// A required PCB or organization operand is absent.
    MissingPcbOrOrganization,
    /// No complete profile admits both operand form and spelling.
    ForbiddenSsaForm,
    /// The family does not take the supplied PCB or organization operand.
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
