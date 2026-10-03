//! Legacy MQ canonical bytes, mechanically retained.

use super::*;

impl Canonical for MqOperation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Open => out.variant("MqOperation", "Open", 0),
            Self::Get => out.variant("MqOperation", "Get", 0),
            Self::Put => out.variant("MqOperation", "Put", 0),
            Self::PutOne => out.variant("MqOperation", "PutOne", 0),
            Self::Close => out.variant("MqOperation", "Close", 0),
            Self::Commit => out.variant("MqOperation", "Commit", 0),
            Self::Rollback => out.variant("MqOperation", "Rollback", 0),
        }
    }
}

impl Canonical for MqRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            correlation_id,
            handle,
            max_message_bytes,
            message,
            message_id,
            mutation,
            operation,
            options,
            queue,
            wait_ticks,
        } = self;
        out.object("MqRequest", 10)?;
        out.text("correlation_id")?;
        correlation_id.encode(out)?;
        out.text("handle")?;
        handle.encode(out)?;
        out.text("max_message_bytes")?;
        max_message_bytes.encode(out)?;
        out.text("message")?;
        message.encode(out)?;
        out.text("message_id")?;
        message_id.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("operation")?;
        operation.encode(out)?;
        out.text("options")?;
        options.encode(out)?;
        out.text("queue")?;
        queue.encode(out)?;
        out.text("wait_ticks")?;
        wait_ticks.encode(out)?;
        Ok(())
    }
}

impl Canonical for MqResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            completion_code,
            correlation_id,
            handle,
            message,
            message_id,
            reason_code,
            trigger_program,
        } = self;
        out.object("MqResult", 7)?;
        out.text("completion_code")?;
        completion_code.encode(out)?;
        out.text("correlation_id")?;
        correlation_id.encode(out)?;
        out.text("handle")?;
        handle.encode(out)?;
        out.text("message")?;
        message.encode(out)?;
        out.text("message_id")?;
        message_id.encode(out)?;
        out.text("reason_code")?;
        reason_code.encode(out)?;
        out.text("trigger_program")?;
        trigger_program.encode(out)?;
        Ok(())
    }
}
