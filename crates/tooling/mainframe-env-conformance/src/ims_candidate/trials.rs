//! Finite equivalence-class recipes, not a workflow language or semantic engine.
use super::*;
use mainframe_env_host_api::ImsOperation as Op;

pub(super) fn execute(
    fixture: &Fixture,
    metadata: &ImsMetadataCatalog,
    seed: &ImsGenericLoadImage,
) -> Result<Outcome, String> {
    let mut route = route::Route::open(fixture, metadata, seed)?;
    let mut calls = Vec::new();
    let get = |route: &route::Route, op, sequence, key| {
        route.call(op, sequence, &["ROOT"], &[], key, &[])
    };
    match fixture.trial {
        Trial::UniqueRepeated => {
            calls.push(get(&route, Op::GetUnique, 10, Some(&b"A1"[..]))?);
            calls.push(get(&route, Op::GetUnique, 11, Some(&b"A1"[..]))?);
        }
        Trial::SequentialBoundary => {
            // The declared roots-only seed avoids unresolved GA/GK transitions.
            for sequence in 10..14 {
                calls.push(route.call(Op::GetNext, sequence, &[], &[], None, &[])?);
            }
        }
        Trial::ParentSequence => {
            calls.push(get(&route, Op::GetUnique, 10, Some(&b"A1"[..]))?);
            for sequence in 11..13 {
                calls.push(route.call(Op::GetNextParent, sequence, &["CHILD"], &[], None, &[])?);
            }
        }
        Trial::ParentRequired => {
            calls.push(route.call(Op::GetNextParent, 10, &["CHILD"], &[], None, &[])?);
        }
        Trial::ParentMismatch => {
            calls.push(get(&route, Op::GetUnique, 10, Some(&b"A1"[..]))?);
            calls.push(route.call(
                Op::GetNextParent,
                11,
                &[],
                &[],
                None,
                &[b"ROOT    (ROOTKEY = B2)", b"CHILD    "],
            )?);
        }
        Trial::HoldUniqueReplace
        | Trial::HoldNextReplace
        | Trial::HoldParentReplace
        | Trial::ReplaceWithoutHold
        | Trial::ReplaceKeyForbidden
        | Trial::ReplaceRollback
        | Trial::ReplaceRestartReplay
        | Trial::InterveningGet => {
            let hold = !matches!(fixture.trial, Trial::ReplaceWithoutHold);
            let mut segment = "ROOT";
            let mut replacement = &b"A1z"[..];
            if matches!(fixture.trial, Trial::HoldParentReplace) {
                calls.push(get(&route, Op::GetUnique, 9, Some(&b"A1"[..]))?);
                calls.push(route.call(Op::GetHoldNextParent, 10, &["CHILD"], &[], None, &[])?);
                segment = "CHILD";
                replacement = b"c1z";
            } else if matches!(fixture.trial, Trial::HoldNextReplace) {
                calls.push(route.call(Op::GetHoldNext, 10, &[], &[], None, &[])?);
            } else {
                calls.push(get(
                    &route,
                    if hold {
                        Op::GetHoldUnique
                    } else {
                        Op::GetUnique
                    },
                    10,
                    Some(&b"A1"[..]),
                )?);
            }
            if matches!(fixture.trial, Trial::InterveningGet) {
                calls.push(route.call(Op::GetNext, 11, &["CHILD"], &[], None, &[])?);
                segment = "CHILD";
                replacement = b"c1z";
            }
            if matches!(fixture.trial, Trial::ReplaceKeyForbidden) {
                replacement = b"Z9z";
            }
            calls.push(route.call(Op::Replace, 12, &[segment], replacement, None, &[])?);
            if matches!(fixture.trial, Trial::ReplaceRollback) {
                calls.push(route.call(Op::Rollback, 13, &[], &[], None, &[])?);
            }
            if matches!(fixture.trial, Trial::ReplaceRestartReplay) {
                route.reopen()?;
                calls.push(route.call(Op::Replace, 12, &[segment], replacement, None, &[])?);
            }
        }
        Trial::DeleteSubtree | Trial::DeleteWithoutHold | Trial::DeleteRestartReplay => {
            calls.push(get(
                &route,
                if matches!(fixture.trial, Trial::DeleteWithoutHold) {
                    Op::GetUnique
                } else {
                    Op::GetHoldUnique
                },
                10,
                Some(&b"A1"[..]),
            )?);
            calls.push(route.call(Op::Delete, 11, &["ROOT"], &[], None, &[])?);
            if matches!(fixture.trial, Trial::DeleteRestartReplay) {
                route.reopen()?;
                calls.push(route.call(Op::Delete, 11, &["ROOT"], &[], None, &[])?);
            }
        }
        Trial::MalformedSsa => {
            calls.push(route.call(Op::GetUnique, 10, &[], &[], None, &[b"ROOT("])?);
        }
        Trial::SsaLimit => {
            calls.push(route.call(Op::GetUnique, 10, &[], &[], None, &[&b"ROOT    "[..]; 16])?);
        }
        Trial::Denied => {
            route.deny();
            calls.push(get(&route, Op::GetHoldUnique, 10, Some(&b"A1"[..]))?);
        }
        Trial::ReplayedNext => {
            calls.push(route.call(Op::GetNext, 10, &[], &[], None, &[])?);
            calls.push(route.call(Op::GetNext, 10, &[], &[], None, &[])?);
        }
    }
    Ok(Outcome {
        calls,
        state: route.state()?,
    })
}
