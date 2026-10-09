use super::*;
use crate::product_provider::{unix_ms, JobReceipt};
use std::collections::BTreeSet;

struct Draft {
    request: DevelopmentRequest,
    result: DevelopmentResult,
    raw_response: Vec<u8>,
    receipt: JobReceipt,
    captures: Vec<CapturedProgram>,
    selected: usize,
    run: ProductRun,
    revision: u64,
}
impl Draft {
    fn basis(&self) -> Result<Basis, String> {
        let runtime = LocalRuntime::default();
        let source = canonical_digest(IdentityDomain::Source, &self.captures[self.selected])
            .map_err(error)?;
        Ok(Basis {
            snapshot: canonical_digest(
                IdentityDomain::Evidence,
                &(
                    &source,
                    runtime.data(&self.run),
                    runtime.session(&self.run),
                    self.run.clock_day(),
                    self.run.artifacts(),
                    self.revision,
                ),
            )
            .map_err(error)?,
            source,
            revision: self.revision,
            data_generation: runtime.data(&self.run).generation,
            data: canonical_digest(IdentityDomain::Data, runtime.data(&self.run)).map_err(error)?,
            session: canonical_digest(IdentityDomain::Session, runtime.session(&self.run))
                .map_err(error)?,
            decisions: canonical_digest(IdentityDomain::Decision, &self.request.decisions)
                .map_err(error)?,
            day: self.run.clock_day(),
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
        })
    }
    fn page(&self) -> Result<Page, String> {
        Ok(Page::Draft {
            basis: self.basis()?, labels: self.captures.iter().map(|c| c.program.label.clone()).collect(), selected: self.selected,
            model: LocalRuntime::default().view_model(&self.run).map_err(error)?,
            origin: match &self.result.producer {
                Producer::Fixture { name } => format!("Synthetic transport fixture: {name}. This is not live AI generation."),
                Producer::LiveAgent { provider, invocation_id, .. } => format!("Returned by {provider}, request {invocation_id}. Independently parsed and executed locally; broader fitness is unverified."),
                _ => return Err("Unexpected generation provenance".into()),
            },
            unsupported: self.result.response.unsupported.clone(),
        })
    }
}
struct Ready {
    prepared: PreparedDevelopment,
    request: DevelopmentRequest,
    transport: ProviderTransport,
    epoch: u64,
    basis: Option<Basis>,
    association: ProviderAssociation,
    reconciliation: Option<intention_flow::Reconciliation>,
}
struct DesignEvidence {
    association: ProviderAssociation,
    raw: Vec<u8>,
    receipt: JobReceipt,
    consent: ConsentReceipt,
}
impl DesignEvidence {
    fn check(&self, design: &intention_flow::Design) -> Result<(), String> {
        let request = &self.association.request;
        let binding = self
            .association
            .reconcile
            .as_ref()
            .ok_or("Missing new-design request binding")?;
        let response = DevelopmentResponse::parse(&self.raw).map_err(error)?;
        response.validate_for(request).map_err(error)?;
        let view = design.view();
        let suggestion = response
            .evolutions
            .iter()
            .find(|e| e.id == binding.evolution)
            .ok_or("New-design response lost its exact evolution")?;
        let source = response
            .candidates
            .iter()
            .find(|c| c.id == suggestion.candidate)
            .ok_or("New-design response lost its executable")?;
        let capture = CapturedProgram::capture(
            source.source_json.as_bytes(),
            &request.project_id,
            view.authored.binding.producer.clone(),
            None,
        )
        .map_err(error)?;
        if capture != view.authored
            || view.operation != binding.operation
            || suggestion.needs.iter().collect::<BTreeSet<_>>()
                != binding.needs.iter().collect::<BTreeSet<_>>()
            || self.receipt.state != JobState::TransportValidated
            || self.receipt.request_id != request.id
            || self.receipt.provider != self.association.provider
            || self.receipt.request_digest != self.association.wire_request
            || self.receipt.source_digest != self.association.wire_source
            || self.receipt.disclosure.digest() != self.consent.disclosure_digest
            || self.receipt.consent.as_ref() != Some(&self.consent)
        {
            return Err(
                "The new design no longer matches its actual response, consent and receipt".into(),
            );
        }
        DevelopmentResult {
            response,
            producer: view.authored.binding.producer,
        }
        .validate_for(request)
        .map_err(error)
    }
}
enum IntentionDraft {
    Design {
        design: intention_flow::Design,
        evidence: DesignEvidence,
    },
    Withdrawal(intention_flow::Withdrawal),
}
struct OpenTool {
    association: Association,
    store: ProductStore,
    snapshot: ProjectSnapshot,
}
struct Worker {
    config: Config,
    locations: Option<ToolLocations>,
    journal: Option<JournalFile>,
    page: Page,
    ready: Option<Ready>,
    draft: Option<Draft>,
    change: Option<ChangeDraft>,
    intention: Option<IntentionDraft>,
    recent_inputs: Vec<SemanticInput>,
    opened: Option<OpenTool>,
    chosen: Option<PathBuf>,
    generation_blocked: bool,
    notice: String,
    committed: Option<PathBuf>,
}
impl Worker {
    fn update(&self) -> Update {
        let mut notice = self.notice.clone();
        let recent = match self
            .locations
            .as_ref()
            .map(|l| RecentTools::open(l.path()).and_then(|r| r.list()))
        {
            Some(Ok(recent)) => recent,
            Some(Err(e)) => {
                notice.push_str(&format!(
                    " Recent tools could not be read: {e}. Existing files were kept."
                ));
                vec![]
            }
            None => vec![],
        };
        let pending = self
            .journal
            .as_ref()
            .is_some_and(|j| j.value.pending.is_some() || j.value.provider.is_some());
        let durable_pending = self.journal.as_ref().is_some_and(|j| {
            matches!(
                j.value.pending,
                Some(
                    Interrupted::Create { .. }
                        | Interrupted::Daily { .. }
                        | Interrupted::Change { .. }
                )
            )
        });
        Update {
            page: Some(self.page.clone()),
            need: None,
            recent,
            notice,
            destination: self
                .chosen
                .clone()
                .or_else(|| self.locations.as_ref().map(|l| l.path().to_path_buf())),
            mutation_blocked: self.journal.is_none() || durable_pending,
            generation_blocked: self.journal.is_none()
                || self.generation_blocked
                || durable_pending,
            pending: pending && self.ready.is_none(),
            abandon_creation: self
                .journal
                .as_ref()
                .is_some_and(|j| matches!(j.value.pending, Some(Interrupted::Create { .. }))),
            abandon_daily: self
                .journal
                .as_ref()
                .is_some_and(|j| matches!(j.value.pending, Some(Interrupted::Daily { .. }))),
            unsaved: self
                .journal
                .as_ref()
                .and_then(|j| j.value.last_unsaved.as_ref())
                .map(|u| {
                    format!(
                        "{}\n{}\nThis entry was not saved or automatically resubmitted.",
                        u.summary, u.explanation
                    )
                }),
        }
    }
    fn journal(&mut self, edit: impl FnOnce(&mut journal::Journal)) -> Result<(), String> {
        let journal = self
            .journal
            .as_mut()
            .ok_or("Restart information is unavailable; new writes are paused")?;
        let mut value = journal.value.clone();
        edit(&mut value);
        journal.write(value)
    }
    fn no_pending(&self) -> Result<(), String> {
        if self
            .journal
            .as_ref()
            .ok_or("Restart information is unavailable; new writes are paused")?
            .value
            .pending
            .is_some()
        {
            Err("An unfinished operation must be reconciled before another write".into())
        } else {
            Ok(())
        }
    }
    fn boot(&mut self) -> Result<(), String> {
        self.locations = Some(match &self.config.root {
            Some(root) => ToolLocations::chosen(root).map_err(error)?,
            None => ToolLocations::default_location().map_err(error)?,
        });
        self.journal = Some(JournalFile::open(self.locations.as_ref().unwrap().path())?);
        if let Err(e) = self.reconcile(false, &Gate::default()) {
            self.notice = e;
        }
        if self.opened.is_none() && !matches!(self.page, Page::Upgrade { .. }) {
            let last = self.journal.as_ref().and_then(|j| j.value.last.clone());
            if let Some(last) = last {
                if let Err(e) = self.open(last.path, Some(last.identity)) {
                    self.notice = format!("Your last tool could not be opened. {e}");
                }
            }
        }
        Ok(())
    }
    fn today(&self) -> i32 {
        #[cfg(test)]
        {
            self.config.hooks.today.load(Ordering::Acquire)
        }
        #[cfg(not(test))]
        {
            day()
        }
    }
    fn transport(&self, provider: ProviderKind) -> Result<ProviderTransport, String> {
        #[cfg(test)]
        if let Some(transport) = &self.config.transport {
            return Ok(transport.clone());
        }
        let name = match provider {
            ProviderKind::Codex => "codex",
            ProviderKind::Claude => "claude",
        };
        let executable = find_endpoint(name).ok_or_else(|| format!("{name} is not available on this computer. You can still open and use saved tools offline"))?;
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or("The provider's normal home folder is unavailable")?;
        ProviderTransport::new(
            self.locations
                .as_ref()
                .ok_or("Tool location unavailable")?
                .path()
                .join("provider-jobs"),
            provider,
            executable,
            home,
        )
    }
    fn prepare(
        &mut self,
        need: String,
        provider: ProviderKind,
        profile: CapabilityProfile,
        key: &Key,
        gate: &Gate,
        modify: bool,
        focused: Option<RecordRef>,
    ) -> Result<(), String> {
        self.no_pending()?;
        if self.generation_blocked
            || self
                .journal
                .as_ref()
                .is_some_and(|j| j.value.provider.is_some())
        {
            return Err("The earlier provider process has an unresolved outcome; no new generation was sent".into());
        }
        let modify_binding = if modify {
            let current = self
                .opened
                .as_ref()
                .ok_or("Open the saved tool before requesting a change")?;
            let basis = Basis::capture(&current.snapshot)?;
            if key.basis.as_ref() != Some(&basis)
                || current.store.load().map_err(error)? != current.snapshot
                || self.today() != basis.day
            {
                return Err("The current source, data, session or date changed. Reopen saved work before requesting this change".into());
            }
            Some((current.association.clone(), basis))
        } else {
            None
        };
        let request = if modify {
            let current = self.opened.as_ref().unwrap();
            let model = current.store.runtime_view().map_err(error)?;
            if current.store.load().map_err(error)? != current.snapshot {
                return Err(
                    "Saved work changed while its context was captured. Reopen it before sending"
                        .into(),
                );
            }
            let mut selected = model.observation.selected.clone();
            if let Some(focused) = focused {
                if !model.observation.rows.iter().any(|r| r.record == focused)
                    || !current
                        .snapshot
                        .data
                        .records
                        .iter()
                        .any(|r| r.entity == focused.entity && r.id == focused.record)
                {
                    return Err("The focused record is no longer in this saved view. Focus current work before requesting the change".into());
                }
                if !selected.contains(&focused) {
                    selected.push(focused);
                }
            }
            if let Some(focused) = &current.snapshot.session.focused_record {
                if !selected.contains(focused) {
                    selected.push(focused.clone());
                }
            }
            let context = DevelopmentContext {
                view: Some(current.snapshot.session.view.clone()),
                selected,
                recent_inputs: self.recent_inputs.clone(),
                data_digest: Some(current.snapshot.data.identity().map_err(error)?),
                session_digest: Some(current.snapshot.session.identity().map_err(error)?),
            };
            DecisionEngine::new(
                LocalRuntime::with_cancellation(gate.cancelled.clone()),
                IntentArchive::new(current.store.clone()),
            )
            .development_request(
                &current.snapshot,
                &id("request"),
                DevelopmentOperation::Modify,
                &need,
                context,
            )
            .map_err(error)?
        } else {
            DevelopmentRequest {
                version: CONTRACT_VERSION,
                id: id("request"),
                project_id: id("project"),
                operation: DevelopmentOperation::Generate,
                request: need.clone(),
                sources: vec![],
                context: DevelopmentContext {
                    view: None,
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: None,
                    session_digest: None,
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: DecisionGraph {
                    version: CONTRACT_VERSION,
                    revision: 0,
                    decisions: vec![],
                },
                unknowns: vec![],
                required_capabilities: BTreeSet::new(),
            }
        };
        self.prepare_request(
            need,
            provider,
            profile,
            key,
            gate,
            request,
            modify_binding,
            None,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare_request(
        &mut self,
        need: String,
        provider: ProviderKind,
        profile: CapabilityProfile,
        key: &Key,
        gate: &Gate,
        request: DevelopmentRequest,
        modify: Option<(Association, Basis)>,
        reconcile: Option<ReconcileAssociation>,
        reconciliation: Option<intention_flow::Reconciliation>,
    ) -> Result<(), String> {
        self.no_pending()?;
        if self.generation_blocked
            || self
                .journal
                .as_ref()
                .is_some_and(|j| j.value.provider.is_some())
        {
            return Err("The earlier provider process has an unresolved outcome; no new generation was sent".into());
        }
        request.validate().map_err(error)?;
        self.journal(|j| j.need = need.clone())?;
        let basis = modify
            .as_ref()
            .map(|(_, basis)| basis.clone())
            .or_else(|| reconcile.as_ref().map(|r| r.basis.clone()));
        // Only explicitly injected test transports can attest a fixture profile.
        #[cfg(test)]
        let profile = if self.config.transport.is_some() {
            CapabilityProfile::DataOnly
        } else {
            profile
        };
        let options = ProviderOptions {
            provider,
            profile,
            ..ProviderOptions::default()
        };
        let transport = self.transport(provider)?;
        gate.check()?;
        let prepared =
            prepare_development(transport.clone(), &request, options.clone()).map_err(error)?;
        gate.check()?;
        let wire = encode_request(&request, &options).map_err(error)?;
        let pending = ProviderAssociation {
            request: request.clone(),
            provider,
            profile,
            wire_request: wire.digest()?,
            wire_source: wire.source_digest,
            issued: false,
            modify,
            reconcile,
        };
        self.journal(|j| {
            j.need = need.clone();
            j.provider = Some(pending.clone());
        })?;
        self.ready = None;
        self.draft = None;
        self.change = None;
        self.intention = None;
        if basis.is_none() {
            self.opened = None;
        }
        self.page = Page::Consent {
            disclosure: prepared.disclosure().clone(),
            need,
            request: request.clone(),
            review: String::from_utf8(wire.prompt).map_err(error)?,
            basis: basis.clone(),
        };
        #[cfg(test)]
        if self.config.transport.is_some() {
            self.notice = "Synthetic transport fixture selected for this test only".into();
        }
        self.ready = Some(Ready {
            prepared,
            request,
            transport,
            epoch: key.epoch,
            basis,
            association: pending,
            reconciliation,
        });
        Ok(())
    }
    fn generate(&mut self, disclosure: String, key: &Key, gate: &Gate) -> Result<(), String> {
        #[cfg(test)]
        match self.config.hooks.provider_association_fault {
            Some(TestProviderAssociationFault::Missing) => self.journal(|j| j.provider = None)?,
            Some(TestProviderAssociationFault::DifferentRequest) => {
                let mut association = self
                    .journal
                    .as_ref()
                    .unwrap()
                    .value
                    .provider
                    .clone()
                    .unwrap();
                association
                    .request
                    .request
                    .push_str(" with an unrelated change");
                let wire = encode_request(
                    &association.request,
                    &ProviderOptions {
                        provider: association.provider,
                        profile: association.profile,
                        ..ProviderOptions::default()
                    },
                )
                .map_err(error)?;
                association.wire_request = wire.digest()?;
                association.wire_source = wire.source_digest;
                self.journal(|j| j.provider = Some(association))?;
            }
            None => (),
        }
        let ready = self
            .ready
            .as_ref()
            .ok_or("Prepare and review a fresh request before generating")?;
        if ready.epoch != key.epoch || ready.prepared.disclosure().digest() != disclosure {
            return Err("The prepared request changed; review it again before authorizing".into());
        }
        if let Some(basis) = &ready.basis {
            let current = self.current(key)?;
            if key.basis.as_ref() != Some(basis)
                || Basis::capture(&current.store.load().map_err(error)?)? != *basis
                || self.today() != basis.day
            {
                return Err("Saved work changed after disclosure. Review a fresh change request before sending".into());
            }
        }
        let association = self
            .journal
            .as_ref()
            .and_then(|j| j.value.provider.as_ref())
            .ok_or("The prepared request is no longer available. Go back and prepare a fresh request before sending.")?;
        if association.issued
            || canonical_bytes(association).map_err(error)?
                != canonical_bytes(&ready.association).map_err(error)?
        {
            return Err(
                "The prepared request changed. Go back and prepare a fresh request before sending."
                    .into(),
            );
        }
        let mut issued = association.clone();
        issued.issued = true;
        gate.check()?;
        self.journal(|j| j.provider = Some(issued))?;
        let ready = self.ready.take().unwrap();
        let consent = ConsentReceipt {
            disclosure_digest: disclosure,
            approval_reference: format!("studio-click-{}", key.operation),
            expires_at_unix_ms: unix_ms().saturating_add(5 * 60_000),
        };
        #[cfg(test)]
        let generation_started = std::time::Instant::now();
        let provider = ready.prepared.authorize(consent.clone());
        let ordinary = ready.reconciliation.is_some()
            && self.opened.as_ref().is_some_and(|o| {
                !o.snapshot
                    .scope
                    .compositions
                    .contains_key(&o.snapshot.active_revision)
            });
        let (developed, result) = if ordinary {
            let current = self.opened.as_ref().unwrap();
            let engine = DecisionEngine::new(
                LocalRuntime::with_cancellation(gate.cancelled.clone()),
                IntentArchive::new(current.store.clone()),
            );
            (
                Some(ready.reconciliation.as_ref().unwrap().develop(
                    &current.store,
                    &engine,
                    &provider,
                    gate.cancelled.clone(),
                )),
                None,
            )
        } else {
            (
                None,
                Some(
                    provider
                        .develop(&ready.request, &|| gate.cancelled.load(Ordering::Acquire))
                        .map_err(error),
                ),
            )
        };
        #[cfg(test)]
        if ready.reconciliation.is_some() {
            eprintln!(
                "Studio reconciliation provider returned: {:?}",
                generation_started.elapsed()
            );
        }
        let receipt = provider.receipt();
        let raw = provider.raw_response();
        if ready.basis.is_none() {
            self.page = Page::Home;
        } else if ready.reconciliation.is_none() {
            if let Some(opened) = &self.opened {
                self.page = daily_page(&opened.association, &opened.store, &opened.snapshot)?;
            }
        }
        // Dropping the provider/job and any cancellation join stays on this worker.
        drop(provider);
        let terminal = ready.transport.reconcile(&ready.request.id);
        match &terminal {
            Ok(receipt)
                if receipt.request_id == ready.request.id
                    && receipt.request_digest == ready.association.wire_request
                    && receipt.source_digest == ready.association.wire_source
                    && receipt.state.is_terminal()
                    && receipt.state != JobState::Interrupted =>
            {
                self.journal(|j| j.provider = None)?
            }
            Ok(receipt)
                if receipt.state == JobState::Prepared
                    && receipt.request_id == ready.request.id
                    && receipt.request_digest == ready.association.wire_request
                    && receipt.source_digest == ready.association.wire_source =>
            {
                self.journal(|j| j.provider = None)?
            }
            _ => self.generation_blocked = true,
        }
        if ready.reconciliation.is_some() {
            // A managed result reaches the domain verifier only after the real
            // transport has supplied the exact authorized terminal receipt.
            if let Some(receipt) = &receipt {
                if receipt.state != JobState::TransportValidated
                    || receipt.request_id != ready.request.id
                    || receipt.request_digest != ready.association.wire_request
                    || receipt.source_digest != ready.association.wire_source
                    || receipt.provider != ready.association.provider
                    || receipt.disclosure.digest() != consent.disclosure_digest
                    || receipt.consent.as_ref() != Some(&consent)
                {
                    return Err(
                        "New-design provider receipt differs from the exact authorized request"
                            .into(),
                    );
                }
            }
            let design = if let Some(developed) = developed {
                developed?
            } else {
                let result = result.ok_or("Missing actual new-design result")??;
                let raw = raw
                    .as_ref()
                    .ok_or("No actual provider response bytes were returned")?;
                result.validate_for(&ready.request).map_err(error)?;
                if DevelopmentResponse::parse(raw).map_err(error)? != result.response {
                    return Err(
                        "The actual new-design result differs from its response bytes".into(),
                    );
                }
                let current = self.current(key)?;
                let engine = DecisionEngine::new(
                    LocalRuntime::with_cancellation(gate.cancelled.clone()),
                    IntentArchive::new(current.store.clone()),
                );
                ready.reconciliation.as_ref().unwrap().develop_prepared(
                    &current.store,
                    &engine,
                    result,
                    gate.cancelled.clone(),
                )?
            };
            self.current(key)?;
            let evidence = DesignEvidence {
                association: ready.association,
                raw: raw.ok_or("No actual provider response bytes were returned")?,
                receipt: receipt.ok_or("No actual provider receipt was returned")?,
                consent,
            };
            evidence.check(&design)?;
            if !gate.finish() {
                return Err("New-design preview cancelled; saved work was kept".into());
            }
            self.intention = Some(IntentionDraft::Design { design, evidence });
            self.page = self.intention_page()?;
            self.notice = "The actual returned new design was checked against the selected needs and independent promises. Try copied work before choosing to save it".into();
            return Ok(());
        }
        let result = result.ok_or("Missing actual provider result")??;
        gate.check()?;
        let receipt = receipt.ok_or("No actual provider receipt was returned")?;
        let raw = raw.ok_or("No actual provider response bytes were returned")?;
        result.validate_for(&ready.request).map_err(error)?;
        if DevelopmentResponse::parse(&raw).map_err(error)? != result.response
            || receipt.state != JobState::TransportValidated
            || receipt.request_id != ready.request.id
        {
            return Err("Provider result, raw response, and receipt do not match".into());
        }
        if result.response.candidates.is_empty() {
            return Err(format!(
                "The provider could not generate this tool: {}",
                result.response.unsupported.join("; ")
            ));
        }
        let runtime = LocalRuntime::with_cancellation(gate.cancelled.clone());
        let mut captures = Vec::new();
        for candidate in &result.response.candidates {
            let capture = CapturedProgram::capture(
                candidate.source_json.as_bytes(),
                &ready.request.project_id,
                result.producer.clone(),
                None,
            )
            .map_err(error)?;
            runtime.validate(&capture).map_err(error)?;
            captures.push(capture);
        }
        if let Some(basis) = ready.basis {
            let current = self
                .opened
                .as_ref()
                .ok_or("The original saved tool is no longer open")?;
            if Basis::capture(&current.store.load().map_err(error)?)? != basis
                || self.today() != basis.day
            {
                self.page = daily_page(&current.association, &current.store, &current.snapshot)?;
                return Err("Saved work changed while the provider was working. Its stale result was not adopted; make a fresh change request".into());
            }
            let mut change = ChangeDraft::new(
                current.snapshot.clone(),
                captures.remove(0),
                self.journal
                    .as_ref()
                    .map(|j| j.value.need.clone())
                    .unwrap_or_else(|| ready.request.request.clone()),
            )?;
            change.request = Some(ready.request);
            change.result = Some(result);
            change.raw = Some(raw);
            change.receipt = Some(receipt);
            // Retain the actual returned alternative even if its shape needs a
            // business clarification or a supported design route.
            let result = change.prepare(&current.store, gate);
            self.page = Page::Change(change.checked_view(&current.store, gate)?);
            self.change = Some(change);
            self.notice =
                "Try the actual change on copied work. Nothing in your saved tool changed".into();
            return result;
        }
        let run = empty_run(&captures[0], self.today(), &runtime)?;
        gate.check()?;
        let draft = Draft {
            request: ready.request,
            result,
            raw_response: raw,
            receipt,
            captures,
            selected: 0,
            run,
            revision: 0,
        };
        self.page = draft.page()?;
        self.draft = Some(draft);
        self.notice = "The actual returned draft is ready to try. Save it explicitly before entering real work".into();
        Ok(())
    }
    fn check_draft(&self, key: &Key) -> Result<(), String> {
        let draft = self.draft.as_ref().ok_or("No generated draft is open")?;
        if key.basis.as_ref() != Some(&draft.basis()?) {
            return Err(
                "The draft source, data, or session changed; try the current view again".into(),
            );
        }
        Ok(())
    }
    fn preview(&mut self, input: SemanticInput, key: &Key, gate: &Gate) -> Result<(), String> {
        self.check_draft(key)?;
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_preview {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        gate.check()?;
        let draft = self.draft.as_mut().unwrap();
        let runtime = LocalRuntime::with_cancellation(gate.cancelled.clone());
        let mut run = runtime
            .resume(
                &draft.captures[draft.selected],
                runtime.data(&draft.run),
                runtime.session(&draft.run),
                draft.run.clock_day(),
                0,
                RuntimeLimits::default(),
                draft.run.artifacts(),
            )
            .map_err(error)?;
        runtime
            .apply(&mut run, &input, &key.operation)
            .map_err(error)?;
        // Compute the complete view before replacing the old copied run.
        runtime.view_model(&run).map_err(error)?;
        if !gate.finish() {
            return Err("Preview cancelled; the previous draft was kept".into());
        }
        draft.run = run;
        draft.revision = draft
            .revision
            .checked_add(1)
            .ok_or("Draft revision overflow")?;
        self.page = draft.page()?;
        Ok(())
    }
    fn select(&mut self, candidate: usize, key: &Key, gate: &Gate) -> Result<(), String> {
        self.check_draft(key)?;
        let draft = self.draft.as_mut().unwrap();
        let source = draft
            .captures
            .get(candidate)
            .ok_or("That returned candidate is unavailable")?;
        let run = empty_run(
            source,
            draft.run.clock_day(),
            &LocalRuntime::with_cancellation(gate.cancelled.clone()),
        )?;
        if !gate.finish() {
            return Err("Preview cancelled; the previous draft was kept".into());
        }
        draft.run = run;
        draft.selected = candidate;
        draft.revision = draft
            .revision
            .checked_add(1)
            .ok_or("Draft revision overflow")?;
        self.page = draft.page()?;
        self.notice =
            "This candidate has a fresh isolated preview; no draft records were kept".into();
        Ok(())
    }
    fn open(&mut self, path: PathBuf, expected: Option<ToolIdentity>) -> Result<(), String> {
        let opened = match inspect_open(&path, expected.as_ref()).map_err(error)? {
            OpenGate::UpgradeRequired(summary) => {
                let tool = Association {
                    path,
                    identity: ToolIdentity {
                        project_id: summary.project_id.clone(),
                        first_program: summary.first_program.clone(),
                    },
                };
                self.journal(|j| j.last = Some(tool.clone()))?;
                self.opened = None;
                self.draft = None;
                self.ready = None;
                self.page = Page::Upgrade { tool, summary };
                return Ok(());
            }
            OpenGate::Ready(opened) => opened,
        };
        let association = Association {
            path,
            identity: opened.summary.identity.clone(),
        };
        self.install_opened(association, opened)
    }
    fn install_opened(
        &mut self,
        association: Association,
        mut opened: crate::product_backup::OpenedTool,
    ) -> Result<(), String> {
        // open_verified checked snapshot, intention objects and identity
        // together. Do not discard that result and open it all again.
        // daily_page still fences its fresh rendered view to this exact basis.
        if association.identity != opened.summary.identity {
            return Err("The verified tool belongs to another saved identity".into());
        }
        let page = daily_page(&association, &opened.store, &opened.snapshot)?;
        self.opened = Some(OpenTool {
            association: association.clone(),
            store: opened.store.clone(),
            snapshot: opened.snapshot.clone(),
        });
        self.draft = None;
        self.change = None;
        self.intention = None;
        self.recent_inputs.clear();
        self.ready = None;
        self.page = page;
        self.journal(|j| j.last = Some(association.clone()))?;
        self.post_save_opened(&association, Some(&mut opened));
        Ok(())
    }
    fn return_daily(&mut self, gate: &Gate) -> Result<(), String> {
        let association = self
            .opened
            .as_ref()
            .ok_or("No saved tool is open")?
            .association
            .clone();
        // Stage every fallible view check before abandoning prepared consent or
        // copied work. The same verified open is handed to checkpoint creation.
        let staged = inspect_open(&association.path, Some(&association.identity)).map_err(error)?;
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_return_install {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        gate.check()?;
        let (page, mut verified) = match staged {
            OpenGate::Ready(opened) => {
                let page = daily_page(&association, &opened.store, &opened.snapshot)?;
                (page, Some(opened))
            }
            OpenGate::UpgradeRequired(summary) => {
                if association.identity
                    != (ToolIdentity {
                        project_id: summary.project_id.clone(),
                        first_program: summary.first_program.clone(),
                    })
                {
                    return Err("The saved tool identity changed while returning".into());
                }
                (
                    Page::Upgrade {
                        tool: association.clone(),
                        summary,
                    },
                    None,
                )
            }
        };
        if !gate.finish() {
            return Err(
                "Returning was cancelled; the prepared request and copied work were kept".into(),
            );
        }
        let abandon_ready = self.ready.is_some();
        self.journal(|j| {
            if abandon_ready && j.provider.as_ref().is_some_and(|p| !p.issued) {
                j.provider = None;
            }
            j.last = Some(association.clone());
        })?;
        self.opened = verified.as_ref().map(|opened| OpenTool {
            association: association.clone(),
            store: opened.store.clone(),
            snapshot: opened.snapshot.clone(),
        });
        self.draft = None;
        self.change = None;
        self.intention = None;
        self.ready = None;
        self.recent_inputs.clear();
        self.page = page;
        if let Some(opened) = verified.as_mut() {
            self.post_save_opened(&association, Some(opened));
        }
        Ok(())
    }
    fn upgrade(
        &mut self,
        tool: &Association,
        summary: &UpgradeSummary,
        gate: &Gate,
    ) -> Result<(), String> {
        if !matches!(&self.page, Page::Upgrade { tool: shown, summary: shown_summary } if shown == tool && shown_summary == summary)
        {
            return Err("The displayed upgrade request changed; open the saved tool again".into());
        }
        if self
            .journal
            .as_ref()
            .and_then(|j| j.value.pending.as_ref())
            .is_some_and(|pending| {
                let (Interrupted::Create { tool: pending_tool }
                | Interrupted::Daily {
                    tool: pending_tool, ..
                }
                | Interrupted::Change {
                    tool: pending_tool, ..
                }) = pending;
                pending_tool != tool
            })
        {
            return Err(
                "Resolve the unfinished save for the other tool before upgrading this one".into(),
            );
        }
        self.before_commit(gate);
        gate.check()?;
        match inspect_open(&tool.path, Some(&tool.identity)).map_err(error)? {
            OpenGate::UpgradeRequired(current) if current == *summary => (),
            OpenGate::Ready(_) => {
                self.notice = "This tool is already upgraded; reopening its verified current work".into();
                return self.open(tool.path.clone(), Some(tool.identity.clone()));
            }
            _ => return Err("Saved work changed since the upgrade was shown. Open it again to review the current upgrade".into()),
        }
        gate.commit()?;
        // No ordinary daily handle survives migration. The verified helper
        // drops its RestartRequired handle; only a fresh open may render/edit.
        self.opened = None;
        let result = upgrade_open_verified(&tool.path, Some(&tool.identity), |stage| {
            gate.upgrade_stage.store(
                match stage {
                    UpgradeProgress::Validating => 1,
                    UpgradeProgress::Staged => 2,
                    UpgradeProgress::Verified => 3,
                    UpgradeProgress::Activating => 4,
                    UpgradeProgress::RestartRequired => 5,
                },
                Ordering::Release,
            );
        });
        #[cfg(test)]
        let result = if result.is_ok() && self.config.hooks.lose_ack.swap(false, Ordering::AcqRel) {
            Err(crate::product_locations::OperationIssue {
                kind: crate::product_locations::IssueKind::Unavailable,
                message: "injected lost upgrade acknowledgement".into(),
                next_step: "reconcile this exact path".into(),
                detail: String::new(),
            })
        } else {
            result
        };
        // A failed acknowledgement is not evidence of an uncommitted upgrade.
        // Reconcile the same path/identity, never create or overwrite a copy.
        match inspect_open(&tool.path, Some(&tool.identity)).map_err(error)? {
            OpenGate::Ready(_) => {
                self.committed = Some(tool.path.clone());
                self.after_commit();
                self.notice = "Local format upgrade verified. The tool was restarted and reopened with its saved work; original snapshots and backups were kept".into();
                self.open(tool.path.clone(), Some(tool.identity.clone()))?;
                if self.journal.as_ref().is_some_and(|j| j.value.pending.is_some()) {
                    if let Err(e) = self.reconcile(false, gate) {
                        self.notice.push_str(&format!(" The earlier input remains recorded for recovery: {e}"));
                    }
                }
                Ok(())
            }
            OpenGate::UpgradeRequired(_) => Err(format!("The upgrade did not activate. Original saved work is kept; review and retry the upgrade. {:?}", result.err())),
        }
    }
    fn post_save(&mut self, tool: &Association) {
        self.post_save_opened(tool, None);
    }
    fn post_save_opened(
        &mut self,
        tool: &Association,
        opened: Option<&mut crate::product_backup::OpenedTool>,
    ) {
        let result = (|| -> Result<(), String> {
            let locations = self
                .locations
                .as_ref()
                .ok_or("Default tool location unavailable")?;
            let recent = RecentTools::open(locations.path()).map_err(error)?;
            let instance = recent.remember(&tool.path, unix_ms()).map_err(error)?;
            let store = &self.opened.as_ref().ok_or("Saved tool is not open")?.store;
            let shelf =
                CheckpointShelf::for_tool(locations, &tool.identity, &instance).map_err(error)?;
            match opened {
                Some(opened) => opened.checkpoint(&shelf),
                None => shelf.capture(store),
            }
            .map_err(error)?;
            Ok(())
        })();
        if let Err(e) = result {
            self.notice = format!("The tool is saved and usable. Its recent-list registration or backup needs attention: {e}. Do not repeat the saved operation");
        }
    }
    fn save(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        self.check_draft(key)?;
        let draft = self.draft.as_ref().unwrap();
        draft.result.validate_for(&draft.request).map_err(error)?;
        if DevelopmentResponse::parse(&draft.raw_response).map_err(error)? != draft.result.response
            || draft.receipt.state != JobState::TransportValidated
        {
            return Err("The returned candidate provenance is no longer valid".into());
        }
        let capture = draft.captures[draft.selected].clone();
        let exact = CapturedProgram::capture(
            draft.result.response.candidates[draft.selected]
                .source_json
                .as_bytes(),
            &draft.request.project_id,
            draft.result.producer.clone(),
            None,
        )
        .map_err(error)?;
        if exact != capture {
            return Err("The candidate differs from the actual returned source".into());
        }
        let path = if let Some(folder) = &self.chosen {
            ToolLocations::chosen(folder)
                .map_err(error)?
                .new_tool_path()
                .map_err(error)?
        } else {
            self.locations
                .as_ref()
                .ok_or("Default tool folder unavailable")?
                .new_tool_path()
                .map_err(error)?
        };
        let association = Association {
            path: path.clone(),
            identity: ToolIdentity {
                project_id: capture.binding.project_id.clone(),
                first_program: canonical_digest(IdentityDomain::Source, &capture).map_err(error)?,
            },
        };
        self.journal(|j| {
            j.pending = Some(Interrupted::Create {
                tool: association.clone(),
            })
        })?;
        self.before_commit(gate);
        if let Err(e) = gate.commit() {
            self.journal(|j| j.pending = None)?;
            return Err(e);
        }
        #[cfg(test)]
        let result = if self
            .config
            .hooks
            .fail_creation
            .swap(false, Ordering::AcqRel)
        {
            Err(crate::product_store::StoreError::Invalid(
                "injected creation failure before activation".into(),
            ))
        } else {
            ProductStore::create(&path, &capture, self.today())
        };
        #[cfg(not(test))]
        let result = ProductStore::create(&path, &capture, self.today());
        #[cfg(test)]
        let result = if self.config.hooks.lose_ack.swap(false, Ordering::AcqRel) && result.is_ok() {
            Err(crate::product_store::StoreError::Invalid(
                "injected lost creation acknowledgement".into(),
            ))
        } else {
            result
        };
        // Even a create error can follow CURRENT activation: inspect this exact
        // path and initial identity, never allocate another destination here.
        let opened = open_verified(&path, Some(&association.identity));
        match opened {
            Ok(opened) => {
                let initial = &opened.snapshot;
                if initial.programs.len() != 1 || initial.programs[0] != capture { return Err("The selected path does not contain the exact approved initial capture; saving remains unresolved".into()); }
                self.committed = Some(path.clone());
                self.after_commit();
                self.opened = Some(OpenTool { association: association.clone(), store: opened.store, snapshot: opened.snapshot });
                self.page = daily_page(&association, &self.opened.as_ref().unwrap().store, &self.opened.as_ref().unwrap().snapshot)?;
                self.draft = None;
                self.journal(|j| { j.pending = None; j.last = Some(association.clone()); })?;
                self.notice = "Tool saved. You can now enter real work; the isolated draft records were not saved".into();
                self.post_save(&association);
                Ok(())
            }
            Err(e) => Err(format!("The save outcome at {} is unresolved. No new tool will be created until this same location is checked. Create: {:?}; check: {e}", path.display(), result.err())),
        }
    }
    fn daily(&mut self, input: SemanticInput, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        let current = self
            .opened
            .as_ref()
            .ok_or("Open a saved tool before entering work")?;
        let basis = Basis::capture(&current.snapshot)?;
        if !matches!(input, SemanticInput::AdvanceClock { .. }) && basis.day != self.today() {
            return Err(
                "The tool date changed; update it to today before entering current-day work".into(),
            );
        }
        if key.basis.as_ref() != Some(&basis) {
            return Err(
                "The displayed source, data, or session changed; reopen the current tool".into(),
            );
        }
        let association = current.association.clone();
        let fresh = open_verified(&association.path, Some(&association.identity)).map_err(error)?;
        if Basis::capture(&fresh.snapshot)? != basis {
            return Err(
                "The tool changed in another window. Reopen it before entering this work".into(),
            );
        }
        let operation = key.operation.clone();
        self.journal(|j| {
            j.pending = Some(Interrupted::Daily {
                tool: association.clone(),
                basis: basis.clone(),
                operation: operation.clone(),
                input: input.clone(),
            })
        })?;
        self.before_commit(gate);
        if let Err(e) = gate.commit() {
            self.journal(|j| j.pending = None)?;
            return Err(e);
        }
        #[cfg(test)]
        let result = if self.config.hooks.lose_ack.swap(false, Ordering::AcqRel) {
            fresh.store.apply_with_fault(
                basis.revision,
                &operation,
                &input,
                RuntimeLimits::default(),
                crate::product_store::FaultPoint::AfterPointer,
            )
        } else {
            fresh
                .store
                .apply(basis.revision, &operation, &input, RuntimeLimits::default())
        };
        #[cfg(not(test))]
        let result =
            fresh
                .store
                .apply(basis.revision, &operation, &input, RuntimeLimits::default());
        let mut checked = open_verified(&association.path, Some(&association.identity)).map_err(|e| format!("The save outcome is unresolved: {e}. Keep this operation for exact reconciliation"))?;
        if has_receipt(&checked.snapshot, &operation, &input)? {
            self.committed = Some(association.path.clone());
            self.after_commit();
            self.opened = Some(OpenTool {
                association: association.clone(),
                store: checked.store.clone(),
                snapshot: checked.snapshot.clone(),
            });
            self.page = daily_page(
                &association,
                &self.opened.as_ref().unwrap().store,
                &self.opened.as_ref().unwrap().snapshot,
            )?;
            self.journal(|j| {
                j.pending = None;
                j.last = Some(association.clone());
            })?;
            self.recent_inputs.push(input);
            if self.recent_inputs.len() > 16 {
                self.recent_inputs.remove(0);
            }
            self.notice = "Your work was saved".into();
            self.post_save_opened(&association, Some(&mut checked));
            Ok(())
        } else {
            // A fresh verified snapshot has no such receipt: no acknowledged
            // commit. Preserve the user's editor and let them decide what next.
            self.journal(|j| j.pending = None)?;
            Err(format!(
                "The input was not saved. {}",
                result
                    .err()
                    .map(error)
                    .unwrap_or_else(|| "The operation receipt was missing".into())
            ))
        }
    }
    fn reconcile_provider(&mut self) -> Result<(), String> {
        let Some(ProviderAssociation {
            request,
            provider,
            wire_request,
            wire_source,
            issued,
            ..
        }) = self.journal.as_ref().and_then(|j| j.value.provider.clone())
        else {
            return Ok(());
        };
        self.generation_blocked = true;
        if !issued {
            self.journal(|j| j.provider = None)?;
            self.generation_blocked = false;
            self.notice = "The earlier request was prepared but never authorized or sent. Review a fresh disclosure to continue".into();
            return Ok(());
        }
        let transport = self.transport(provider)?;
        let receipt = transport.reconcile(&request.id)?;
        if receipt.request_digest != wire_request
            || receipt.source_digest != wire_source
            || receipt.request_id != request.id
        {
            self.generation_blocked = true;
            return Err(
                "The interrupted provider receipt no longer matches its exact request".into(),
            );
        }
        if receipt.state == JobState::Interrupted || receipt.state == JobState::Running {
            self.generation_blocked = true;
            self.notice = "The earlier provider was interrupted and its process cleanup is unconfirmed. No late file was imported and no new request will be submitted. Resolve that provider process before generating again; saved tools remain usable offline".into();
        } else {
            self.journal(|j| j.provider = None)?;
            self.generation_blocked = false;
            self.notice = format!("The earlier provider request ended as {:?}. Its draft was not reopened and nothing was resubmitted. Your original need was kept", receipt.state);
        }
        Ok(())
    }
    fn reconcile(&mut self, retry: bool, gate: &Gate) -> Result<(), String> {
        let pending = self.journal.as_ref().and_then(|j| j.value.pending.clone());
        if let Some(
            Interrupted::Create { tool }
            | Interrupted::Daily { tool, .. }
            | Interrupted::Change { tool, .. },
        ) = &pending
        {
            if matches!(
                inspect_open(&tool.path, Some(&tool.identity)).map_err(error)?,
                OpenGate::UpgradeRequired(_)
            ) {
                // Launch inspects only. Neither an interrupted save nor a
                // previous upgrade request grants permission to migrate now.
                return self.open(tool.path.clone(), Some(tool.identity.clone()));
            }
        }
        match pending {
            None => self.reconcile_provider(),
            Some(Interrupted::Change {
                tool,
                basis: _,
                plan,
            }) => {
                let opened = open_verified(&tool.path, Some(&tool.identity)).map_err(error)?;
                if has_change_receipt(&opened.snapshot, &plan)? {
                    self.committed = Some(tool.path.clone());
                    self.journal(|j| {
                        j.pending = None;
                        j.last = Some(tool.clone());
                    })?;
                    self.notice =
                        "The exact saved choice was verified. It was not adopted twice".into();
                    self.install_opened(tool.clone(), opened)?;
                    Ok(())
                } else if retry {
                    // Opaque replay/adoption authority does not survive restart.
                    // A verified absence permits fresh work, never blind resend.
                    self.journal(|j| j.pending = None)?;
                    self.notice.clear();
                    self.open(tool.path.clone(), Some(tool.identity))?;
                    let outcome = "The interrupted change has no saved receipt. Your current work is kept; reopen the choice or request it again for a fresh rehearsal before accepting";
                    self.notice = if self.notice.is_empty() {
                        outcome.into()
                    } else {
                        format!("{outcome} {}", self.notice)
                    };
                    Ok(())
                } else {
                    self.notice="A change was interrupted before acknowledgement. Check its exact receipt before making another change; nothing is automatically adopted".into();
                    Ok(())
                }
            }
            Some(Interrupted::Create { tool }) => {
                let opened = open_verified(&tool.path, Some(&tool.identity)).map_err(|e| {
                    format!(
                        "The unfinished save at {} is still unresolved; it was not recreated: {e}",
                        tool.path.display()
                    )
                })?;
                self.committed = Some(tool.path.clone());
                self.opened = Some(OpenTool {
                    association: tool.clone(),
                    store: opened.store,
                    snapshot: opened.snapshot,
                });
                self.page = daily_page(
                    &tool,
                    &self.opened.as_ref().unwrap().store,
                    &self.opened.as_ref().unwrap().snapshot,
                )?;
                self.journal(|j| {
                    j.pending = None;
                    j.last = Some(tool.clone());
                })?;
                self.notice = "The exact previously saved tool was found and reopened; no duplicate was created".into();
                self.post_save(&tool);
                Ok(())
            }
            Some(Interrupted::Daily {
                tool,
                basis,
                operation,
                input,
            }) => {
                let mut opened = open_verified(&tool.path, Some(&tool.identity)).map_err(error)?;
                if !has_receipt(&opened.snapshot, &operation, &input)? {
                    if !retry {
                        self.notice = "An input was interrupted before a verified acknowledgement. It was not automatically resubmitted. Check or retry the exact unfinished save to continue".into();
                        return Ok(());
                    }
                    if Basis::capture(&opened.snapshot)? != basis {
                        return Err("The saved source, data, or session changed. The unfinished input cannot be repeated safely; it remains recorded for recovery".into());
                    }
                    gate.commit()?;
                    let result = opened.store.apply(
                        basis.revision,
                        &operation,
                        &input,
                        RuntimeLimits::default(),
                    );
                    opened = open_verified(&tool.path, Some(&tool.identity)).map_err(error)?;
                    if !has_receipt(&opened.snapshot, &operation, &input)? {
                        let failed = result.is_err();
                        let explanation =
                            format!("The exact retry did not commit: {:?}", result.err());
                        if failed && Basis::capture(&opened.snapshot)? == basis {
                            self.retain_unsaved(
                                tool.clone(),
                                operation,
                                input,
                                &opened.snapshot,
                                explanation.clone(),
                            )?;
                        }
                        return Err(explanation);
                    }
                }
                self.committed = Some(tool.path.clone());
                self.opened = Some(OpenTool {
                    association: tool.clone(),
                    store: opened.store,
                    snapshot: opened.snapshot,
                });
                self.page = daily_page(
                    &tool,
                    &self.opened.as_ref().unwrap().store,
                    &self.opened.as_ref().unwrap().snapshot,
                )?;
                self.journal(|j| {
                    j.pending = None;
                    j.last = Some(tool.clone());
                })?;
                self.notice =
                    "The exact saved operation was verified; no duplicate work was added".into();
                self.post_save(&tool);
                Ok(())
            }
        }
    }
    fn retain_unsaved(
        &mut self,
        tool: Association,
        operation: Id,
        input: SemanticInput,
        snapshot: &ProjectSnapshot,
        explanation: String,
    ) -> Result<(), String> {
        let summary = describe_input(&input, snapshot);
        let unsaved = UnsavedInput {
            tool,
            operation,
            input,
            summary,
            explanation: explanation.chars().take(MAX_TEXT_BYTES / 4).collect(),
        };
        self.journal(|j| {
            j.pending = None;
            j.last_unsaved = Some(unsaved);
        })
    }
    fn abandon_daily(&mut self, gate: &Gate) -> Result<(), String> {
        let Some(Interrupted::Daily {
            tool,
            operation,
            input,
            ..
        }) = self.journal.as_ref().and_then(|j| j.value.pending.clone())
        else {
            return Err("There is no unfinished daily input to set aside".into());
        };
        let opened = open_verified(&tool.path, Some(&tool.identity)).map_err(error)?;
        if has_receipt(&opened.snapshot, &operation, &input)? {
            return self.reconcile(false, gate);
        }
        self.before_abandon(gate);
        gate.commit()?;
        self.retain_unsaved(tool, operation, input, &opened.snapshot, "You explicitly set aside this input after verifying that the current saved tool has no matching commit".into())?;
        self.notice = "The uncommitted input was set aside and kept for reference. You can continue using your saved work".into();
        Ok(())
    }
    fn abandon_creation(&mut self, gate: &Gate) -> Result<(), String> {
        let Some(Interrupted::Create { tool }) =
            self.journal.as_ref().and_then(|j| j.value.pending.clone())
        else {
            return Err("There is no unfinished initial save to set aside".into());
        };
        // Keep every original byte. With the host lock held, no old host worker
        // can still be creating this tool. Existing folders share the store's
        // no-follow lock while checking that activation never occurred.
        let folder = match std::fs::symlink_metadata(&tool.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(error(e)),
            Ok(_) => Some(crate::product_locations::Folder::open(&tool.path).map_err(error)?),
        };
        let _lock = match &folder {
            Some(folder) => Some(folder.lock().map_err(error)?),
            None => None,
        };
        let parent = crate::product_locations::Folder::open(
            tool.path
                .parent()
                .ok_or("The unfinished folder has no parent")?,
        )
        .map_err(error)?;
        match std::fs::symlink_metadata(tool.path.join("CURRENT")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("The original activation status is unreadable; it was kept: {e}")),
            Ok(_) => return Err("The original folder contains an activation pointer. Check the saved result; it cannot be discarded as uncommitted".into()),
        }
        parent.check().map_err(error)?;
        if let Some(folder) = &folder {
            folder.check().map_err(error)?;
        }
        self.before_abandon(gate);
        gate.commit()?;
        self.journal(|j| {
            j.pending = None;
            j.abandoned_creation = Some(tool.clone());
        })?;
        self.notice = format!("The uncommitted save was set aside. Its original location and any partial files were kept at {}. You can explicitly save the draft again or start a new request", tool.path.display());
        Ok(())
    }
    fn current(&self, key: &Key) -> Result<&OpenTool, String> {
        let current = self.opened.as_ref().ok_or("Open the saved tool first")?;
        let fresh = current.store.load().map_err(error)?;
        if fresh != current.snapshot
            || key.basis.as_ref() != Some(&Basis::capture(&fresh)?)
            || ToolIdentity::from_snapshot(&fresh).map_err(error)? != current.association.identity
            || self.today() != fresh.clock_day
        {
            return Err("Saved work, its location, source or today's date changed. Return to saved work and prepare a fresh preview".into());
        }
        Ok(current)
    }
    fn history(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        let current = self.current(key)?;
        let view = intention_flow::HistoryView::load(
            &current.store,
            &current.snapshot,
            gate.cancelled.clone(),
        )?;
        let basis = Basis::capture(&current.snapshot)?;
        let tool = current.association.clone();
        if !gate.finish() {
            return Err("Opening history was cancelled; saved work was kept".into());
        }
        self.change = None;
        self.intention = None;
        self.page = Page::History { tool, basis, view };
        Ok(())
    }
    fn prepare_reconciliation(
        &mut self,
        needs: &[Id],
        need: &str,
        provider: ProviderKind,
        profile: CapabilityProfile,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        let current = self.current(key)?;
        if !matches!(self.page, Page::History { .. }) {
            return Err("Choose the exact saved needs from history first".into());
        }
        let engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(gate.cancelled.clone()),
            IntentArchive::new(current.store.clone()),
        );
        let binding = ReconcileAssociation {
            tool: current.association.clone(),
            basis: Basis::capture(&current.snapshot)?,
            needs: needs.to_vec(),
            evolution: id("evolution"),
            operation: id("adopt-design"),
        };
        let reconciliation = intention_flow::Reconciliation::new(
            &current.store,
            &current.snapshot,
            &engine,
            &id("request"),
            need,
            needs,
            &binding.evolution,
            &binding.operation,
            gate.cancelled.clone(),
        )?;
        let request = reconciliation.request().clone();
        self.prepare_request(
            need.into(),
            provider,
            profile,
            key,
            gate,
            request,
            None,
            Some(binding),
            Some(reconciliation),
        )
    }
    fn preview_withdrawal(
        &mut self,
        layers: &[Digest],
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        let current = self.current(key)?;
        if !matches!(self.page, Page::History { .. }) {
            return Err("Choose the exact rules from saved history first".into());
        }
        let engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(gate.cancelled.clone()),
            IntentArchive::new(current.store.clone()),
        );
        let withdrawal = intention_flow::Withdrawal::prepare(&current.store, &current.snapshot, &engine, layers, &key.operation, gate.cancelled.clone()).map_err(|e| format!("This rule cannot safely be withdrawn on current work. Saved work remains usable. Return to work, or request a compatible new design that keeps the independent promises. {e}"))?;
        if !gate.finish() {
            return Err("Withdrawal preview cancelled; saved work was kept".into());
        }
        self.intention = Some(IntentionDraft::Withdrawal(withdrawal));
        self.page = self.intention_page()?;
        Ok(())
    }
    fn intention_page(&self) -> Result<Page, String> {
        let basis = Basis::capture(
            &self
                .opened
                .as_ref()
                .ok_or("The saved tool was closed")?
                .snapshot,
        )?;
        let (view, origin) = match self
            .intention
            .as_ref()
            .ok_or("No checked preview is available")?
        {
            IntentionDraft::Design { design, .. } => {
                let view = design.view();
                let origin = match &view.authored.binding.producer {
                    Producer::Fixture { name } => format!("Synthetic transport fixture: {name}. This is not live AI generation."),
                    Producer::LiveAgent { provider, invocation_id, .. } => format!("Returned by {provider}, request {invocation_id}. Selected needs were independently executed locally."),
                    _ => return Err("Unexpected new-design provenance".into()),
                };
                (IntentionView::Design(view), origin)
            }
            IntentionDraft::Withdrawal(withdrawal) => (
                IntentionView::Withdrawal(withdrawal.view()),
                "Local checked withdrawal on copies of current saved work. No AI request is sent."
                    .into(),
            ),
        };
        Ok(Page::Intention {
            basis,
            view,
            origin,
        })
    }
    fn intention_trial(
        &mut self,
        input: SemanticInput,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        let store = self.current(key)?.store.clone();
        let finish = || {
            if gate.finish() {
                Ok(())
            } else {
                Err("Copied input cancelled; the prior copied work was kept".into())
            }
        };
        match self
            .intention
            .as_mut()
            .ok_or("No checked copied preview is available")?
        {
            IntentionDraft::Design { design, .. } => {
                design.trial_when(&store, input, gate.cancelled.clone(), finish)?;
            }
            IntentionDraft::Withdrawal(withdrawal) => {
                withdrawal.trial_when(&store, input, gate.cancelled.clone(), finish)?;
            }
        }
        self.page = self.intention_page()?;
        Ok(())
    }
    fn commit_intention(&mut self, operation: &str, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        let current = self.current(key)?;
        let basis = Basis::capture(&current.snapshot)?;
        let engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(gate.cancelled.clone()),
            IntentArchive::new(current.store.clone()),
        );
        let (change, notice) = match self
            .intention
            .as_ref()
            .ok_or("No checked preview is available")?
        {
            IntentionDraft::Design { design, evidence } => {
                evidence.check(design)?;
                if design.view().operation != operation {
                    return Err("This design belongs to another adoption operation".into());
                }
                (design.decision(&current.store, &engine, gate.cancelled.clone())?, "The checked new design was saved. Only the named old intentions were replaced; independent promises and later work were kept")
            }
            IntentionDraft::Withdrawal(withdrawal) => {
                if withdrawal.view().operation != operation {
                    return Err("This withdrawal belongs to another adoption operation".into());
                }
                (withdrawal.decision(&current.store, &engine, gate.cancelled.clone())?, "The selected rules were withdrawn on current work. Later records, edits, completed facts, events and earlier outputs were kept")
            }
        };
        self.commit_change(change, basis, gate, notice)
    }
    fn change_action(
        &mut self,
        key: &Key,
        gate: &Gate,
        action: impl FnOnce(&mut ChangeDraft, &ProductStore) -> Result<(), String>,
    ) -> Result<(), String> {
        self.no_pending()?;
        let store = &self
            .opened
            .as_ref()
            .ok_or("Open the saved tool first")?
            .store;
        let draft = self.change.as_ref().ok_or("No change is being tried")?;
        if key.basis.as_ref() != Some(&Basis::capture(&draft.snapshot)?)
            || self.today() != draft.snapshot.clock_day
        {
            return Err(
                "The comparison's source, data, session or date is stale. Return to saved work"
                    .into(),
            );
        }
        let mut next = draft.clone();
        action(&mut next, store)?;
        let page = Page::Change(next.checked_view(store, gate)?);
        if !gate.finish() {
            return Err("Trial cancelled; the previous copied experience was kept".into());
        }
        self.change = Some(next);
        self.page = page;
        Ok(())
    }
    fn pre_checkpoint(&self, tool: &Association, store: &ProductStore) -> Result<(), String> {
        let locations = self.locations.as_ref().ok_or("Tool location unavailable")?;
        let recent = RecentTools::open(locations.path()).map_err(error)?;
        let instance = recent.remember(&tool.path, unix_ms()).map_err(error)?;
        CheckpointShelf::for_tool(locations, &tool.identity, &instance)
            .map_err(error)?
            .capture(store)
            .map_err(error)?;
        Ok(())
    }
    fn decide(&mut self, outcome: DecisionOutcome, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        let opened = self.opened.as_ref().ok_or("Open the saved tool first")?;
        let draft = self
            .change
            .as_ref()
            .ok_or("No experienced change is available")?;
        let basis = Basis::capture(&draft.snapshot)?;
        if key.basis.as_ref() != Some(&basis) || self.today() != basis.day {
            return Err("This comparison is stale. Rehearse on current saved work".into());
        }
        let change = draft.decision(&opened.store, outcome, &key.operation, gate)?;
        self.commit_change(change, basis, gate, "Your choice was saved. Continue ordinary work; pending choices did not activate their alternatives")
    }
    fn commit_change(
        &mut self,
        change: crate::product_decisions::VerifiedChange,
        basis: Basis,
        gate: &Gate,
        notice: &str,
    ) -> Result<(), String> {
        self.no_pending()?;
        let opened = self.opened.as_ref().ok_or("Open the saved tool first")?;
        let fresh = opened.store.load().map_err(error)?;
        if Basis::capture(&fresh)? != basis
            || fresh != opened.snapshot
            || ToolIdentity::from_snapshot(&fresh).map_err(error)? != opened.association.identity
            || self.today() != basis.day
        {
            return Err("Saved work changed before this checked change could be saved".into());
        }
        let expected_day = basis.day;
        let plan = change.plan().clone();
        let tool = opened.association.clone();
        let store = opened.store.clone();
        self.pre_checkpoint(&tool, &store)?;
        self.journal(|j| {
            j.pending = Some(Interrupted::Change {
                tool: tool.clone(),
                basis,
                plan: plan.clone(),
            })
        })?;
        self.before_commit(gate);
        if self.today() != expected_day {
            self.journal(|j| j.pending = None)?;
            return Err("The date changed before saving. Return to saved work and rehearse a fresh comparison; no choice was committed".into());
        }
        if let Err(e) = gate.commit() {
            self.journal(|j| j.pending = None)?;
            return Err(e);
        }
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let result = engine.adopt(&store, &change);
        #[cfg(test)]
        let result = if result.is_ok() && self.config.hooks.lose_ack.swap(false, Ordering::AcqRel) {
            Err(crate::product_decisions::DecisionError::Invalid(
                "injected lost adoption acknowledgement".into(),
            ))
        } else {
            result
        };
        let checked = open_verified(&tool.path, Some(&tool.identity)).map_err(|e| {
            format!("The change outcome is unresolved. Keep its exact receipt association: {e}")
        })?;
        if has_change_receipt(&checked.snapshot, &plan)? {
            self.committed = Some(tool.path.clone());
            self.after_commit();
            self.journal(|j| {
                j.pending = None;
                j.last = Some(tool.clone());
            })?;
            self.notice = notice.into();
            self.install_opened(tool, checked)?;
            Ok(())
        } else {
            self.journal(|j| j.pending = None)?;
            Err(format!(
                "The change was not saved. Current work is kept. {}",
                result
                    .err()
                    .map(error)
                    .unwrap_or_else(|| "No matching adoption receipt".into())
            ))
        }
    }
    fn resume_choice(&mut self, decision: &str, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        let opened = self.opened.as_ref().ok_or("Open a saved tool first")?;
        let snapshot = opened.store.load().map_err(error)?;
        if key.basis.as_ref() != Some(&Basis::capture(&snapshot)?)
            || snapshot.clock_day != self.today()
        {
            return Err("Saved work changed. Reopen it before revisiting this choice".into());
        }
        let choice = snapshot
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == decision && d.status == DecisionStatus::Pending)
            .ok_or("This choice is no longer pending")?
            .clone();
        let matches: Vec<_> = snapshot
            .scope
            .rehearsals
            .values()
            .filter(|p| p.witnesses.get(decision) == Some(&choice.witness))
            .collect();
        let (candidate, request) = if matches.len() == 1 {
            let proof = matches[0];
            let candidate_id = proof
                .layer
                .as_ref()
                .map(|l| &l.candidate)
                .unwrap_or(&proof.manifest.business);
            let candidate = snapshot
                .programs
                .iter()
                .find(|p| {
                    canonical_digest(IdentityDomain::Source, *p).ok().as_ref() == Some(candidate_id)
                })
                .ok_or("The exact retained authored source is missing")?
                .clone();
            (candidate, proof.layer.as_ref().map(|l| l.request.clone()))
        } else if matches.is_empty() {
            // Public inheritance independently verifies each retained scene and
            // supplies its real source. Do not decode private archive objects.
            let engine = DecisionEngine::new(
                LocalRuntime::with_cancellation(gate.cancelled.clone()),
                IntentArchive::new(opened.store.clone()),
            );
            let inherited = engine
                .development_request(
                    &snapshot,
                    &id("revisit"),
                    DevelopmentOperation::Modify,
                    &choice.request,
                    DevelopmentContext {
                        view: Some(snapshot.session.view.clone()),
                        selected: vec![],
                        recent_inputs: vec![],
                        data_digest: Some(snapshot.data.identity().map_err(error)?),
                        session_digest: Some(snapshot.session.identity().map_err(error)?),
                    },
                )
                .map_err(error)?;
            let mut alternatives = vec![];
            for accepted in inherited.accepted_scenes.iter().filter(|s| {
                s.decision == decision
                    && s.source != snapshot.program().expect("verified source").artifact
            }) {
                let source = inherited
                    .sources
                    .iter()
                    .find(|s| s.artifact == accepted.source)
                    .ok_or("The archived choice source is missing")?;
                if !alternatives.contains(source) {
                    alternatives.push(source.clone());
                }
            }
            if alternatives.len() != 1
                || crate::product_runtime::has_protected_fields(&alternatives[0])
            {
                return Err("This saved choice has no unambiguous current-versus-one ordinary alternative. A fresh design request is needed; no source was substituted".into());
            }
            (alternatives.remove(0), None)
        } else {
            return Err(
                "The saved choice has ambiguous rehearsal proofs; no alternative was selected"
                    .into(),
            );
        };
        let mut draft = ChangeDraft::new(snapshot, candidate, choice.request)?;
        draft.resolves = vec![decision.into()];
        if let Some(request) = request {
            draft.population = request.population;
            draft.lifecycles = request
                .lifecycles
                .into_iter()
                .map(|mut l| {
                    l.source = draft.snapshot.active_revision.clone();
                    l
                })
                .collect();
        }
        let result = draft.prepare(&opened.store, gate);
        let page = Page::Change(draft.checked_view(&opened.store, gate)?);
        if !gate.finish() {
            return Err(
                "Opening the retained choice was cancelled; saved work is unchanged".into(),
            );
        }
        self.page = page;
        self.change = Some(draft);
        self.notice="This is a fresh comparison on your current saved work. The earlier experiences remain unchanged in history; try the alternatives again before resolving".into();
        result
    }
    fn close(&mut self) -> Result<(), String> {
        if self.ready.is_some() {
            self.journal(|j| {
                if j.provider.as_ref().is_some_and(|p| !p.issued) {
                    j.provider = None;
                }
            })?;
        }
        self.ready = None;
        self.draft = None;
        self.change = None;
        self.intention = None;
        self.recent_inputs.clear();
        self.opened = None;
        self.page = Page::Home;
        self.chosen = None;
        Ok(())
    }
    fn pick_folder(&self, _title: &str) -> Option<PathBuf> {
        #[cfg(test)]
        {
            self.config.hooks.folder_choice.clone()
        }
        #[cfg(not(test))]
        {
            rfd::FileDialog::new().set_title(_title).pick_folder()
        }
    }
    fn before_abandon(&self, _gate: &Gate) {
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_abandon {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !_gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn before_commit(&self, _gate: &Gate) {
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_commit {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !_gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn after_commit(&self) {
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.after_commit {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn handle(&mut self, command: &Command) -> Result<(), String> {
        let key = &command.key;
        let gate = &command.gate;
        gate.check()?;
        #[cfg(test)]
        if matches!(
            &command.action,
            Action::Decide { .. }
                | Action::Modify { .. }
                | Action::ResumeChoice { .. }
                | Action::History
                | Action::PrepareReconciliation { .. }
                | Action::PreviewWithdrawal { .. }
                | Action::CommitIntention { .. }
        ) {
            if let Some(pause) = &self.config.hooks.before_context_transition {
                pause.reached.store(true, Ordering::Release);
                while !pause.release.load(Ordering::Acquire)
                    && !gate.cancelled.load(Ordering::Acquire)
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                gate.check()?;
            }
        }
        match &command.action {
            Action::Boot => self.boot(),
            Action::Prepare {
                need,
                provider,
                profile,
            } => self.prepare(need.clone(), *provider, *profile, key, gate, false, None),
            Action::Modify {
                need,
                provider,
                profile,
                focused,
            } => self.prepare(
                need.clone(),
                *provider,
                *profile,
                key,
                gate,
                true,
                focused.clone(),
            ),
            Action::Trial { input } => {
                #[cfg(test)]
                if let Some(pause) = &self.config.hooks.before_preview {
                    pause.reached.store(true, Ordering::Release);
                    while !pause.release.load(Ordering::Acquire)
                        && !gate.cancelled.load(Ordering::Acquire)
                    {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                self.change_action(key, gate, |draft, store| {
                    draft.trial(store, input.clone(), gate)
                })
            }
            Action::Scope { population } => self.change_action(key, gate, |draft, store| {
                draft.set_scope(store, population.clone(), gate)
            }),
            Action::Finished { record, field } => self.change_action(key, gate, |draft, store| {
                draft.clarify(store, record, field, gate)
            }),
            Action::NoFinished { entity } => self.change_action(key, gate, |draft, store| {
                draft.no_finished_state(store, entity, gate)
            }),
            Action::Decide { outcome } => self.decide(outcome.clone(), key, gate),
            Action::ResumeChoice { decision } => self.resume_choice(decision, key, gate),
            Action::History => self.history(key, gate),
            Action::PrepareReconciliation {
                needs,
                need,
                provider,
                profile,
            } => self.prepare_reconciliation(needs, need, *provider, *profile, key, gate),
            Action::PreviewWithdrawal { layers } => self.preview_withdrawal(layers, key, gate),
            Action::CommitIntention { operation } => self.commit_intention(operation, key, gate),
            Action::IntentionTrial { input } => {
                #[cfg(test)]
                if let Some(pause) = &self.config.hooks.before_preview {
                    pause.reached.store(true, Ordering::Release);
                    while !pause.release.load(Ordering::Acquire)
                        && !gate.cancelled.load(Ordering::Acquire)
                    {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                self.intention_trial(input.clone(), key, gate)
            }
            Action::ReturnDaily => self.return_daily(gate),
            Action::Consent { disclosure } => {
                let result = self.generate(disclosure.clone(), key, gate);
                if result.is_err()
                    && self.ready.is_none()
                    && matches!(&self.page, Page::Consent { request, .. } if request.operation == DevelopmentOperation::Reconcile)
                {
                    if let Some(opened) = &self.opened {
                        if let Ok(page) =
                            daily_page(&opened.association, &opened.store, &opened.snapshot)
                        {
                            self.page = page;
                        }
                    }
                }
                result
            }
            Action::Select { candidate } => self.select(*candidate, key, gate),
            Action::Preview { input } => self.preview(input.clone(), key, gate),
            Action::Save => self.save(key, gate),
            Action::Daily { input } => self.daily(input.clone(), key, gate),
            Action::Open { path, expected } => self.open(path.clone(), expected.clone()),
            Action::Upgrade { tool, summary } => self.upgrade(tool, summary, gate),
            Action::OpenDialog => match self.pick_folder("Open a saved generated-tool folder") {
                Some(path) => {
                    gate.check()?;
                    self.open(path, None)
                }
                None => {
                    self.notice = "Folder selection cancelled; no tool was opened".into();
                    Ok(())
                }
            },
            Action::ChooseDestination => {
                #[cfg(test)]
                if let Some(pause) = &self.config.hooks.before_destination {
                    pause.reached.store(true, Ordering::Release);
                    while !pause.release.load(Ordering::Acquire)
                        && !gate.cancelled.load(Ordering::Acquire)
                    {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                gate.check()?;
                match self.pick_folder("Choose where to keep this tool's data") {
                    Some(path) => {
                        ToolLocations::chosen(&path).map_err(error)?;
                        if !gate.finish() {
                            return Err("Folder selection cancelled; the draft and previous destination were kept".into());
                        }
                        self.chosen = Some(path);
                        Ok(())
                    }
                    None => {
                        self.notice = "Folder selection cancelled; nothing was saved and no other destination was selected".into();
                        Ok(())
                    }
                }
            }
            Action::Reconcile => self.reconcile(true, gate),
            Action::AbandonCreation => self.abandon_creation(gate),
            Action::AbandonDaily => self.abandon_daily(gate),
            Action::Tick { today } => {
                if *today != self.today() {
                    return Err("The device date changed again; refresh the current view".into());
                }
                let basis = key.basis.as_ref().ok_or("No current tool clock")?;
                let days = u32::try_from(i64::from(*today) - i64::from(basis.day))
                    .map_err(|_| "A saved clock cannot be moved backwards automatically")?;
                if days == 0 {
                    return Ok(());
                }
                self.daily(SemanticInput::AdvanceClock { days }, key, gate)
            }
            Action::Close => self.close(),
            #[cfg(test)]
            Action::DialogCancelled => {
                self.notice = "Folder selection cancelled; no tool was opened".into();
                Ok(())
            }
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Release I/O locks and provider state before signalling test shutdown.
        self.ready = None;
        self.journal = None;
        #[cfg(test)]
        self.config.hooks.stopped.store(true, Ordering::Release);
    }
}
pub(super) fn run(
    config: Config,
    commands: mpsc::Receiver<Command>,
    complete: mpsc::SyncSender<Completion>,
) {
    let mut host = Worker {
        config,
        locations: None,
        journal: None,
        page: Page::Home,
        ready: None,
        draft: None,
        change: None,
        intention: None,
        recent_inputs: vec![],
        opened: None,
        chosen: None,
        generation_blocked: false,
        notice: String::new(),
        committed: None,
    };
    let mut session = None;
    let mut epoch = 0;
    let mut last_operation = None;
    while let Ok(command) = commands.recv() {
        if session.as_ref().is_some_and(|s| s != &command.key.session)
            || command.key.epoch < epoch
            || last_operation.as_ref() == Some(&command.key.operation)
        {
            continue;
        }
        session = Some(command.key.session.clone());
        epoch = command.key.epoch;
        last_operation = Some(command.key.operation.clone());
        host.committed = None;
        if !matches!(command.action, Action::Close) {
            host.notice.clear();
        }
        let mut result = host.handle(&command);
        if !command.gate.finish() && host.committed.is_none() {
            // Only unfinished generation/opening is cleared on cancellation.
            // Cancelling an edit, save, or recovery operation preserves its
            // previously visible tool/draft and unresolved associations.
            if matches!(
                command.action,
                Action::Prepare { .. }
                    | Action::Modify { .. }
                    | Action::PrepareReconciliation { .. }
                    | Action::Consent { .. }
                    | Action::Open { .. }
                    | Action::OpenDialog
            ) {
                if let Err(e) = host.close() {
                    host.notice = e;
                }
            }
            result = Err(
                "Cancelled; no business change was committed. Your original need was kept".into(),
            );
        }
        if let Err(e) = &result {
            host.notice = match &host.committed {
                Some(path) => format!("Your work was saved at {}, but finishing the view or restart record failed: {e}. Do not repeat the operation", path.display()),
                None => e.clone(),
            };
        }
        let mut update = host.update();
        if matches!(command.action, Action::Boot) {
            update.need = host.journal.as_ref().map(|j| j.value.need.clone());
        }
        let completion = Completion {
            key: command.key,
            update,
            acknowledged: result.is_ok(),
            committed: host.committed.take(),
        };
        #[cfg(test)]
        let duplicate = host
            .config
            .hooks
            .duplicate_completion
            .then(|| completion.clone());
        if complete.send(completion).is_err() {
            break;
        }
        #[cfg(test)]
        if let Some(duplicate) = duplicate {
            if complete.send(duplicate).is_err() {
                break;
            }
        }
    }
}
fn empty_run(
    capture: &CapturedProgram,
    day: i32,
    runtime: &LocalRuntime,
) -> Result<ProductRun, String> {
    runtime
        .start(
            capture,
            &DataSnapshot::empty(&capture.binding.project_id, &capture.program).map_err(error)?,
            &SessionState::initial(&capture.program).map_err(error)?,
            day,
            0,
            RuntimeLimits::default(),
        )
        .map_err(error)
}
fn daily_page(
    tool: &Association,
    store: &ProductStore,
    snapshot: &ProjectSnapshot,
) -> Result<Page, String> {
    let model = store.runtime_view().map_err(error)?;
    // runtime_view performs its own verified load. Do not pair a newer view
    // with an older request basis if another process saves during rendering.
    if store.load().map_err(error)? != *snapshot {
        return Err(
            "The saved tool changed while opening its view; reopen the current work".into(),
        );
    }
    Ok(Page::Daily {
        tool: tool.clone(),
        basis: Basis::capture(snapshot)?,
        model,
        choices: snapshot.decisions.decisions.clone(),
    })
}
fn has_change_receipt(snapshot: &ProjectSnapshot, plan: &AdoptionPlan) -> Result<bool, String> {
    let found = snapshot
        .adoptions
        .iter()
        .find(|receipt| receipt.plan.id == plan.id);
    match found {
        Some(receipt) if &receipt.plan == plan => Ok(true),
        Some(_) => Err(
            "The change operation ID belongs to a different saved plan; it will not be retried"
                .into(),
        ),
        None => Ok(false),
    }
}
fn has_receipt(
    snapshot: &ProjectSnapshot,
    operation: &str,
    input: &SemanticInput,
) -> Result<bool, String> {
    let Some(receipt) = snapshot.operations.get(operation) else {
        return Ok(false);
    };
    if receipt.operation != operation
        || receipt.request != canonical_digest(IdentityDomain::Input, input).map_err(error)?
    {
        return Err(
            "The saved operation ID belongs to a different input; it will not be repeated".into(),
        );
    }
    Ok(true)
}
/// Host allowlist only. Neither model output nor a generated tool chooses a
/// command. Ignore relative PATH entries and resolve the selected endpoint once.
fn find_endpoint(name: &str) -> Option<PathBuf> {
    if !matches!(name, "codex" | "claude") {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
        let candidate = directory.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        });
        let Ok(metadata) = std::fs::metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        if let Ok(canonical) = std::fs::canonicalize(candidate) {
            return Some(canonical);
        }
    }
    None
}

fn describe_input(input: &SemanticInput, snapshot: &ProjectSnapshot) -> String {
    use crate::ui::product_runtime_view::value_text;
    let program = snapshot.program().ok().map(|c| &c.program);
    let view_label = |id: &str| {
        program
            .and_then(|p| p.views.iter().find(|v| v.id == id))
            .map(|v| v.label.as_str())
            .unwrap_or(id)
            .to_string()
    };
    let summary = match input {
        SemanticInput::Submit { view, arguments } => {
            let fields = program
                .and_then(|p| p.views.iter().find(|v| &v.id == view))
                .and_then(|v| match &v.kind {
                    ViewKind::Form { fields, .. } => Some(fields),
                    _ => None,
                });
            let values = arguments
                .iter()
                .map(|(id, value)| {
                    format!(
                        "{}: {}",
                        fields
                            .and_then(|f| f.iter().find(|f| &f.parameter == id))
                            .map(|f| f.label.as_str())
                            .unwrap_or(id),
                        value_text(value)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("{}: {values}", view_label(view))
        }
        SemanticInput::Invoke { action, arguments } => format!(
            "{}: {}",
            program
                .and_then(|p| p.actions.iter().find(|a| &a.id == action))
                .map(|a| a.label.as_str())
                .unwrap_or(action),
            arguments
                .iter()
                .map(|(name, value)| format!("{name}: {}", value_text(value)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        SemanticInput::Control {
            view,
            control,
            value,
        } => format!("{} / {control}: {}", view_label(view), value_text(value)),
        SemanticInput::Activate { view, binding, row } => format!(
            "{} / {}{}",
            view_label(view),
            program
                .and_then(|p| p.views.iter().find(|v| &v.id == view))
                .and_then(|v| v.actions.iter().find(|a| &a.id == binding))
                .map(|a| a.label.as_str())
                .unwrap_or(binding),
            row.as_ref()
                .map(|r| format!(" ({})", r.record))
                .unwrap_or_default()
        ),
        SemanticInput::Navigate { view } => format!("Open {}", view_label(view)),
        SemanticInput::AdvanceClock { days } => format!("Advance the saved date by {days} days"),
        SemanticInput::Observe { point } => format!("Observe {point}"),
    };
    summary.chars().take(MAX_TEXT_BYTES / 4).collect()
}
