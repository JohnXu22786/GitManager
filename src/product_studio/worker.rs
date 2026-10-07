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
                Some(Interrupted::Create { .. } | Interrupted::Daily { .. })
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
        if self.opened.is_none() {
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
        let request = DevelopmentRequest {
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
        };
        request.validate().map_err(error)?;
        self.journal(|j| j.need = need)?;
        self.ready = None;
        self.draft = None;
        self.opened = None;
        self.page = Page::Home;
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
        };
        self.journal(|j| j.provider = Some(pending))?;
        self.page = Page::Consent {
            disclosure: prepared.disclosure().clone(),
            need: request.request.clone(),
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
        });
        Ok(())
    }
    fn generate(&mut self, disclosure: String, key: &Key, gate: &Gate) -> Result<(), String> {
        let ready = self
            .ready
            .as_ref()
            .ok_or("Prepare and review a fresh request before generating")?;
        if ready.epoch != key.epoch || ready.prepared.disclosure().digest() != disclosure {
            return Err("The prepared request changed; review it again before authorizing".into());
        }
        gate.check()?;
        self.journal(|j| {
            if let Some(ProviderAssociation { issued, .. }) = &mut j.provider {
                *issued = true;
            }
        })?;
        let ready = self.ready.take().unwrap();
        let consent = ConsentReceipt {
            disclosure_digest: disclosure,
            approval_reference: format!("studio-click-{}", key.operation),
            expires_at_unix_ms: unix_ms().saturating_add(5 * 60_000),
        };
        let provider = ready.prepared.authorize(consent);
        let result = provider.develop(&ready.request, &|| gate.cancelled.load(Ordering::Acquire));
        let receipt = provider.receipt();
        let raw = provider.raw_response();
        self.page = Page::Home;
        // Dropping the provider/job and any cancellation join stays on this worker.
        drop(provider);
        let terminal = ready.transport.reconcile(&ready.request.id);
        match &terminal {
            Ok(receipt)
                if receipt.state.is_terminal() && receipt.state != JobState::Interrupted =>
            {
                self.journal(|j| j.provider = None)?
            }
            Ok(receipt) if receipt.state == JobState::Prepared => {
                self.journal(|j| j.provider = None)?
            }
            _ => self.generation_blocked = true,
        }
        let result = result.map_err(error)?;
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
        let opened = open_verified(&path, expected.as_ref()).map_err(error)?;
        let association = Association {
            path,
            identity: ToolIdentity::from_snapshot(&opened.snapshot).map_err(error)?,
        };
        let page = daily_page(&association, &opened.snapshot)?;
        self.opened = Some(OpenTool {
            association: association.clone(),
            store: opened.store,
            snapshot: opened.snapshot,
        });
        self.draft = None;
        self.ready = None;
        self.page = page;
        self.journal(|j| j.last = Some(association.clone()))?;
        self.post_save(&association);
        Ok(())
    }
    fn post_save(&mut self, tool: &Association) {
        let result = (|| -> Result<(), String> {
            let locations = self
                .locations
                .as_ref()
                .ok_or("Default tool location unavailable")?;
            let recent = RecentTools::open(locations.path()).map_err(error)?;
            let instance = recent.remember(&tool.path, unix_ms()).map_err(error)?;
            let store = &self.opened.as_ref().ok_or("Saved tool is not open")?.store;
            CheckpointShelf::for_tool(locations, &tool.identity, &instance)
                .map_err(error)?
                .capture(store)
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
                self.page = daily_page(&association, &self.opened.as_ref().unwrap().snapshot)?;
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
        let checked = open_verified(&association.path, Some(&association.identity)).map_err(|e| format!("The save outcome is unresolved: {e}. Keep this operation for exact reconciliation"))?;
        if has_receipt(&checked.snapshot, &operation, &input)? {
            self.committed = Some(association.path.clone());
            self.after_commit();
            self.opened = Some(OpenTool {
                association: association.clone(),
                store: checked.store,
                snapshot: checked.snapshot,
            });
            self.page = daily_page(&association, &self.opened.as_ref().unwrap().snapshot)?;
            self.journal(|j| {
                j.pending = None;
                j.last = Some(association.clone());
            })?;
            self.notice = "Your work was saved".into();
            self.post_save(&association);
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
        match pending {
            None => self.reconcile_provider(),
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
                self.page = daily_page(&tool, &self.opened.as_ref().unwrap().snapshot)?;
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
                        return Err(format!(
                            "The exact retry did not commit: {:?}",
                            result.err()
                        ));
                    }
                }
                self.committed = Some(tool.path.clone());
                self.opened = Some(OpenTool {
                    association: tool.clone(),
                    store: opened.store,
                    snapshot: opened.snapshot,
                });
                self.page = daily_page(&tool, &self.opened.as_ref().unwrap().snapshot)?;
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
        gate.commit()?;
        self.journal(|j| {
            j.pending = None;
            j.abandoned_creation = Some(tool.clone());
        })?;
        self.notice = format!("The uncommitted save was set aside. Its original location and any partial files were kept at {}. You can explicitly save the draft again or start a new request", tool.path.display());
        Ok(())
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
        match &command.action {
            Action::Boot => self.boot(),
            Action::Prepare {
                need,
                provider,
                profile,
            } => self.prepare(need.clone(), *provider, *profile, key, gate),
            Action::Consent { disclosure } => self.generate(disclosure.clone(), key, gate),
            Action::Select { candidate } => self.select(*candidate, key, gate),
            Action::Preview { input } => self.preview(input.clone(), key, gate),
            Action::Save => self.save(key, gate),
            Action::Daily { input } => self.daily(input.clone(), key, gate),
            Action::Open { path, expected } => self.open(path.clone(), expected.clone()),
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
                match self.pick_folder("Choose where to keep this tool's data") {
                    Some(path) => {
                        gate.check()?;
                        ToolLocations::chosen(&path).map_err(error)?;
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
            if !matches!(
                command.action,
                Action::Daily { .. }
                    | Action::Tick { .. }
                    | Action::Reconcile
                    | Action::Save
                    | Action::Preview { .. }
                    | Action::Select { .. }
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
fn daily_page(tool: &Association, snapshot: &ProjectSnapshot) -> Result<Page, String> {
    let runtime = LocalRuntime::default();
    let run = runtime
        .resume(
            snapshot.program().map_err(error)?,
            &snapshot.data,
            &snapshot.session,
            snapshot.clock_day,
            0,
            RuntimeLimits::default(),
            &snapshot.artifacts,
        )
        .map_err(error)?;
    Ok(Page::Daily {
        tool: tool.clone(),
        basis: Basis::capture(snapshot)?,
        model: runtime.view_model(&run).map_err(error)?,
    })
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
