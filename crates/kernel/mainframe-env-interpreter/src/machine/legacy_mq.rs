use super::*;

impl ReferenceMachine {
    pub(super) fn mq_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let operation = match normalize(
            args.first()
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']),
        )
        .as_str()
        {
            "MQOPEN" => MqOperation::Open,
            "MQGET" => MqOperation::Get,
            "MQPUT" => MqOperation::Put,
            "MQPUT1" => MqOperation::PutOne,
            "MQCLOSE" => MqOperation::Close,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let using = position(args, "USING").ok_or(MachineProblem::InvalidOperation)?;
        let parameters = args[using + 1..]
            .iter()
            .filter(|argument| {
                !matches!(
                    argument.as_str(),
                    "BY" | "REFERENCE" | "CONTENT" | "VALUE" | "END-CALL"
                )
            })
            .map(|argument| normalize(argument))
            .collect::<Vec<_>>();
        let parameter = |index: usize| {
            parameters
                .get(index)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)
        };
        let read_i32 = |machine: &Self, target: &str| -> Result<i32, MachineProblem> {
            let value = machine.decimal(target)?;
            if value.scale != 0 {
                return Err(MachineProblem::DataException);
            }
            i32::try_from(value.coefficient).map_err(|_| MachineProblem::DataException)
        };
        let read_handle = |machine: &Self, target: &str| -> Result<u32, MachineProblem> {
            let value = read_i32(machine, target)?;
            u32::try_from(value).map_err(|_| MachineProblem::DataException)
        };

        let mut request = MqRequest {
            operation,
            queue: None,
            handle: None,
            options: 0,
            message: Vec::new(),
            message_id: None,
            correlation_id: None,
            wait_ticks: 0,
            max_message_bytes: 1,
            mutation: Some(self.mutation()?),
        };
        let mut descriptor = None;
        let mut handle_target = None;
        let mut buffer = None;
        let mut data_length = None;
        let (completion_code, reason_code) = match operation {
            MqOperation::Open => {
                let object_descriptor = parameter(1)?;
                request.queue = Some(self.mq_queue_name(&object_descriptor)?);
                request.options = read_i32(self, &parameter(2)?)?;
                handle_target = Some(parameter(3)?);
                (Some(parameter(4)?), Some(parameter(5)?))
            }
            MqOperation::Get => {
                let target = parameter(1)?;
                request.handle = Some(read_handle(self, &target)?);
                let message_descriptor = parameter(2)?;
                let get_options = parameter(3)?;
                request.options = self
                    .mq_descriptor_decimal(&get_options, "MQGMO-OPTIONS")
                    .unwrap_or_else(|| read_i32(self, &get_options))?;
                request.wait_ticks = self
                    .mq_descriptor_decimal(&get_options, "MQGMO-WAITINTERVAL")
                    .transpose()?
                    .unwrap_or_default()
                    .max(0) as u64;
                let maximum = read_i32(self, &parameter(4)?)?.max(0);
                request.max_message_bytes =
                    u32::try_from(maximum).map_err(|_| MachineProblem::ResourceExhausted)?;
                request.message_id = self.mq_descriptor_bytes(&message_descriptor, "MQMD-MSGID")?;
                request.correlation_id =
                    self.mq_descriptor_bytes(&message_descriptor, "MQMD-CORRELID")?;
                descriptor = Some(message_descriptor);
                buffer = Some(parameter(5)?);
                data_length = Some(parameter(6)?);
                (Some(parameter(7)?), Some(parameter(8)?))
            }
            MqOperation::Put => {
                let target = parameter(1)?;
                request.handle = Some(read_handle(self, &target)?);
                let message_descriptor = parameter(2)?;
                let put_options = parameter(3)?;
                request.options = self
                    .mq_descriptor_decimal(&put_options, "MQPMO-OPTIONS")
                    .unwrap_or_else(|| read_i32(self, &put_options))?;
                let length = read_i32(self, &parameter(4)?)?.max(0) as usize;
                let target = parameter(5)?;
                let mut message = self.read(&target)?;
                message.truncate(length);
                request.max_message_bytes =
                    u32::try_from(length).map_err(|_| MachineProblem::ResourceExhausted)?;
                request.message = message;
                request.message_id = self.mq_descriptor_bytes(&message_descriptor, "MQMD-MSGID")?;
                request.correlation_id =
                    self.mq_descriptor_bytes(&message_descriptor, "MQMD-CORRELID")?;
                descriptor = Some(message_descriptor);
                (Some(parameter(6)?), Some(parameter(7)?))
            }
            MqOperation::PutOne => {
                let object_descriptor = parameter(1)?;
                request.queue = Some(self.mq_queue_name(&object_descriptor)?);
                let message_descriptor = parameter(2)?;
                let put_options = parameter(3)?;
                request.options = self
                    .mq_descriptor_decimal(&put_options, "MQPMO-OPTIONS")
                    .unwrap_or_else(|| read_i32(self, &put_options))?;
                let length = read_i32(self, &parameter(4)?)?.max(0) as usize;
                let target = parameter(5)?;
                let mut message = self.read(&target)?;
                message.truncate(length);
                request.max_message_bytes =
                    u32::try_from(length).map_err(|_| MachineProblem::ResourceExhausted)?;
                request.message = message;
                request.message_id = self.mq_descriptor_bytes(&message_descriptor, "MQMD-MSGID")?;
                request.correlation_id =
                    self.mq_descriptor_bytes(&message_descriptor, "MQMD-CORRELID")?;
                descriptor = Some(message_descriptor);
                (Some(parameter(6)?), Some(parameter(7)?))
            }
            MqOperation::Close => {
                let target = parameter(1)?;
                request.handle = Some(read_handle(self, &target)?);
                request.options = read_i32(self, &parameter(2)?)?;
                handle_target = Some(target);
                (Some(parameter(3)?), Some(parameter(4)?))
            }
            MqOperation::Commit | MqOperation::Rollback => {
                return Err(MachineProblem::UnsupportedForm);
            }
        };
        self.effect(
            HostRequest::Mq(request),
            PendingKind::Mq {
                handle: handle_target,
                descriptor,
                buffer,
                data_length,
                completion_code,
                reason_code,
            },
        )
    }
}
