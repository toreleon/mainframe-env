//! Stored finite profiles preserve the original wrapper identity. These are
//! codec inputs, never grants or a replacement canonical encoder.
use super::*;
use mainframe_env_host_api::MqMessageLimits;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Profiles {
    host: [usize; 6],
    message: [usize; 9],
    wait_ticks: u64,
    mqi: [usize; 4],
}
impl Profiles {
    pub(super) fn new(host: HostLimits, mqi: MqMqiLimits) -> Self {
        let m = mqi.message;
        Self {
            host: [
                host.max_name_bytes,
                host.max_record_bytes,
                host.max_records,
                host.max_fields,
                host.max_audit_fields,
                host.max_state_bytes,
            ],
            message: [
                m.body_bytes,
                m.identifier_bytes,
                m.format_bytes,
                m.properties,
                m.property_name_bytes,
                m.property_value_bytes,
                m.property_total_bytes,
                m.distribution_items,
                m.destination_bytes,
            ],
            wait_ticks: m.wait_ticks,
            mqi: [
                mqi.selectors,
                mqi.attribute_bytes,
                mqi.buffer_bytes,
                mqi.canonical_bytes,
            ],
        }
    }
    pub(super) fn values(&self) -> Result<(HostLimits, MqMqiLimits), HostProblem> {
        let h = self.host;
        let m = self.message;
        let q = self.mqi;
        let host = HostLimits {
            max_name_bytes: h[0],
            max_record_bytes: h[1],
            max_records: h[2],
            max_fields: h[3],
            max_audit_fields: h[4],
            max_state_bytes: h[5],
        };
        let mqi = MqMqiLimits {
            message: MqMessageLimits {
                body_bytes: m[0],
                identifier_bytes: m[1],
                format_bytes: m[2],
                properties: m[3],
                property_name_bytes: m[4],
                property_value_bytes: m[5],
                property_total_bytes: m[6],
                distribution_items: m[7],
                destination_bytes: m[8],
                wait_ticks: self.wait_ticks,
            },
            selectors: q[0],
            attribute_bytes: q[1],
            buffer_bytes: q[2],
            canonical_bytes: q[3],
        };
        let ceiling = Self::new(HostLimits::default(), MqMqiLimits::default());
        if self
            .host
            .iter()
            .zip(ceiling.host)
            .any(|(n, c)| *n == 0 || *n > c)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        mqi.validate().map_err(|_| HostProblem::Malformed)?;
        Ok((host, mqi))
    }
}
