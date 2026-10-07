use super::*;

variants!(MqRfh2Profile {
    ZosBatchUtf8NativeV1
});
object!(MqRfh2Options {
    call,
    options,
    version
});
variants!(MqMqiRfh2Request {
    BufferToHandle { buffer, connection, descriptor, handle, options, profile },
    HandleToBuffer { buffer_capacity, connection, descriptor, handle, name, options, profile }
});
object!(MqRfh2Observation {
    buffer,
    data_length,
    descriptor
});
impl Canonical for MqRfh2BufferObservation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Unchanged => out.variant("MqRfh2BufferObservation", "Unchanged", 0),
            Self::WrittenPrefix(bytes) => {
                out.variant("MqRfh2BufferObservation", "WrittenPrefix", 1)?;
                out.text("0")?;
                bytes.encode(out)
            }
        }
    }
}
