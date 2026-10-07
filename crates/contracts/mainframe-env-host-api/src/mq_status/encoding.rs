use super::*;
use crate::HostProblem;
use crate::canonical::{Canonical, Encoder};

impl Canonical for MqReviewedStatus {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { call, pair } = self;
        out.object("MqReviewedStatus", 6)?;
        out.text("call")?;
        call.encode(out)?;
        out.text("catalog_sha256")?;
        out.text(MQ_STATUS_CATALOG_SHA256)?;
        out.text("completion")?;
        out.text(pair.completion.symbol())?;
        out.text("reason_decimal")?;
        self.reason_decimal().encode(out)?;
        out.text("reason_hex")?;
        out.text(self.reason_hex())?;
        out.text("reason_symbol")?;
        out.text(pair.reason_symbol)
    }
}
