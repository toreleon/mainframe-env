//! Captured MQCONN writeback. Registry authority remains with the provider.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Capture {
    arguments: Vec<connx::Storage>,
}

impl ReferenceMachine {
    pub(super) fn capture_connect(&self, parameters: &[String]) -> Result<Capture, MachineProblem> {
        if parameters.len() != 4 {
            return Err(MachineProblem::InvalidOperation);
        }
        let arguments = parameters
            .iter()
            .map(|name| self.connx_storage(name))
            .collect::<Result<Vec<_>, _>>()?;
        // These are the compiler's explicit fullword storage rules, not host
        // native encoding. A numeric alias is never an opaque handle token.
        if arguments[1..].iter().any(|s| s.layout.native_binary) {
            return Err(MachineProblem::UnsupportedForm);
        }
        Ok(Capture { arguments })
    }

    pub(super) fn recheck_connect(&self, capture: &Capture) -> Result<(), MachineProblem> {
        let state = self.mqi.as_ref().ok_or(MachineProblem::InvalidOperation)?;
        connx::contained(|| state.current(&self.invocation))?;
        for stored in &capture.arguments {
            if self.connx_storage(&stored.layout.name)? != *stored {
                return Err(MachineProblem::Host(HostProblem::IdempotencyConflict));
            }
        }
        Ok(())
    }

    pub(super) fn write_connect(
        &mut self,
        capture: &Capture,
        targets: &Targets,
        wire: Option<i32>,
        completion: i32,
        reason: i32,
    ) -> Result<(), MachineProblem> {
        let mut writes = Vec::with_capacity(3);
        for (target, value) in [
            (&targets.connection, wire),
            (&targets.completion, Some(completion)),
            (&targets.reason, Some(reason)),
        ] {
            let Some(value) = value else { continue };
            self.mq_long_target(target)?;
            let storage = self.connx_storage(target)?;
            let bytes = encode_decimal(
                &storage.layout,
                Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                },
            )?;
            if bytes.len() != storage.view.length {
                return Err(MachineProblem::UnsupportedForm);
            }
            writes.push((storage.view, bytes));
        }
        // Final callback/capture checks precede every write and alias insertion.
        // No allocation, callback, encoding or fallible lookup follows them.
        self.recheck_connect(capture)?;
        for (view, bytes) in writes {
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
        }
        Ok(())
    }
}
