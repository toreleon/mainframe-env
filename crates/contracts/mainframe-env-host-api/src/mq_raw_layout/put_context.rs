//! Defined context-only PUT output; no new generic layout or execution authority.
//! MQ9.4 supplements q098655_194–227, q097395_1498–1508/1571–1955.
use super::*;
use crate::mq_md_value::MqMdValue;

pub(super) fn is_context_field(name: &str) -> bool {
    matches!(
        name,
        "UserIdentifier"
            | "AccountingToken"
            | "ApplIdentityData"
            | "PutApplType"
            | "PutApplName"
            | "PutDate"
            | "PutTime"
            | "ApplOriginData"
    )
}

impl MqRawCapture {
    /// Atomically copy the eight actual returned default/no-context fields for
    /// one z/OS single-queue PUT/PUT1 with an otherwise unchanged MQMD1/2.
    /// This bounded writer requires all other observed fields, including ignored
    /// BackoutCount and supplied IDs, to equal the exact captured input. It does
    /// not support generated IDs, queue-default rewrites, pass/set context or
    /// distribution lists. It neither generates context nor attests its origin.
    /// Generic writeback policy and generated layout identities stay unchanged.
    /// Caller suffix bytes remain unowned; every failure leaves bytes untouched.
    pub fn writeback_put_context_md(
        &self,
        context: MqRawWritebackContext,
        descriptor: &MqMdValue,
        destination: &mut [u8],
    ) -> Result<(), MqRawProblem> {
        if !matches!(context.call, MqMqiCall::Put | MqMqiCall::PutOne)
            || context.platform != MqRawPlatform::Zos
            || !context.single_queue
            || context.dynamic_model_open
        {
            return Err(MqRawProblem::OutputPending);
        }
        let input = self.to_full_md_value()?;
        if descriptor.version() != input.version() {
            return Err(MqRawProblem::Version);
        }
        if descriptor.characters() != input.characters() {
            return Err(MqRawProblem::UnsupportedEncoding);
        }
        validate_md_identity(descriptor).map_err(|_| MqRawProblem::StructureIdentifier)?;
        let original = input.fields();
        let mut unowned = descriptor.clone();
        let fields = match &mut unowned {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
        };
        fields.user_identifier = original.user_identifier;
        fields.accounting_token = original.accounting_token;
        fields.appl_identity_data = original.appl_identity_data;
        fields.put_appl_type = original.put_appl_type;
        fields.put_appl_name = original.put_appl_name;
        fields.put_date = original.put_date;
        fields.put_time = original.put_time;
        fields.appl_origin_data = original.appl_origin_data;
        if unowned != input {
            return Err(MqRawProblem::OutputPending);
        }
        let fields = descriptor.fields();
        let observations = [
            observed(
                "UserIdentifier",
                MqRawFieldValue::Characters(&fields.user_identifier),
            ),
            observed(
                "AccountingToken",
                MqRawFieldValue::Bytes(&fields.accounting_token),
            ),
            observed(
                "ApplIdentityData",
                MqRawFieldValue::Characters(&fields.appl_identity_data),
            ),
            observed("PutApplType", MqRawFieldValue::Long(fields.put_appl_type)),
            observed(
                "PutApplName",
                MqRawFieldValue::Characters(&fields.put_appl_name),
            ),
            observed("PutDate", MqRawFieldValue::Characters(&fields.put_date)),
            observed("PutTime", MqRawFieldValue::Characters(&fields.put_time)),
            observed(
                "ApplOriginData",
                MqRawFieldValue::Characters(&fields.appl_origin_data),
            ),
        ];
        self.writeback_inner(context, &observations, destination, true)
    }
}

fn observed<'a>(field: &'static str, value: MqRawFieldValue<'a>) -> MqRawObservedField<'a> {
    MqRawObservedField {
        field,
        observation: MqRawObservation::Observed(value),
    }
}

#[cfg(test)]
mod tests;
