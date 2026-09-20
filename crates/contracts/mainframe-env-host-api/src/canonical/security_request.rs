use super::*;

pub(super) fn encode_principal_validation(
    out: &mut Encoder<'_>,
    principal: &PrincipalId,
) -> Result<(), HostProblem> {
    out.variant("SecurityRequest", "ValidatePrincipal", 1)?;
    out.text("principal")?;
    principal.encode(out)
}
