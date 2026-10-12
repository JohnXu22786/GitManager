//! Actual Discover requests, checked queues and copied experiences on the sole worker.
use super::super::{discovery_flow as whole, discovery_host as host, rule_discovery as rule};
use super::*;

enum Draft {
    Whole(whole::DiscoveryDraft),
    Rule(rule::RuleDiscoveryDraft),
}
enum Experience {
    WholeExample(whole::ExampleExperience),
    RuleExample(rule::RuleExample),
    Pair(whole::PairExperience),
}
enum Queue {
    Whole(whole::DiscoveryQueue),
    Rule(rule::RuleDiscoveryQueue),
}
pub(super) struct State {
    draft: Option<Draft>,
    queue: Option<Queue>,
    view: Option<host::View>,
    source_operation: Option<Id>,
    experience: Option<Experience>,
    opened_example: Option<(Id, usize, bool)>,
    resolves: Vec<Id>,
}
impl State {
    pub(super) fn report_only(self) -> Option<Self> {
        Some(Self {
            draft: None,
            queue: None,
            view: Some(self.view?),
            source_operation: None,
            experience: None,
            opened_example: None,
            resolves: vec![],
        })
    }
}
impl Worker {
    pub(super) fn decorate_change(&self, view: &mut ChangeView) -> Result<(), String> {
        if let Some(record) = self
            .journal
            .as_ref()
            .and_then(|j| j.value.discovery.as_ref())
            .filter(|r| r.basis == view.basis && r.need == view.need)
        {
            if let Ok(result) = record
                .provider_original()
                .and_then(host::Invocation::result)
            {
                view.candidate_choices = result
                    .response
                    .candidates
                    .iter()
                    .map(|c| {
                        let source = CapturedProgram::capture(
                            c.source_json.as_bytes(),
                            &record.tool.identity.project_id,
                            result.producer.clone(),
                            None,
                        )
                        .map_err(error)?;
                        Ok((c.id.clone(), source.program.label))
                    })
                    .collect::<Result<_, String>>()?;
                view.selected_candidate = record.selected.clone();
            }
        }
        Ok(())
    }
    fn change_from_record(
        &self,
        record: &host::Record,
        gate: &Gate,
    ) -> Result<ChangeDraft, String> {
        record.validate()?;
        let current = self.opened.as_ref().ok_or("Open the saved tool first")?;
        if current.association != record.tool
            || Basis::capture(&current.store.load().map_err(error)?)? != record.basis
            || self.today() != record.basis.day
        {
            return Err("This original change is stale. Its exact evidence is retained, but it cannot authorize today's work".into());
        }
        host::verify_original_need(
            &current.store,
            &current.snapshot,
            &record.original_request()?,
            &record.need,
            record.external.is_some(),
            gate,
        )?;
        let candidate = record.candidate()?;
        let mut change =
            ChangeDraft::new(current.snapshot.clone(), candidate, record.need.clone())?;
        change.request = Some(record.original_request()?);
        if let Some(original) = &record.original {
            change.result = Some(original.result()?);
            change.raw = Some(
                original
                    .completed
                    .as_ref()
                    .unwrap()
                    .raw
                    .to_bytes(original.result_limit)
                    .map_err(error)?,
            );
            change.receipt = Some(original.completed.as_ref().unwrap().receipt.clone());
        }
        if let Some(selection) = &record.selection {
            change.restore_selection(
                selection.operation.clone(),
                selection.population.clone(),
                selection.lifecycles.clone(),
                selection.operations.clone(),
            );
        }
        change.prepare_for_discovery(&current.store, gate)?;
        Ok(change)
    }
    pub(super) fn select_modify(
        &mut self,
        candidate: &str,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        self.current(key)?;
        if self.ready.is_some()
            || self
                .journal
                .as_ref()
                .is_some_and(|j| j.value.provider.is_some())
        {
            return Err("Finish or decline the displayed provider request before selecting a different design".into());
        }
        let mut record = self
            .journal
            .as_ref()
            .and_then(|j| j.value.discovery.clone())
            .ok_or("No completed original Modify result is available")?;
        if !record
            .provider_original()
            .and_then(host::Invocation::result)?
            .response
            .candidates
            .iter()
            .any(|c| c.id == candidate)
        {
            return Err("Choose an actual returned candidate".into());
        }
        record.selected = Some(candidate.into());
        record.selection = None;
        record.discover = None;
        record.accepted_case = None;
        let change = self.change_from_record(&record, gate)?;
        record.selection = Some(host::Selection {
            operation: change.operation.clone(),
            population: change.population.clone(),
            lifecycles: change.lifecycles.clone(),
            operations: change.scope_operations().clone(),
        });
        let store = self.opened.as_ref().unwrap().store.clone();
        let mut view = change.checked_view(&store, gate)?;
        self.current(key)?;
        gate.check()?;
        self.journal(|j| j.discovery = Some(record))?;
        self.decorate_change(&mut view)?;
        self.change = Some(change);
        self.discovery = None;
        self.page = Page::Change(view);
        Ok(())
    }
    pub(super) fn reopen_discovery(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        let current = self.current(key)?;
        let record = self
            .journal
            .as_ref()
            .and_then(|j| j.value.discovery.clone())
            .ok_or("No exact completed change is retained for this saved tool")?;
        record.validate()?;
        if record.tool != current.association {
            return Err("The retained change belongs to another saved tool".into());
        }
        if record.basis != Basis::capture(&current.snapshot)? || self.today() != record.basis.day {
            let lineage = record.identity()?;
            let identities = record.evidence_text()?;
            let mut view = self.cached_discovery_report.as_ref().filter(|cached| cached.tool == record.tool && cached.view.lineage == lineage).map(|cached| cached.view.clone()).unwrap_or_else(|| host::View {
                lineage,
                basis: record.basis.clone(), queue: whole::QueueView { state: whole::QueueState::Unverified, questions: vec![], defects: vec![], unverified: vec!["No complete report for this exact change is cached in this host session. Exact requests, raw responses and selected source remain retained; current verification is required".into()], coverage: vec![] },
                report: None,
                identities, stale: true,
            });
            view.basis = key.basis.clone().unwrap();
            view.stale = true;
            self.page = Page::Discovery(view);
            return Ok(());
        }
        let change = self.change_from_record(&record, gate)?;
        let store = current.store.clone();
        let source_operation = if record.external.is_some() {
            let evidence = self
                .retained_task_evidence(&record.candidate()?, gate)?
                .ok_or("The original external task source is no longer available")?;
            self.task.evidence = Some((change.operation.clone(), evidence));
            Some(change.operation.clone())
        } else {
            None
        };
        let Some(invocation) = record.discover.as_ref().filter(|i| i.completed.is_some()) else {
            let mut view = change.checked_view(&store, gate)?;
            self.decorate_change(&mut view)?;
            self.change = Some(change);
            self.page = Page::Change(view);
            self.notice = "The exact original change was reopened locally. No request was resubmitted and no earlier consent was renewed".into();
            return Ok(());
        };
        let request = invocation.request()?;
        let draft = self.discovery_draft(&record, &change, &request.id, &store, gate)?;
        let rebuilt = match &draft {
            Draft::Whole(d) => d.request(),
            Draft::Rule(d) => d.request(),
        };
        if rebuilt != &request {
            return Err("The retained Discover request cannot be reproduced from the exact original selection".into());
        }
        let result = invocation.result()?;
        self.change = Some(change);
        self.discovery = Some(State {
            draft: Some(draft),
            queue: None,
            view: None,
            source_operation,
            experience: None,
            opened_example: None,
            resolves: vec![],
        });
        self.evaluate_discovery(result, record, key, gate)
    }
    fn discovery_draft(
        &self,
        record: &host::Record,
        change: &ChangeDraft,
        request_id: &str,
        store: &ProductStore,
        gate: &Gate,
    ) -> Result<Draft, String> {
        let original = record.original_request()?;
        host::verify_original_need(
            store,
            &change.snapshot,
            &original,
            &record.need,
            record.external.is_some(),
            gate,
        )?;
        let selection = change.rule_selection(store, gate)?;
        let draft: Result<Draft, String> = match (
            record.original.as_ref(),
            record.external.as_ref(),
            selection,
        ) {
            (Some(provider), None, Some(selection)) => {
                Ok(Draft::Rule(rule::RuleDiscoveryDraft::after_modify(
                    store,
                    &change.snapshot,
                    original,
                    provider.result()?,
                    record.selected.as_ref().ok_or("Choose an actual design")?,
                    selection,
                    request_id,
                    &record.need,
                    gate.cancelled.clone(),
                )?))
            }
            (Some(provider), None, None) => Ok(Draft::Whole(
                whole::DiscoveryDraft::after_modify(
                    store,
                    &change.snapshot,
                    original,
                    provider.result()?,
                    record.selected.as_ref().ok_or("Choose an actual design")?,
                    change.prepared.clone(),
                    request_id,
                    &record.need,
                    gate.cancelled.clone(),
                )
                .map_err(error)?,
            )),
            (None, Some(_), Some(selection)) => {
                Ok(Draft::Rule(rule::RuleDiscoveryDraft::after_external(
                    store,
                    &change.snapshot,
                    original,
                    record.candidate()?,
                    selection,
                    request_id,
                    &record.need,
                    gate.cancelled.clone(),
                )?))
            }
            (None, Some(_), None) => Ok(Draft::Whole(
                whole::DiscoveryDraft::after_external(
                    store,
                    &change.snapshot,
                    original,
                    record.candidate()?,
                    change.prepared.clone(),
                    request_id,
                    &record.need,
                    gate.cancelled.clone(),
                )
                .map_err(error)?,
            )),
            _ => Err("The original change has no single checked origin".into()),
        };
        let draft = draft?;
        if let Some(case) = &record.accepted_case {
            let (_, example) = case.check(change, record, store, gate)?;
            match draft {
                Draft::Whole(d) => Ok(Draft::Whole(d.with_selected_example(example)?)),
                Draft::Rule(d) => Ok(Draft::Rule(d.with_selected_example(example)?)),
            }
        } else {
            Ok(draft)
        }
    }
    fn feature_fresh(&self, key: &Key, gate: &Gate) -> Result<host::Record, String> {
        let current = self.current(key)?;
        gate.check()?;
        let change = self
            .change
            .as_ref()
            .ok_or("Open the actual original change first")?;
        let record = self
            .journal
            .as_ref()
            .and_then(|j| j.value.discovery.clone())
            .ok_or("The original human need and exact change context are unavailable")?;
        record.validate()?;
        if record.tool != current.association
            || record.basis != Basis::capture(&change.snapshot)?
            || record.need != change.need
            || record.candidate()? != change.candidate
        {
            return Err(
                "The candidate example belongs to an older source, need or saved work".into(),
            );
        }
        host::verify_original_need(
            &current.store,
            &change.snapshot,
            &record.original_request()?,
            &record.need,
            record.external.is_some(),
            gate,
        )?;
        if record.external.is_some() {
            if !self.has_task_evidence(&change.operation) {
                return Err("The exact original task source must be checked again".into());
            }
            self.check_task_key(key)?;
            self.check_task_evidence(&change.operation, gate)?;
        }
        Ok(record)
    }
    pub(super) fn feature_example(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        if self.ready.is_some()
            || self
                .journal
                .as_ref()
                .is_some_and(|j| j.value.provider.is_some())
        {
            return Err("Finish the exact provider request before selecting a feature case".into());
        }
        let record = self.feature_fresh(key, gate)?;
        let change = self.change.as_ref().unwrap();
        let store = &self.opened.as_ref().unwrap().store;
        let mut trial = discovery_policy::Trial::open(
            change,
            &record,
            store,
            record.accepted_case.as_ref().map(|c| &c.scenario),
            gate,
        )?;
        if let Some(case) = &record.accepted_case {
            case.check(change, &record, store, gate)?;
            trial.accepted = true;
        }
        self.feature_fresh(key, gate)?;
        if !gate.finish() {
            return Err("The candidate example was cancelled".into());
        }
        self.page = Page::DiscoveryRequirement(trial.view()?);
        self.feature = Some(trial);
        Ok(())
    }
    pub(super) fn feature_trial(
        &mut self,
        input: SemanticInput,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        let mut record = self.feature_fresh(key, gate)?;
        let mut scenario = self
            .feature
            .as_ref()
            .ok_or("Open an actual candidate example first")?
            .scenario
            .clone();
        // Revoke the actual published case before a failed changed replay.
        if let Page::DiscoveryRequirement(view) = &mut self.page {
            view.can_accept = false;
            view.accepted = false;
        }
        self.feature = None;
        record.accepted_case = None;
        record.discover = None;
        self.journal(|j| j.discovery = Some(record.clone()))?;
        self.discovery = None;
        scenario.inputs.push(input);
        let trial = discovery_policy::Trial::open(
            self.change.as_ref().unwrap(),
            &record,
            &self.opened.as_ref().unwrap().store,
            Some(&scenario),
            gate,
        )?;
        self.feature_fresh(key, gate)?;
        if !gate.finish() {
            return Err(
                "The changed copied result was cancelled; its old acceptance was revoked".into(),
            );
        }
        self.page = Page::DiscoveryRequirement(trial.view()?);
        self.feature = Some(trial);
        Ok(())
    }
    pub(super) fn feature_accept(
        &mut self,
        ticket: &Digest,
        offered: &str,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        let mut record = self.feature_fresh(key, gate)?;
        let case = self
            .feature
            .as_ref()
            .ok_or("Try an actual candidate result first")?
            .accept(ticket, offered, &key.operation)?;
        let change = self.change.as_ref().unwrap();
        case.check(change, &record, &self.opened.as_ref().unwrap().store, gate)?;
        record.selection = Some(host::Selection {
            operation: change.operation.clone(),
            population: change.population.clone(),
            lifecycles: change.lifecycles.clone(),
            operations: change.scope_operations().clone(),
        });
        record.accepted_case = Some(case);
        record.discover = None;
        self.feature_fresh(key, gate)?;
        gate.commit()?;
        self.journal(|j| j.discovery = Some(record))?;
        self.discovery = None;
        let trial = self.feature.as_mut().unwrap();
        trial.accepted = true;
        self.page = Page::DiscoveryRequirement(trial.view()?);
        self.notice = "You explicitly kept this typed result for this exact copied example. Nothing was adopted or sent; a new Discover disclosure will include it".into();
        Ok(())
    }
    pub(super) fn prepare_discover(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.no_pending()?;
        if self.ready.is_some()
            || self.generation_blocked
            || self
                .journal
                .as_ref()
                .is_some_and(|j| j.value.provider.is_some())
        {
            return Err("Resolve the earlier provider request before reviewing Discover".into());
        }
        let current = self.current(key)?;
        let change = self
            .change
            .as_ref()
            .ok_or("Try the actual completed change before Discover")?;
        let source_operation = self
            .has_task_evidence(&change.operation)
            .then(|| change.operation.clone());
        if let Some(operation) = &source_operation {
            self.check_task_evidence(operation, gate)?;
        }
        let mut record = self.journal.as_ref().and_then(|j| j.value.discovery.clone()).ok_or("This change has no retained exact original provider evidence. Make a fresh change request; saved work is kept")?;
        record.validate()?;
        if record.basis != Basis::capture(&change.snapshot)?
            || record.tool != current.association
            || record.need != change.need
        {
            return Err(
                "The original need, selection or saved work changed; prepare a fresh comparison"
                    .into(),
            );
        }
        if record.candidate()? != change.candidate {
            return Err("The selected original candidate changed".into());
        }
        if record.external.is_some() && source_operation.is_none() {
            return Err(
                "The original external source must be checked again before Discover".into(),
            );
        }
        let request_id = id("discover");
        let draft = self.discovery_draft(&record, change, &request_id, &current.store, gate)?;
        let request = match &draft {
            Draft::Whole(d) => d.request(),
            Draft::Rule(d) => d.request(),
        }
        .clone();
        record.selection = Some(host::Selection {
            operation: change.operation.clone(),
            population: change.population.clone(),
            lifecycles: change.lifecycles.clone(),
            operations: change.scope_operations().clone(),
        });
        let options = record.options()?;
        let transport = self.transport(options.provider)?;
        gate.check()?;
        let prepared =
            prepare_development(transport.clone(), &request, options.clone()).map_err(error)?;
        let wire = encode_request(&request, &options).map_err(error)?;
        record.discover = Some(host::Invocation::prepared(&request, prepared.disclosure())?);
        let association = ProviderAssociation {
            request: request.clone(),
            provider: options.provider,
            profile: options.profile,
            wire_request: wire.digest()?,
            wire_source: wire.source_digest.clone(),
            issued: false,
            modify: None,
            reconcile: None,
        };
        self.current(key)?;
        if let Some(operation) = &source_operation {
            self.check_task_evidence(operation, gate)?;
        }
        gate.check()?;
        self.journal(|j| {
            j.discovery = Some(record.clone());
            j.provider = Some(association.clone());
        })?;
        self.page = Page::Consent {
            disclosure: prepared.disclosure().clone(),
            need: record.need,
            request: request.clone(),
            review: String::from_utf8(wire.prompt).map_err(error)?,
            basis: Some(record.basis.clone()),
        };
        self.discovery = Some(State {
            draft: Some(draft),
            queue: None,
            view: None,
            source_operation,
            experience: None,
            opened_example: None,
            resolves: vec![],
        });
        self.ready = Some(Ready {
            prepared,
            request,
            transport,
            epoch: key.epoch,
            basis: Some(record.basis),
            association,
            reconciliation: None,
        });
        self.notice = "Discover is a separate request. Review its exact sources, context and examples before deciding whether to send it".into();
        Ok(())
    }
    pub(super) fn run_discover(
        &mut self,
        disclosure: String,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        self.current(key)?;
        self.discovery_fresh(key, gate)?;
        let ready = self
            .ready
            .as_ref()
            .ok_or("Review the exact Discover request first")?;
        let association = self
            .journal
            .as_ref()
            .and_then(|j| j.value.provider.as_ref())
            .ok_or("The prepared Discover association is missing")?;
        let mut record = self
            .journal
            .as_ref()
            .and_then(|j| j.value.discovery.clone())
            .ok_or("The original change is missing")?;
        record.validate()?;
        let invocation = record
            .discover
            .as_mut()
            .ok_or("The exact Discover request is missing")?;
        if ready.epoch != key.epoch
            || ready.basis.as_ref() != key.basis.as_ref()
            || ready.prepared.disclosure().digest() != disclosure
            || invocation.disclosure != *ready.prepared.disclosure()
            || invocation.request()? != ready.request
            || invocation.issued
            || association.issued
            || canonical_bytes(association).map_err(error)?
                != canonical_bytes(&ready.association).map_err(error)?
        {
            return Err(
                "The displayed Discover request or consent changed; nothing was sent".into(),
            );
        }
        let consent = ConsentReceipt {
            disclosure_digest: disclosure,
            approval_reference: format!("studio-discover-click-{}", key.operation),
            expires_at_unix_ms: unix_ms().saturating_add(5 * 60_000),
        };
        invocation.consent = Some(consent.clone());
        invocation.issued = true;
        let mut issued = association.clone();
        issued.issued = true;
        gate.check()?;
        self.journal(|j| {
            j.discovery = Some(record.clone());
            j.provider = Some(issued);
        })?;
        let ready = self.ready.take().unwrap();
        let provider = ready.prepared.authorize(consent);
        let result = provider
            .develop(&ready.request, &|| {
                #[cfg(test)]
                if self
                    .config
                    .hooks
                    .cancel_after_validated_provider
                    .load(Ordering::Acquire)
                    && provider
                        .receipt()
                        .is_some_and(|r| r.state == JobState::TransportValidated)
                {
                    gate.cancelled.store(true, Ordering::Release);
                }
                gate.cancelled.load(Ordering::Acquire)
            })
            .map_err(error);
        let receipt = provider.receipt();
        let raw = provider.raw_response();
        drop(provider);
        let terminal = ready.transport.reconcile(&ready.request.id);
        if let (Ok(result), Some(raw), Some(receipt)) = (&result, &raw, &receipt) {
            record
                .discover
                .as_mut()
                .unwrap()
                .finish(result, raw, receipt)?;
            // This single bounded journal publication is the terminal completion boundary.
            self.journal(|j| {
                j.discovery = Some(record.clone());
                j.provider = None;
            })?;
        } else if terminal.as_ref().is_ok_and(|r| {
            r.request_id == ready.request.id
                && r.request_digest == ready.association.wire_request
                && r.source_digest == ready.association.wire_source
                && r.state.is_terminal()
                && r.state != JobState::Interrupted
                && r.state != JobState::TransportValidated
        }) {
            self.journal(|j| j.provider = None)?;
        } else {
            self.generation_blocked = true;
        }
        self.evaluate_discovery(result?, record, key, gate)
    }
    fn evaluate_discovery(
        &mut self,
        result: DevelopmentResult,
        record: host::Record,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_discovery_evaluation {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        gate.check()?;
        self.current(key)?;
        let source_operation = self.discovery.as_ref().unwrap().source_operation.clone();
        if let Some(operation) = &source_operation {
            self.check_task_evidence(operation, gate)?;
        }
        let store = self.opened.as_ref().unwrap().store.clone();
        let state = self.discovery.as_ref().unwrap();
        let policy = if let Some(case) = &record.accepted_case {
            case.check(
                self.change
                    .as_ref()
                    .ok_or("The exact original change is unavailable")?,
                &record,
                &store,
                gate,
            )?
            .0
        } else {
            crate::product_discovery::DiscoveryPolicy::default()
        };
        let (queue, view, report) = match state
            .draft
            .as_ref()
            .ok_or("The exact prepared discovery is missing")?
        {
            Draft::Whole(draft) => {
                let alternatives = draft.prepare_alternatives(
                    &store,
                    &result,
                    &key.operation,
                    gate.cancelled.clone(),
                )?;
                let queue = draft
                    .evaluate(&store, result, policy, alternatives, gate.cancelled.clone())
                    .map_err(error)?;
                let view = queue.checked_view(&store, gate.cancelled.clone())?;
                let report = queue.report().clone();
                (Queue::Whole(queue), view, report)
            }
            Draft::Rule(draft) => {
                #[cfg(test)]
                let started = std::time::Instant::now();
                #[cfg(test)]
                eprintln!("Discover fixed-rule evaluation begins");
                let queue = draft.evaluate(
                    &store,
                    draft.selection(),
                    result,
                    policy,
                    vec![],
                    gate.cancelled.clone(),
                )?;
                #[cfg(test)]
                eprintln!(
                    "Discover fixed-rule evaluation finished at {:?}; checked presentation begins",
                    started.elapsed()
                );
                let view = queue.checked_view(&store, draft.selection(), gate.cancelled.clone())?;
                #[cfg(test)]
                eprintln!(
                    "Discover fixed-rule checked presentation finished at {:?}",
                    started.elapsed()
                );
                let report = queue.report().clone();
                (Queue::Rule(queue), view, report)
            }
        };
        self.current(key)?;
        if let Some(operation) = &source_operation {
            self.check_task_evidence(operation, gate)?;
        }
        if !gate.finish() {
            return Err("Discover was cancelled; the exact terminal result was kept without publishing choice authority".into());
        }
        let view = host::View {
            stale: false,
            basis: record.basis.clone(),
            lineage: record.identity()?,
            queue: view,
            report: Some(Arc::new(report)),
            identities: record.evidence_text()?,
        };
        let state = self.discovery.as_mut().unwrap();
        state.queue = Some(queue);
        state.view = Some(view.clone());
        // Cache only the successfully checked, exact-record report. Navigation
        // can replace State without either losing this report or retaining its
        // queues, experiences, source operation or choice tickets as authority.
        self.cached_discovery_report = Some(CachedDiscoveryReport {
            tool: record.tool.clone(),
            view: view.clone(),
        });
        self.page = Page::Discovery(view);
        self.notice = "Discover completed with independent local checks. Only actual demonstrated unresolved consequences become questions".into();
        Ok(())
    }
    pub(super) fn discovery_back(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        let store = self.current(key)?.store.clone();
        let change = self
            .change
            .as_ref()
            .ok_or("The original copied change is unavailable")?;
        let mut view = change.checked_view(&store, gate)?;
        self.decorate_change(&mut view)?;
        self.page = Page::Change(view);
        Ok(())
    }
    fn discovery_fresh(&self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.current(key)?;
        gate.check()?;
        if let Some(operation) = self
            .discovery
            .as_ref()
            .and_then(|s| s.source_operation.as_ref())
        {
            if !self.has_task_evidence(operation) {
                return Err("The original task source proof is no longer available".into());
            }
            self.check_task_key(key)?;
            self.check_task_evidence(operation, gate)?;
        }
        Ok(())
    }
    pub(super) fn discovery_example(
        &mut self,
        question: &str,
        witness: usize,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.discovery_fresh(key, gate)?;
        let store = self.opened.as_ref().unwrap().store.clone();
        let state = self
            .discovery
            .as_ref()
            .ok_or("No checked discovery queue is available")?;
        let (experience, playback) = match state
            .queue
            .as_ref()
            .ok_or("No checked discovery report is available")?
        {
            Queue::Whole(queue) => {
                let e = queue.open_example(&store, question, witness, gate.cancelled.clone())?;
                let v = e.view();
                (Experience::WholeExample(e), v)
            }
            Queue::Rule(queue) => {
                let Some(Draft::Rule(draft)) = &state.draft else {
                    return Err("The fixed rule selection is unavailable".into());
                };
                let e = queue.open_example(
                    &store,
                    draft.selection(),
                    question,
                    witness,
                    gate.cancelled.clone(),
                )?;
                let v = e.playback()?;
                (Experience::RuleExample(e), v)
            }
        };
        self.discovery_fresh(key, gate)?;
        if !gate.finish() {
            return Err("Opening the example was cancelled".into());
        }
        self.page = Page::DiscoveryExperience(host::ExperienceView::example(
            key.basis.clone().unwrap(),
            playback,
        ));
        let state = self.discovery.as_mut().unwrap();
        state.experience = Some(experience);
        state.opened_example = Some((question.into(), witness, false));
        Ok(())
    }
    pub(super) fn discovery_copy(
        &mut self,
        question: &str,
        witness: usize,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.discovery_fresh(key, gate)?;
        let store = self.opened.as_ref().unwrap().store.clone();
        let state = self
            .discovery
            .as_ref()
            .ok_or("No checked discovery queue is available")?;
        let view = state
            .view
            .as_ref()
            .ok_or("The complete report is unavailable")?;
        if !view.queue.defects.is_empty() {
            return Err("The actual change violates a retained obligation and needs repair before a preference can be adopted".into());
        }
        match state
            .queue
            .as_ref()
            .ok_or("No checked discovery report is available")?
        {
            Queue::Whole(queue) => {
                let pair = queue.open_pair(&store, question, witness, gate.cancelled.clone())?;
                let view =
                    pair.checked_view(&store, &key.operation, &[], gate.cancelled.clone())?;
                self.discovery_fresh(key, gate)?;
                if !gate.finish() {
                    return Err("The current-work copy was cancelled".into());
                }
                self.page = Page::DiscoveryExperience(host::ExperienceView::copied(
                    key.basis.clone().unwrap(),
                    view,
                    true,
                ));
                self.discovery.as_mut().unwrap().experience = Some(Experience::Pair(pair));
            }
            Queue::Rule(queue) => {
                let Some(Draft::Rule(draft)) = &state.draft else {
                    return Err("The fixed rule selection is unavailable".into());
                };
                let selection = draft.selection().clone();
                // Revoke the actual published receiver before any copy failure.
                // Mutating a discarded clone would leave the old choice usable.
                let change = self
                    .change
                    .as_mut()
                    .ok_or("The original fixed rule is unavailable")?;
                change.clear_readiness();
                self.page = Page::Change(change.view()?);
                let trace = queue.copy_trace(
                    &store,
                    &selection,
                    question,
                    witness,
                    gate.cancelled.clone(),
                )?;
                self.change
                    .as_mut()
                    .unwrap()
                    .import_rule_trace(&store, &selection, &trace, gate)?;
                self.discovery_fresh(key, gate)?;
                let view = self.change.as_ref().unwrap().checked_view(&store, gate)?;
                if !gate.finish() {
                    self.change.as_mut().unwrap().clear_readiness();
                    self.page = Page::Change(self.change.as_ref().unwrap().view()?);
                    return Err("The copied rule was cancelled before publication".into());
                }
                self.page = Page::Change(view);
            }
        }
        self.discovery.as_mut().unwrap().opened_example = Some((question.into(), witness, true));
        Ok(())
    }
    pub(super) fn discovery_report(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.discovery_fresh(key, gate)?;
        let view = self.discovery.as_ref().and_then(|s| s.view.clone()).ok_or("This reopened saved choice has no new discovery report. Its old observations remain history; return to saved work")?;
        self.page = Page::Discovery(view);
        Ok(())
    }
    pub(super) fn discovery_reset(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        let (question, witness, copied) = self
            .discovery
            .as_ref()
            .and_then(|s| s.opened_example.clone())
            .ok_or("This later-work choice must be reopened from saved work")?;
        if copied {
            self.discovery_copy(&question, witness, key, gate)
        } else {
            self.discovery_example(&question, witness, key, gate)
        }
    }
    pub(super) fn discovery_trial(
        &mut self,
        input: SemanticInput,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.discovery_fresh(key, gate)?;
        let store = self.opened.as_ref().unwrap().store.clone();
        if let Page::DiscoveryExperience(view) = &mut self.page {
            view.available = [false; 6];
            view.minimal = false;
        }
        let state = self
            .discovery
            .as_mut()
            .ok_or("No checked copied experience is available")?;
        let view = match state
            .experience
            .as_mut()
            .ok_or("Open an actual example or copied pair first")?
        {
            Experience::WholeExample(example) => {
                example.trial(&store, input, gate.cancelled.clone())?;
                host::ExperienceView::example(key.basis.clone().unwrap(), example.view())
            }
            Experience::RuleExample(example) => {
                let Some(Draft::Rule(draft)) = &state.draft else {
                    return Err("The exact rule selection is unavailable".into());
                };
                example.trial(&store, draft.selection(), input, gate.cancelled.clone())?;
                host::ExperienceView::example(key.basis.clone().unwrap(), example.playback()?)
            }
            Experience::Pair(pair) => {
                pair.trial(&store, input, gate.cancelled.clone())?;
                host::ExperienceView::copied(
                    key.basis.clone().unwrap(),
                    pair.checked_view(
                        &store,
                        &key.operation,
                        &state.resolves,
                        gate.cancelled.clone(),
                    )?,
                    state.opened_example.is_some(),
                )
            }
        };
        self.discovery_fresh(key, gate)?;
        if !gate.finish() {
            return Err(
                "The new copied input was cancelled; previous choice readiness was revoked".into(),
            );
        }
        self.page = Page::DiscoveryExperience(view);
        Ok(())
    }
    pub(super) fn discovery_choose(
        &mut self,
        ticket: &Digest,
        choice: usize,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        self.discovery_fresh(key, gate)?;
        let current = self.current(key)?;
        let state = self
            .discovery
            .as_ref()
            .ok_or("No checked copied pair is open")?;
        let Some(Experience::Pair(pair)) = &state.experience else {
            return Err(
                "An exact example cannot authorize a choice; open current-work copies first".into(),
            );
        };
        let scenes = pair
            .scenes()
            .ok_or("The copied input has not completed successfully")?;
        let outcome = match choice {
            0 | 1 if scenes[choice].program() == current.snapshot.program().map_err(error)? => {
                DecisionOutcome::KeepCurrent
            }
            0 | 1 => DecisionOutcome::Accept {
                artifact: scenes[choice].program().artifact.program_digest.clone(),
            },
            2 => DecisionOutcome::EitherAcceptable,
            3 => DecisionOutcome::BothNeeded,
            4 => DecisionOutcome::NeitherFits,
            5 => DecisionOutcome::Deferred,
            _ => return Err("Choose one of the displayed outcomes".into()),
        };
        let change = pair.prepare_choice(
            &current.store,
            ticket,
            outcome,
            &format!("discovery-{}", key.operation),
            &key.operation,
            &state.resolves,
            gate.cancelled.clone(),
        )?;
        let source_operation = state.source_operation.clone();
        self.commit_change(change, key.basis.clone().unwrap(), gate, "Your experienced choice was saved. Pending outcomes keep both experiences and leave the live design unchanged", source_operation.as_deref())
    }
    pub(super) fn reopen_discovery_pair(
        &mut self,
        decision: &str,
        key: &Key,
        gate: &Gate,
    ) -> Result<bool, String> {
        let current = self.current(key)?;
        let store = current.store.clone();
        let pair = match whole::PairExperience::reopen(
            &store,
            decision,
            &key.operation,
            gate.cancelled.clone(),
        ) {
            Ok(pair) => pair,
            Err(whole::AdmissionError::RuleComparisonRequired) => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        let mut evidence = None;
        for source in pair.prospective_authored_sources() {
            if let Some(next) = self.retained_task_evidence(source, gate)? {
                if evidence.is_some() {
                    return Err("Two external designs require independently current task evidence before reopening".into());
                }
                evidence = Some(next);
            }
        }
        let resolves = vec![decision.into()];
        let view = pair.checked_view(&store, &key.operation, &resolves, gate.cancelled.clone())?;
        // Choice readiness may take time to reproduce. Recheck the same original
        // authored capture and registered task after that work, before publishing.
        let evidence = match evidence {
            Some(prior) => self.retained_task_evidence(prior.source(), gate)?,
            None => None,
        };
        self.current(key)?;
        if !gate.finish() {
            return Err("Reopening the saved choice was cancelled".into());
        }
        let source_operation = evidence.as_ref().map(|_| key.operation.clone());
        self.task.evidence = evidence.map(|e| (key.operation.clone(), e));
        self.discovery = Some(State {
            draft: None,
            queue: None,
            view: None,
            source_operation,
            experience: Some(Experience::Pair(pair)),
            opened_example: None,
            resolves,
        });
        self.change = None;
        self.page = Page::DiscoveryExperience(host::ExperienceView::copied(
            key.basis.clone().unwrap(),
            view,
            false,
        ));
        self.notice = "This is a fresh pair on current saved work. Historical observations were not reused as current approval; try the current copies before resolving".into();
        Ok(true)
    }
}
