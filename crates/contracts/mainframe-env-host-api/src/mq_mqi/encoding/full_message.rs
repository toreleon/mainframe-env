//! Additive tags using the one streaming authority and the owned full MD encoder.
use super::*;
variants!(MqMqiDestinationCount { UndefinedZos });
variants!(MqMqiIgnoredCounter {
    PreservedIgnoredInput
});
object!(MqMqiProduced {
    backout_count,
    descriptor,
    invalid_dest_count,
    known_dest_count,
    outcome,
    resolved_manager,
    resolved_queue,
    unknown_dest_count
});
object!(MqFullMessage {
    body,
    descriptor,
    properties
});
object!(MqMqiFullPut {
    context,
    message,
    message_handle,
    options,
    unit
});
object!(MqMqiFullGet {
    buffer_capacity,
    connection,
    descriptor,
    message_handle,
    mode,
    object,
    options,
    truncation,
    unit,
    wait
});
