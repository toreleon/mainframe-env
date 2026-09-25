use mainframe_env_host_api::HostProblem;

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8], magic: &[u8]) -> Result<Self, HostProblem> {
        if !bytes.starts_with(magic) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Self {
            bytes,
            at: magic.len(),
        })
    }

    pub(super) fn take(&mut self, amount: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(amount)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    pub(super) fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn bytes(&mut self, maximum: usize) -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if length > maximum {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(length)?.to_vec())
    }

    pub(super) fn text(&mut self, maximum: usize) -> Result<String, HostProblem> {
        String::from_utf8(self.bytes(maximum)?).map_err(|_| HostProblem::InfrastructureFailure)
    }

    pub(super) fn count(&mut self, maximum: usize) -> Result<usize, HostProblem> {
        let count = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if count > maximum {
            Err(HostProblem::ResourceExhausted)
        } else {
            Ok(count)
        }
    }

    pub(super) fn usize(&mut self) -> Result<usize, HostProblem> {
        usize::try_from(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)
    }

    pub(super) fn finish(self) -> Result<(), HostProblem> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        }
    }
}
