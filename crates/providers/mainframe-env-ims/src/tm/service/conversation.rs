use super::super::model::ConversationStart;
use super::*;

impl TmService {
    pub(super) fn resolve_conversation(
        &self,
        invocation: &Invocation,
        transaction: &TmTransactionDefinition,
        supplied: Option<&str>,
        request_digest: [u8; 32],
        package_binding: Option<&str>,
    ) -> Result<(Option<String>, bool), HostProblem> {
        if !transaction.conversational {
            return if supplied.is_none() {
                Ok((None, false))
            } else {
                Err(HostProblem::Unsupported)
            };
        }
        if let Some(id) = supplied {
            let (_, conversation) = read::<ConversationRow>(
                self.store.as_ref(),
                CONVERSATION_NAMESPACE,
                id,
                self.limits.max_state_bytes,
            )?
            .ok_or(HostProblem::NotFound)?;
            if conversation.principal != invocation.principal.id().as_str()
                || conversation.next_transaction != transaction.code
                || conversation.package_binding.as_deref() != package_binding
            {
                return Err(HostProblem::Unauthorized);
            }
            Ok((Some(id.into()), false))
        } else {
            Ok((
                Some(format!("conv-{}", &hex_digest(&request_digest)[..24])),
                true,
            ))
        }
    }

    pub(super) fn start_conversation(
        &self,
        invocation: &Invocation,
        transaction: &TmTransactionDefinition,
        message: &MessageRow,
    ) -> Result<Option<ConversationStart>, HostProblem> {
        let Some(id) = message.message.conversation_id.as_deref() else {
            return Ok(None);
        };
        let existing = read::<ConversationRow>(
            self.store.as_ref(),
            CONVERSATION_NAMESPACE,
            id,
            self.limits.max_state_bytes,
        )?;
        match (message.new_conversation, existing) {
            (true, None) => Ok(Some(ConversationStart {
                current_version: None,
                row: ConversationRow {
                    conversation_id: id.into(),
                    principal: invocation.principal.id().as_str().into(),
                    next_transaction: transaction.code.clone(),
                    spa: Vec::new(),
                    step: 0,
                    package_binding: message.package_binding.clone(),
                },
                spa: Vec::new(),
            })),
            (false, Some((version, conversation)))
                if conversation.principal == invocation.principal.id().as_str()
                    && conversation.next_transaction == transaction.code
                    && conversation.package_binding == message.package_binding =>
            {
                let spa = conversation.spa.clone();
                Ok(Some(ConversationStart {
                    current_version: Some(version),
                    row: conversation,
                    spa,
                }))
            }
            _ => Err(HostProblem::IdempotencyConflict),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn conversation_commit(
        &self,
        invocation: &Invocation,
        catalog: &CatalogRow,
        transaction: &TmTransactionDefinition,
        session: &SessionRow,
        action: Option<TmConversationAction>,
        mutations: &mut Vec<ProviderStateMutation>,
    ) -> Result<(), HostProblem> {
        let Some(id) = session.conversation_id.as_deref() else {
            return if action.is_none() {
                Ok(())
            } else {
                Err(HostProblem::Unsupported)
            };
        };
        let (version, mut row) = read::<ConversationRow>(
            self.store.as_ref(),
            CONVERSATION_NAMESPACE,
            id,
            self.limits.max_state_bytes,
        )?
        .ok_or(HostProblem::InfrastructureFailure)?;
        let action = action.ok_or(HostProblem::Malformed)?;
        match action {
            TmConversationAction::Continue { spa } => {
                row.spa = spa;
                row.next_transaction = transaction.code.clone();
                row.step = row
                    .step
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                mutations.push(put(
                    CONVERSATION_NAMESPACE,
                    id,
                    &row,
                    Some(version),
                    self.limits.max_state_bytes,
                )?);
            }
            TmConversationAction::Switch {
                transaction: next,
                spa,
            } => {
                let target = find_transaction(catalog, &next)?;
                if !target.conversational {
                    return Err(HostProblem::Unsupported);
                }
                self.authorize_transaction(invocation, target, AccessIntent::Execute)?;
                row.spa = spa;
                row.next_transaction = next;
                row.step = row
                    .step
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                mutations.push(put(
                    CONVERSATION_NAMESPACE,
                    id,
                    &row,
                    Some(version),
                    self.limits.max_state_bytes,
                )?);
            }
            TmConversationAction::End => {
                mutations.push(delete(CONVERSATION_NAMESPACE, id, version));
            }
        }
        Ok(())
    }
}
