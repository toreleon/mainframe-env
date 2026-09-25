//! Validated MCEP profile for task-channel container commands.

use super::{
    CicsEffectPlan, CicsOperandName as I, CicsOutputName as O, CicsPlanOperation as P,
    CicsPlanOption as F,
};
use std::collections::BTreeSet;

pub(super) const fn is_channel_container(operation: P) -> bool {
    matches!(
        operation,
        P::DeleteChannel
            | P::DeleteContainer
            | P::GetContainer
            | P::MoveContainer
            | P::PutContainer
            | P::QueryChannel
    )
}

pub(super) const fn allowed_output(operation: P, output: O) -> bool {
    if matches!(output, O::Resp | O::Resp2) {
        return true;
    }
    match operation {
        P::GetContainer => matches!(
            output,
            O::ContainerInto | O::ContainerSet | O::ContainerLength | O::ContainerCcsid
        ),
        P::QueryChannel => matches!(output, O::ContainerCount),
        _ => false,
    }
}

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<I>,
    outputs: &BTreeSet<O>,
) -> bool {
    let (required, optional): (&[I], &[I]) = match plan.operation {
        P::DeleteChannel | P::QueryChannel => (&[I::BtsChannel], &[]),
        P::DeleteContainer => (&[I::ContainerName], &[I::BtsChannel]),
        P::GetContainer => (
            &[I::ContainerName],
            &[
                I::BtsChannel,
                I::ContainerLength,
                I::ContainerByteOffset,
                I::ContainerIntoCcsid,
                I::ContainerIntoCodepage,
                I::ContainerConvertst,
            ],
        ),
        P::MoveContainer => (
            &[I::ContainerName, I::ContainerAs],
            &[I::BtsChannel, I::ContainerToChannel],
        ),
        P::PutContainer => (
            &[I::ContainerName, I::ContainerFrom],
            &[
                I::BtsChannel,
                I::ContainerLength,
                I::ContainerDatatype,
                I::ContainerCcsid,
                I::ContainerFromCodepage,
            ],
        ),
        _ => return true,
    };
    if required.iter().any(|name| !inputs.contains(name))
        || inputs
            .iter()
            .any(|name| !required.contains(name) && !optional.contains(name))
        || outputs
            .iter()
            .any(|name| !allowed_output(plan.operation, *name))
    {
        return true;
    }
    if plan.options.iter().any(|option| {
        !matches!(option, F::NoHandle)
            && !matches!(
                (plan.operation, option),
                (P::PutContainer, F::ContainerAppend) | (P::GetContainer, F::ContainerNoData)
            )
    }) {
        return true;
    }
    match plan.operation {
        P::GetContainer => {
            let destinations = usize::from(outputs.contains(&O::ContainerInto))
                + usize::from(outputs.contains(&O::ContainerSet))
                + usize::from(plan.options.contains(&F::ContainerNoData));
            destinations != 1
                || (outputs.contains(&O::ContainerSet)
                    || plan.options.contains(&F::ContainerNoData))
                    && !outputs.contains(&O::ContainerLength)
                || (outputs.contains(&O::ContainerInto)
                    && outputs.contains(&O::ContainerLength)
                        != inputs.contains(&I::ContainerLength))
                || (!outputs.contains(&O::ContainerInto) && inputs.contains(&I::ContainerLength))
        }
        P::QueryChannel => !outputs.contains(&O::ContainerCount),
        _ => false,
    }
}
