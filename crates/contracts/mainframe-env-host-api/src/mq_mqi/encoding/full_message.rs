//! Additive tags using the one streaming authority and the owned full MD encoder.
use super::*;
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
