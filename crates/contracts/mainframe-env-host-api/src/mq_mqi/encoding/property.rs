use super::*;

object!(MqPropertyOptions {
    call,
    options,
    version
});
object!(MqPropertyDescriptor {
    context,
    copy_options,
    options,
    struc_id,
    support,
    version
});
object!(MqPropertyData {
    bytes,
    ccsid,
    encoding,
    kind
});
object!(MqPropertyInquiryObservation {
    copied_value,
    data_length,
    descriptor,
    kind,
    name_ccsid,
    name_length,
    returned_ccsid,
    returned_encoding,
    returned_name
});
variants!(MqPropertyRequest {
    Create { connection, options },
    Set { connection, descriptor, handle, name, options, value },
    Inquire { connection, handle, name, name_capacity, options, requested_type, value_capacity },
    Delete { connection, handle, name, options },
    DeleteHandle { connection, handle, options }
});
impl Canonical for MqPropertyName {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("MqPropertyName", 1)?;
        out.text("0")?;
        self.0.encode(out)
    }
}
impl Canonical for MqPropertyObservation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Set(v) => {
                out.variant("MqPropertyObservation", "Set", 1)?;
                out.text("0")?;
                v.encode(out)
            }
            Self::Inquired(v) => {
                out.variant("MqPropertyObservation", "Inquired", 1)?;
                out.text("0")?;
                v.encode(out)
            }
            Self::PropertyDeleted => out.variant("MqPropertyObservation", "PropertyDeleted", 0),
            Self::HandleDeleted => out.variant("MqPropertyObservation", "HandleDeleted", 0),
            Self::Absent => out.variant("MqPropertyObservation", "Absent", 0),
        }
    }
}
