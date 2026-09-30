//! Validated MCEP profile for task-channel and BTS container commands.

use super::{
    CicsEffectPlan, CicsOperandName as I, CicsOperandValue, CicsOutputName as O,
    CicsPlanOperation as P, CicsPlanOption as F,
};
use std::collections::BTreeSet;

pub(super) const fn is_channel_container(operation: P) -> bool {
    matches!(
        operation,
        P::DeleteChannel
            | P::DeleteContainer
            | P::GetContainer
            | P::GetContainer64
            | P::MoveContainer
            | P::PutContainer
            | P::PutContainer64
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
        P::GetContainer64 => matches!(
            output,
            O::ContainerInto64 | O::ContainerLength | O::ContainerCcsid
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
        P::DeleteContainer => (&[I::ContainerName], &[I::BtsChannel, I::ContainerActivity]),
        P::GetContainer => (
            &[I::ContainerName],
            &[
                I::BtsChannel,
                I::ContainerActivity,
                I::ContainerLength,
                I::ContainerByteOffset,
                I::ContainerIntoCcsid,
                I::ContainerIntoCodepage,
                I::ContainerConvertst,
            ],
        ),
        P::GetContainer64 => (
            &[I::ContainerName, I::Abi64],
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
            &[
                I::BtsChannel,
                I::ContainerToChannel,
                I::ContainerFromActivity,
                I::ContainerToActivity,
            ],
        ),
        P::PutContainer => (
            &[I::ContainerName, I::ContainerFrom],
            &[
                I::BtsChannel,
                I::ContainerActivity,
                I::ContainerLength,
                I::ContainerDatatype,
                I::ContainerCcsid,
                I::ContainerFromCodepage,
            ],
        ),
        P::PutContainer64 => (
            &[I::ContainerName, I::ContainerFrom64, I::Abi64],
            &[
                I::BtsChannel,
                I::ContainerLength,
                I::ContainerDatatype,
                I::ContainerCcsid,
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
    if matches!(plan.operation, P::GetContainer64 | P::PutContainer64)
        && (plan.operands.iter().any(|operand| {
            operand.name == I::Abi64
                && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"mainframe-env.cics-amode64-nonle@1")
        })
            || plan.operation == P::PutContainer64
                && plan.operands.iter().any(|operand| {
                    operand.name == I::ContainerFrom64
                        && !matches!(operand.value, CicsOperandValue::Storage(_))
                }))
    {
        return true;
    }
    if plan.options.iter().any(|option| {
        !matches!(option, F::NoHandle)
            && !matches!(
                (plan.operation, option),
                (P::PutContainer, F::ContainerAppend)
                    | (P::PutContainer64, F::ContainerAppend)
                    | (P::GetContainer64, F::ContainerNoData)
                    | (P::GetContainer, F::ContainerNoData)
                    | (
                        P::DeleteContainer | P::GetContainer | P::PutContainer,
                        F::ContainerProcess | F::ContainerAcqProcess | F::ContainerAcqActivity
                    )
                    | (
                        P::MoveContainer,
                        F::ContainerFromProcess | F::ContainerToProcess
                    )
            )
    }) {
        return true;
    }
    let bts = inputs.contains(&I::ContainerActivity)
        || inputs.contains(&I::ContainerFromActivity)
        || inputs.contains(&I::ContainerToActivity)
        || plan.options.iter().any(|option| {
            matches!(
                option,
                F::ContainerProcess
                    | F::ContainerAcqProcess
                    | F::ContainerAcqActivity
                    | F::ContainerFromProcess
                    | F::ContainerToProcess
            )
        });
    let selectors = usize::from(inputs.contains(&I::ContainerActivity))
        + plan
            .options
            .iter()
            .filter(|option| {
                matches!(
                    option,
                    F::ContainerProcess | F::ContainerAcqProcess | F::ContainerAcqActivity
                )
            })
            .count();
    let from = usize::from(inputs.contains(&I::ContainerFromActivity))
        + usize::from(plan.options.contains(&F::ContainerFromProcess));
    let to = usize::from(inputs.contains(&I::ContainerToActivity))
        + usize::from(plan.options.contains(&F::ContainerToProcess));
    if bts
        && (inputs.contains(&I::BtsChannel)
            || inputs.contains(&I::ContainerToChannel)
            || inputs.contains(&I::ContainerDatatype)
            || inputs.contains(&I::ContainerCcsid)
            || inputs.contains(&I::ContainerByteOffset)
            || inputs.contains(&I::ContainerIntoCcsid)
            || inputs.contains(&I::ContainerIntoCodepage)
            || inputs.contains(&I::ContainerFromCodepage)
            || inputs.contains(&I::ContainerConvertst)
            || outputs.contains(&O::ContainerCcsid))
        || selectors > 1
        || from > 1
        || to > 1
    {
        return true;
    }
    match plan.operation {
        P::GetContainer64 => {
            let nodata = plan.options.contains(&F::ContainerNoData);
            (outputs.contains(&O::ContainerInto64) == nodata)
                || (nodata && !outputs.contains(&O::ContainerLength))
                || (nodata && inputs.contains(&I::ContainerByteOffset))
                || (!nodata
                    && outputs.contains(&O::ContainerLength) != inputs.contains(&I::ContainerLength))
                || (nodata && inputs.contains(&I::ContainerLength))
                || (outputs.contains(&O::ContainerCcsid)
                    && !inputs.contains(&I::ContainerConvertst))
                || inputs.contains(&I::ContainerIntoCcsid)
                    && inputs.contains(&I::ContainerIntoCodepage)
                || inputs.contains(&I::ContainerIntoCcsid)
                    && plan.operands.iter().any(|operand| {
                        operand.name == I::ContainerIntoCcsid
                            && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"37")
                            && !matches!(operand.value, CicsOperandValue::Integer(37))
                    })
                || plan.operands.iter().any(|operand| {
                    operand.name == I::ContainerConvertst
                        && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"NOCONVERT")
                        || operand.name == I::ContainerIntoCodepage
                            && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"37")
                        || operand.name == I::ContainerDatatype
                            && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"BIT" || value == b"CHAR")
                })
        }
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
        P::PutContainer64 => {
            let bit = plan.operands.iter().any(|operand| {
                operand.name == I::ContainerDatatype
                    && matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"BIT")
            });
            (bit && inputs.contains(&I::ContainerCcsid))
                || plan.operands.iter().any(|operand| {
                    (operand.name == I::ContainerDatatype
                        && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"BIT" || value == b"CHAR"))
                        || (operand.name == I::ContainerCcsid
                            && !matches!(&operand.value, CicsOperandValue::Literal(value) if value == b"37")
                            && !matches!(operand.value, CicsOperandValue::Integer(37)))
                })
        }
        P::QueryChannel => !outputs.contains(&O::ContainerCount),
        _ => false,
    }
}
