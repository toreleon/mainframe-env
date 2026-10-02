//! IMS facades share the product publication and retained-package authorities.
use super::*;

impl ProductServer {
    /// Owned feedback shares signed package selection and metadata publication fences.
    pub fn ims_pcb_feedback_selected_v1(
        &self,
        application: &str,
        invocation: &Invocation,
        request: &mainframe_env_host_api::ImsPcbFeedbackRequestV1,
    ) -> Result<mainframe_env_host_api::ImsPcbFeedbackResultV1, HostProblem> {
        self.ims_execute_selected_with(application, |ims| {
            ims.execute_pcb_feedback_v1(invocation, request)
        })
    }
    /// Execute with the catalog from the selected, published application package.
    pub fn ims_execute_selected(
        &self,
        application: &str,
        invocation: &Invocation,
        request: &mainframe_env_host_api::ImsRequest,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        self.ims_execute_selected_with(application, |ims| ims.execute(invocation, request))
    }

    /// SSA navigation uses the same signed selection and metadata publication fences.
    pub fn ims_navigation_selected(
        &self,
        application: &str,
        invocation: &Invocation,
        request: &mainframe_env_host_api::ImsNavigationRequest,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        self.ims_execute_selected_with(application, |ims| {
            ims.execute_navigation(invocation, request)
        })
    }

    /// GSAM shares the selected package and metadata publication boundary.
    pub fn ims_gsam_selected(
        &self,
        application: &str,
        invocation: &Invocation,
        request: &mainframe_env_host_api::ImsGsamRequest,
    ) -> Result<mainframe_env_host_api::ImsGsamResult, HostProblem> {
        self.ims_execute_selected_with(application, |ims| ims.execute_gsam(invocation, request))
    }

    fn ims_execute_selected_with<T>(
        &self,
        application: &str,
        execute: impl FnOnce(&ImsService) -> Result<T, HostProblem>,
    ) -> Result<T, HostProblem> {
        let _publication = self
            .application_publication
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let selected = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .installer
            .selected_generation(application)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        let catalog = selected
            .package()
            .sections
            .ims_metadata
            .as_ref()
            .ok_or(HostProblem::NotFound)?;
        let published = self
            .ims
            .selected_metadata_generation(application)?
            .ok_or(HostProblem::NotFound)?;
        if published.generation != selected.record().generation
            || published.package_identity != selected.record().identity
            || &published.catalog != catalog
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let publication = self
            .store
            .get_provider_state(
                APPLICATION_PUBLICATION_NAMESPACE,
                &selected.record().package.to_ascii_uppercase(),
            )
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let state: ApplicationPublicationState = serde_json::from_slice(&publication.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if !state.complete || state.identity != selected.record().identity {
            return Err(HostProblem::NotFound);
        }
        self.ims.install_metadata(published.catalog)?;
        execute(&self.ims)
    }
    /// Admit a message only against the complete selected signed package.
    pub fn ims_tm_enqueue(
        &self,
        application: &str,
        invocation: &Invocation,
        message: TmInputMessage,
    ) -> Result<TmEnqueueReceipt, HostProblem> {
        let _publication = self
            .application_publication
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.selected_ims_tm_application(application)?;
        self.ims_tm.enqueue(invocation, message)
    }

    pub fn ims_tm_claim(
        &self,
        application: &str,
        transaction: &str,
        worker: &str,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, HostProblem> {
        let _publication = self
            .application_publication
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.selected_ims_tm_application(application)?;
        self.ims_tm
            .claim(transaction, worker, now_tick, lease_ticks)
    }

    pub fn ims_tm_start(
        &self,
        application: &str,
        invocation: &Invocation,
        work: &WorkRecord,
    ) -> Result<TmScheduleReceipt, HostProblem> {
        self.verify_ims_tm_binding(application, &self.ims_tm.package_for_work(work)?)?;
        self.ims_tm.start(invocation, work)
    }

    pub fn ims_tm_claim_retained(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        transaction: &str,
        worker: &str,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, HostProblem> {
        let binding = TmPackageBinding {
            application: application.to_ascii_uppercase(),
            generation,
            package_identity: package_identity.into(),
        };
        self.verify_ims_tm_binding(application, &binding)?;
        self.ims_tm.claim_retained(
            application,
            generation,
            package_identity,
            transaction,
            worker,
            now_tick,
            lease_ticks,
        )
    }

    pub fn ims_tm_call(
        &self,
        application: &str,
        invocation: &Invocation,
        call: TmCall,
    ) -> Result<TmCallResult, HostProblem> {
        self.verify_ims_tm_binding(
            application,
            &self.ims_tm.package_for_call(invocation, &call)?,
        )?;
        self.ims_tm.call(invocation, call)
    }

    pub fn ims_tm_cancel(
        &self,
        application: &str,
        invocation: &Invocation,
        message_id: &str,
    ) -> Result<TmCancelReceipt, HostProblem> {
        self.verify_ims_tm_binding(application, &self.ims_tm.package_for_message(message_id)?)?;
        self.ims_tm.cancel(invocation, message_id)
    }

    fn selected_ims_tm_application(&self, application: &str) -> Result<(), HostProblem> {
        let selected = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .installer
            .selected_generation(application)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if selected.record().state != InstallState::Ready
            || selected.package().sections.ims_tm.is_none()
            || !self.ims_tm.selected_package_matches(
                &selected.record().package,
                selected.record().generation,
                &selected.record().identity,
            )?
        {
            return Err(HostProblem::NotFound);
        }
        let publication = self
            .store
            .get_provider_state(
                APPLICATION_PUBLICATION_NAMESPACE,
                &selected.record().package.to_ascii_uppercase(),
            )
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let state: ApplicationPublicationState = serde_json::from_slice(&publication.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if !state.complete || state.identity != selected.record().identity {
            return Err(HostProblem::NotFound);
        }
        Ok(())
    }

    fn verify_ims_tm_binding(
        &self,
        application: &str,
        binding: &TmPackageBinding,
    ) -> Result<(), HostProblem> {
        if !binding.application.eq_ignore_ascii_case(application) {
            return Err(HostProblem::Unauthorized);
        }
        let retained = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .installer
            .generation(
                &binding.application,
                binding.generation,
                &binding.package_identity,
            )
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        let definitions = retained.package().sections.ims_tm.as_ref();
        if retained.record().state != InstallState::Ready
            || definitions.is_none()
            || !self.ims_tm.retained_package_matches(
                &binding.application,
                binding.generation,
                &binding.package_identity,
                definitions.ok_or(HostProblem::NotFound)?,
            )?
        {
            return Err(HostProblem::NotFound);
        }
        Ok(())
    }

    pub(super) fn apply_application_ims_metadata(
        &self,
        selected: &SelectedApplicationGeneration,
    ) -> Result<(), HostProblem> {
        let package = selected.package();
        self.ims.publish_metadata_generation(
            &package.base.manifest.name,
            package.generation,
            &selected.record().identity,
            package.sections.ims_metadata.as_ref(),
        )?;
        self.ims_tm.publish_package_definitions(
            &package.base.manifest.name,
            package.generation,
            &selected.record().identity,
            package.sections.ims_tm.as_ref(),
        )?;
        Ok(())
    }
}
