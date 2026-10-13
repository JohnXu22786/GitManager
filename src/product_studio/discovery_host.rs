//! One bounded exact discovery lineage. Stored bytes are evidence to recheck,
//! never a deserialized queue, an approval token, or permission to resend.
use super::*;
use crate::product_discovery::ExactUtf8DevelopmentRequest;
use serde::{Deserialize, Serialize};

pub(super) fn verify_original_need(
    store: &ProductStore,
    basis: &ProjectSnapshot,
    original: &DevelopmentRequest,
    need: &str,
    external: bool,
    gate: &Gate,
) -> Result<(), String> {
    let mut rebuilt = DecisionEngine::new(
        LocalRuntime::with_cancellation(gate.cancelled.clone()),
        IntentArchive::new(store.clone()),
    )
    .development_request(
        basis,
        &original.id,
        DevelopmentOperation::Modify,
        need,
        original.context.clone(),
    )
    .map_err(error)?;
    if external {
        let before = original
            .sources
            .first()
            .ok_or("Missing actual task baseline")?;
        rebuilt.sources.retain(|s| s.binding != before.binding);
        rebuilt.sources.insert(0, before.clone());
    }
    if &rebuilt != original {
        return Err("The retained original human need or exact inherited request changed".into());
    }
    gate.check()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompletionEvidence {
    pub raw: response_storage::ExactStoredResponseV1,
    pub producer: Producer,
    pub receipt: JobReceipt,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Invocation {
    pub request: ExactUtf8DevelopmentRequest,
    pub provider: ProviderKind,
    pub profile: CapabilityProfile,
    pub result_limit: usize,
    pub disclosure: DataDisclosure,
    pub consent: Option<ConsentReceipt>,
    pub issued: bool,
    pub completed: Option<CompletionEvidence>,
}
impl Invocation {
    pub fn prepared(
        request: &DevelopmentRequest,
        disclosure: &DataDisclosure,
    ) -> Result<Self, String> {
        Ok(Self {
            request: ExactUtf8DevelopmentRequest::from_request(request).map_err(error)?,
            provider: disclosure.provider,
            profile: disclosure.profile,
            result_limit: crate::product_provider::JobLimits::default().result_bytes,
            disclosure: disclosure.clone(),
            consent: None,
            issued: false,
            completed: None,
        })
    }
    pub fn request(&self) -> Result<DevelopmentRequest, String> {
        self.request.to_request().map_err(error)
    }
    pub fn result(&self) -> Result<DevelopmentResult, String> {
        let completed = self
            .completed
            .as_ref()
            .ok_or("This request has no retained terminal result")?;
        let raw = completed.raw.to_bytes(self.result_limit).map_err(error)?;
        let result = DevelopmentResult {
            response: DevelopmentResponse::parse(&raw).map_err(error)?,
            producer: completed.producer.clone(),
        };
        result.validate_for(&self.request()?).map_err(error)?;
        Ok(result)
    }
    pub fn retain_ingested(
        &mut self,
        result: &crate::product_provider::ProviderResult,
    ) -> Result<(), String> {
        let request = self.request()?;
        let receipt = &result.receipt;
        let producer = match receipt.provenance {
            crate::product_provider::InvocationProvenance::TransportFixture => Producer::Fixture {
                name: format!("recorded transport fixture {}", receipt.request_id),
            },
            crate::product_provider::InvocationProvenance::LiveCli => Producer::LiveAgent {
                provider: match receipt.provider {
                    ProviderKind::Codex => "codex",
                    ProviderKind::Claude => "claude",
                }
                .into(),
                invocation_id: receipt.request_id.clone(),
                session_id: receipt.session_id.clone(),
                request_digest: request.identity().map_err(error)?,
            },
        };
        let domain = DevelopmentResult {
            response: DevelopmentResponse::parse(&result.final_bytes).map_err(error)?,
            producer,
        };
        self.finish(&domain, &result.final_bytes, receipt)
    }
    pub fn finish(
        &mut self,
        result: &DevelopmentResult,
        raw: &[u8],
        receipt: &JobReceipt,
    ) -> Result<(), String> {
        self.completed = Some(CompletionEvidence {
            raw: response_storage::ExactStoredResponseV1::from_bytes(raw, self.result_limit)
                .map_err(error)?,
            producer: result.producer.clone(),
            receipt: receipt.clone(),
        });
        self.validate()?;
        if &self.result()? != result {
            return Err(
                "The returned domain result differs from the exact retained raw bytes".into(),
            );
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        let request = self.request()?;
        let options = ProviderOptions {
            provider: self.provider,
            profile: self.profile,
            ..ProviderOptions::default()
        };
        let wire = encode_request(&request, &options).map_err(error)?;
        let d = &self.disclosure;
        if self.result_limit != options.limits.result_bytes
            || d.provider != self.provider
            || d.profile != self.profile
            || d.request_id != request.id
            || d.request_digest != wire.digest()?
            || d.source_digest != wire.source_digest
            || d.prompt_digest != crate::product_provider::digest(&wire.prompt)
            || d.schema_digest != crate::product_provider::digest(&wire.schema)
            || d.purpose != wire.purpose
            || d.data_categories != wire.data_categories
            || self.issued != self.consent.is_some()
            || self.consent.as_ref().is_some_and(|c| {
                c.disclosure_digest != d.digest() || c.approval_reference.is_empty()
            })
            || (self.completed.is_some() && !self.issued)
        {
            return Err(
                "The retained provider request, disclosure or explicit consent changed".into(),
            );
        }
        if let Some(completed) = &self.completed {
            let receipt = &completed.receipt;
            let raw = completed.raw.to_bytes(self.result_limit).map_err(error)?;
            let result = self.result()?;
            let producer = match receipt.provenance {
                crate::product_provider::InvocationProvenance::TransportFixture => {
                    Producer::Fixture {
                        name: format!("recorded transport fixture {}", receipt.request_id),
                    }
                }
                crate::product_provider::InvocationProvenance::LiveCli => Producer::LiveAgent {
                    provider: match receipt.provider {
                        ProviderKind::Codex => "codex",
                        ProviderKind::Claude => "claude",
                    }
                    .into(),
                    invocation_id: receipt.request_id.clone(),
                    session_id: receipt.session_id.clone(),
                    request_digest: request.identity().map_err(error)?,
                },
            };
            if receipt.version != crate::product_provider::RECEIPT_VERSION
                || receipt.session_id.as_ref().is_none_or(|id| id.is_empty())
                || receipt.started_at_unix_ms.is_none()
                || receipt.finished_at_unix_ms.is_none()
                || receipt.finished_at_unix_ms < receipt.started_at_unix_ms
                || receipt.capabilities.provider != self.provider
                || receipt.capabilities.provenance != receipt.provenance
                || receipt.capabilities.digest() != self.disclosure.capability_digest
                || !receipt.capabilities.supports(self.profile)
                || receipt.state != JobState::TransportValidated
                || receipt.exit_code != Some(0)
                || receipt.request_id != request.id
                || receipt.request_digest != wire.digest()?
                || receipt.source_digest != wire.source_digest
                || receipt.provider != self.provider
                || receipt.disclosure != self.disclosure
                || receipt.consent != self.consent
                || receipt.result_digest.as_deref()
                    != Some(crate::product_provider::digest(&raw).as_str())
                || completed.producer != producer
                || result.producer != producer
            {
                return Err("The exact retained response is not correlated with its actual producer and authorized receipt".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    pub operation: Id,
    pub population: ScopePopulation,
    pub lifecycles: Vec<crate::product_store::scope::LifecycleBinding>,
    pub operations: std::collections::BTreeSet<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExactSource {
    artifact: ArtifactRef,
    binding: SourceBinding,
    source_utf8: String,
    program: AppDefinition,
}
impl ExactSource {
    fn capture(source: &CapturedProgram) -> Result<Self, String> {
        source.validate().map_err(error)?;
        Ok(Self {
            artifact: source.artifact.clone(),
            binding: source.binding.clone(),
            source_utf8: String::from_utf8(source.source_bytes.clone()).map_err(error)?,
            program: source.program.clone(),
        })
    }
    pub fn source(&self) -> Result<CapturedProgram, String> {
        let source = CapturedProgram {
            artifact: self.artifact.clone(),
            binding: self.binding.clone(),
            source_bytes: self.source_utf8.as_bytes().to_vec(),
            program: self.program.clone(),
        };
        source.validate().map_err(error)?;
        Ok(source)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExternalOriginal {
    pub request: ExactUtf8DevelopmentRequest,
    pub after: ExactSource,
    pub provider: ProviderKind,
    pub original_disclosure: Digest,
    pub approval_operation: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub tool: Association,
    pub basis: Basis,
    /// Exact unenriched UI text, independent of both request inheritance and Journal.need.
    pub need: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<Invocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalOriginal>,
    pub selected: Option<Id>,
    pub selection: Option<Selection>,
    pub discover: Option<Invocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_case: Option<discovery_policy::AcceptedCase>,
}
impl Record {
    pub fn identity(&self) -> Result<Digest, String> {
        canonical_digest(IdentityDomain::Evidence, self).map_err(error)
    }
    pub fn evidence_text(&self) -> Result<String, String> {
        let mut text = format!("Original human need: {}\nSelected candidate: {:?}\nExact saved Basis: {:#?}\nExact selection: {:#?}\n", self.need, self.selected, self.basis, self.selection);
        text.push_str(&format!(
            "Explicit local typed case: {:#?}\n",
            self.accepted_case
        ));
        if let Some(external) = &self.external {
            text.push_str(&format!("Actual external original request and captured edit (no preceding provider response):\n{}\n", serde_json::to_string_pretty(external).map_err(error)?));
        }
        for (name, invocation) in [
            ("Original Modify", self.original.as_ref()),
            ("Separate Discover", self.discover.as_ref()),
        ] {
            if let Some(invocation) = invocation {
                let request = invocation.request()?;
                let options = ProviderOptions {
                    provider: invocation.provider,
                    profile: invocation.profile,
                    ..ProviderOptions::default()
                };
                let wire = encode_request(&request, &options).map_err(error)?;
                text.push_str(&format!(
                    "{name} exact disclosed prompt:\n{}\nDisclosure: {:#?}\nConsent: {:#?}\n",
                    String::from_utf8(wire.prompt).map_err(error)?,
                    invocation.disclosure,
                    invocation.consent
                ));
                if let Some(completed) = &invocation.completed {
                    text.push_str(&format!("{name} exact original raw response bytes as UTF-8:\n{}\nActual producer: {:#?}\nActual receipt: {:#?}\n", String::from_utf8(completed.raw.to_bytes(invocation.result_limit).map_err(error)?).map_err(error)?, completed.producer, completed.receipt));
                }
            }
        }
        Ok(text)
    }
    pub fn external(
        tool: Association,
        basis: Basis,
        need: String,
        completed: &journal::CompletedTask,
        change: &ChangeDraft,
    ) -> Result<Self, String> {
        discovery_flow::check_external_origin(
            &change.snapshot,
            completed.external.pending.request(),
            &completed.capture,
        )?;
        let record = Self {
            tool,
            basis,
            need,
            original: None,
            selected: None,
            external: Some(ExternalOriginal {
                request: ExactUtf8DevelopmentRequest::from_request(
                    completed.external.pending.request(),
                )
                .map_err(error)?,
                after: ExactSource::capture(&completed.capture)?,
                provider: completed.external.provider,
                original_disclosure: completed.external.disclosure.clone(),
                approval_operation: completed.external.approval_operation.clone(),
            }),
            selection: Some(Selection {
                operation: change.operation.clone(),
                population: change.population.clone(),
                lifecycles: change.lifecycles.clone(),
                operations: change.scope_operations().clone(),
            }),
            discover: None,
            accepted_case: None,
        };
        record.validate()?;
        Ok(record)
    }
    pub fn provider_original(&self) -> Result<&Invocation, String> {
        self.original
            .as_ref()
            .ok_or_else(|| "This actual external change has no Modify provider invocation".into())
    }
    pub fn provider_original_mut(&mut self) -> Result<&mut Invocation, String> {
        self.original
            .as_mut()
            .ok_or_else(|| "This actual external change has no Modify provider invocation".into())
    }
    pub fn original_request(&self) -> Result<DevelopmentRequest, String> {
        match (&self.original, &self.external) {
            (Some(original), None) => original.request(),
            (None, Some(external)) => external.request.to_request().map_err(error),
            _ => Err("A change must have exactly one actual origin".into()),
        }
    }
    pub fn candidate(&self) -> Result<CapturedProgram, String> {
        match (&self.original, &self.external) {
            (Some(original), None) => {
                let result = original.result()?;
                let selected = self
                    .selected
                    .as_ref()
                    .ok_or("Choose an actual returned design first")?;
                let candidate = result
                    .response
                    .candidates
                    .iter()
                    .find(|c| &c.id == selected)
                    .ok_or("The selected candidate is absent from the actual original result")?;
                CapturedProgram::capture(
                    candidate.source_json.as_bytes(),
                    &self.tool.identity.project_id,
                    result.producer,
                    None,
                )
                .map_err(error)
            }
            (None, Some(external)) => external.after.source(),
            _ => Err("A change must have exactly one actual origin".into()),
        }
    }
    pub fn options(&self) -> Result<ProviderOptions, String> {
        Ok(match (&self.original, &self.external) {
            (Some(original), None) => ProviderOptions {
                provider: original.provider,
                profile: original.profile,
                ..ProviderOptions::default()
            },
            (None, Some(external)) => ProviderOptions {
                provider: external.provider,
                ..ProviderOptions::default()
            },
            _ => return Err("A change must have exactly one actual origin".into()),
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.need.trim().is_empty()
            || self.need.len() > MAX_TEXT_BYTES
            || !self.tool.path.is_absolute()
        {
            return Err("Invalid original discovery need or tool".into());
        }
        let original = self.original_request()?;
        if let Some(provider) = &self.original {
            provider.validate()?;
        }
        if original.operation != DevelopmentOperation::Modify
            || original.project_id != self.tool.identity.project_id
            || !original.sources.iter().any(|s| {
                canonical_digest(IdentityDomain::Source, s).ok().as_ref()
                    == Some(&self.basis.source)
            })
            || (self.original.is_some()
                && original
                    .sources
                    .last()
                    .and_then(|s| canonical_digest(IdentityDomain::Source, s).ok())
                    .as_ref()
                    != Some(&self.basis.source))
            || original.context.data_digest.as_ref() != Some(&self.basis.data)
            || original.context.session_digest.as_ref() != Some(&self.basis.session)
            || canonical_digest(IdentityDomain::Decision, &original.decisions).map_err(error)?
                != self.basis.decisions
        {
            return Err("The original change no longer matches its saved work".into());
        }
        if let Some(external) = &self.external {
            let source = external.after.source()?;
            let before = original
                .sources
                .first()
                .ok_or("Missing original external baseline")?;
            if self.selected.is_some()
                || !valid_id(&external.approval_operation)
                || task_source_host::Disclosure::new(original.clone(), external.provider)?.digest
                    != external.original_disclosure
                || !matches!(source.binding.producer, Producer::ExternalAuthor { .. })
                || !matches!(before.binding.producer, Producer::ExternalAuthor { .. })
                || source.binding.project_id != original.project_id
                || source.binding.task.is_none()
                || source.binding.task.as_ref().map(|t| &t.task_id)
                    != before.binding.task.as_ref().map(|t| &t.task_id)
                || source.binding.program_path != before.binding.program_path
            {
                return Err("The actual external source or original task context changed".into());
            }
        } else if self.selected.is_some() {
            self.candidate()?;
        }
        if let Some(selection) = &self.selection {
            if !valid_id(&selection.operation)
                || (self.selected.is_none() && self.external.is_none())
            {
                return Err("Invalid original copied selection".into());
            }
        }
        if let Some(case) = &self.accepted_case {
            case.validate()?;
            if case.basis != self.basis
                || case.original_request != original.identity().map_err(error)?
                || case.authored_source
                    != canonical_digest(IdentityDomain::Source, &self.candidate()?)
                        .map_err(error)?
                || self.selection.as_ref().map(|s| &s.operation) != Some(&case.source_operation)
            {
                return Err(
                    "The explicit typed case belongs to a different original change".into(),
                );
            }
        }
        if let Some(invocation) = &self.discover {
            invocation.validate()?;
            let request = invocation.request()?;
            if (self.selected.is_none() && self.external.is_none())
                || self.selection.is_none()
                || request.operation != DevelopmentOperation::Discover
                || request.id == original.id
                || request.project_id != original.project_id
                || request
                    .sources
                    .first()
                    .and_then(|s| canonical_digest(IdentityDomain::Source, s).ok())
                    .as_ref()
                    != Some(&self.basis.source)
                || request.context != original.context
                || request.decisions != original.decisions
                || request.sources.len() < 2
            {
                return Err(
                    "Discover lost its exact original change, source, context or distinct identity"
                        .into(),
                );
            }
        }
        Ok(())
    }
    pub fn invocation_mut(&mut self, id: &str) -> Result<&mut Invocation, String> {
        if self
            .original
            .as_ref()
            .is_some_and(|i| i.request().is_ok_and(|r| r.id == id))
        {
            return Ok(self.original.as_mut().unwrap());
        }
        self.discover
            .as_mut()
            .filter(|d| d.request().is_ok_and(|r| r.id == id))
            .ok_or_else(|| "The active request has no exact same-record lineage".into())
    }
    pub fn invocation(&self, id: &str) -> Result<&Invocation, String> {
        if let Some(original) = self
            .original
            .as_ref()
            .filter(|i| i.request().is_ok_and(|r| r.id == id))
        {
            return Ok(original);
        }
        self.discover
            .as_ref()
            .filter(|d| d.request().is_ok_and(|r| r.id == id))
            .ok_or_else(|| "The active request has no exact same-record lineage".into())
    }
}

#[derive(Clone)]
pub(super) struct View {
    pub basis: Basis,
    pub lineage: Digest,
    pub queue: discovery_flow::QueueView,
    /// Complete immutable current-run report, without serializing opaque proof authority.
    pub report: Option<Arc<crate::product_discovery::DiscoveryReport>>,
    pub identities: String,
    pub stale: bool,
}
impl View {
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        trace: &mut WidgetTrace,
        enabled: bool,
    ) -> Option<Action> {
        trace.label(ui, "Discover: checked choices in this actual change");
        let navigate = enabled;
        let enabled = enabled && !self.stale;
        if self.stale {
            trace.label(ui, "This report is stale: saved source, work, intentions or date changed. Its original evidence was kept; it does not authorize a current choice.");
        }
        trace.label(
            ui,
            format!(
                "{} coherent questions · {:?}",
                self.queue.questions.len(),
                self.queue.state
            ),
        );
        for defect in &self.queue.defects {
            trace.label(ui, format!("Needs repair: {defect}"));
        }
        for unknown in &self.queue.unverified {
            trace.label(ui, format!("Not verified: {unknown}"));
        }
        for coverage in &self.queue.coverage {
            trace.label(ui, coverage);
        }
        if self.queue.questions.is_empty() {
            trace.label(ui, "No new question was established by these finite checks. This is not a claim that every behavior is equivalent or correct.");
        }
        let mut action = None;
        for q in &self.queue.questions {
            trace.label(ui, &q.statement);
            for unknown in &q.unknowns {
                trace.label(ui, format!("Still unknown: {unknown}"));
            }
            for (index, example) in q.witnesses.iter().enumerate() {
                trace.label(ui, format!("{} / {}", example.labels[0], example.labels[1]));
                if trace.button(
                    ui,
                    &format!("studio.discovery.example.{}.{index}", q.id),
                    "Play the exact checked example",
                    enabled && example.playable,
                ) {
                    action = Some(Action::DiscoveryExample {
                        question: q.id.clone(),
                        witness: index,
                    });
                }
                if trace.button(
                    ui,
                    &format!("studio.discovery.copy.{}.{index}", q.id),
                    "Try on copies of my current work",
                    enabled && example.copyable && self.queue.defects.is_empty(),
                ) {
                    action = Some(Action::DiscoveryCopy {
                        question: q.id.clone(),
                        witness: index,
                    });
                }
                for issue in example
                    .example_issue
                    .iter()
                    .chain(example.copy_issue.iter())
                {
                    trace.label(ui, issue);
                }
            }
        }
        let _details = ui.collapsing("Complete report and exact evidence details", |ui| {
            trace.label(ui, &self.identities);
            let detail = self.report.as_ref().map(|report| format!("{report:#?}")).unwrap_or_else(|| "No complete report for this exact change is available in this host session. The original exact inputs remain available above; no reconstructed proof is claimed".into());
            egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                let mut text = detail.as_str();
                #[cfg(test)]
                trace.text.push(detail.clone());
                let _detail_text = ui.add(egui::TextEdit::multiline(&mut text).interactive(false).desired_width(f32::INFINITY));
                #[cfg(test)]
                trace.control("studio.discovery.detail-text", _detail_text);
            });
        });
        #[cfg(test)]
        trace.control("studio.discovery.details", _details.header_response);
        if trace.button(
            ui,
            "studio.discovery.back",
            "Back to the original copied change",
            enabled,
        ) {
            action = Some(Action::DiscoveryBack);
        }
        if trace.button(ui, "studio.return", "Return to saved work", navigate) {
            action = Some(Action::ReturnDaily);
        }
        action
    }
}

#[derive(Clone)]
pub(super) struct ExperienceView {
    pub basis: Basis,
    pub labels: [String; 2],
    pub runs: [RuntimeView; 2],
    pub trial_day: i32,
    pub minimal: bool,
    pub ticket: Option<Digest>,
    pub available: [bool; 6],
    pub notes: Vec<String>,
    pub resettable: bool,
}
impl ExperienceView {
    pub fn example(basis: Basis, view: discovery_flow::ExamplePlayback) -> Self {
        Self {
            basis,
            labels: view.labels,
            runs: view.runs,
            trial_day: view.trial_day,
            minimal: view.minimal,
            ticket: None,
            available: [false; 6],
            notes: vec![],
            resettable: true,
        }
    }
    pub fn copied(basis: Basis, view: discovery_flow::PairView, resettable: bool) -> Self {
        Self {
            basis,
            labels: view.labels,
            runs: view.runs,
            trial_day: view.trial_day,
            minimal: view.minimal,
            ticket: Some(view.ticket),
            available: view.available,
            notes: view.readiness_notes,
            resettable,
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn show_experience(
    view: &ExperienceView,
    ui: &mut egui::Ui,
    trace: &mut WidgetTrace,
    left: &mut ProductRuntimeView,
    right: &mut ProductRuntimeView,
    busy: bool,
    frozen: bool,
    blocked: bool,
    today: i32,
    shortcuts: &[Id],
) -> Option<Action> {
    // Both panes are real runtime controls. Each dispatches the same semantic
    // input to both copies, and only one pane can own an unacknowledged edit.
    let pending = left.pending_edits() || right.pending_edits();
    let current = view.basis.day == today;
    let mut action = None;
    trace.label(ui, if view.ticket.is_some() { "Copies of your current saved work" } else { "Exact checked example. These are its original records and date, not your current saved work" });
    trace.label(
        ui,
        format!(
            "Copied date: {}",
            crate::ui::product_runtime_view::value_text(&DataValue::Date {
                days: view.trial_day
            })
        ),
    );
    trace.label(
        ui,
        if view.minimal {
            "This exact example has a checked 1-minimal certificate for the stated reductions"
        } else {
            "This played experience does not claim a minimum certificate"
        },
    );
    for note in &view.notes {
        trace.label(ui, note);
    }
    let enabled = !busy && !pending && !blocked && current;
    if trace.button(
        ui,
        "studio.discovery.next-day",
        "Try one day later on these copies",
        enabled,
    ) {
        action = Some(Action::DiscoveryTrial {
            input: SemanticInput::AdvanceClock { days: 1 },
        });
    }
    if trace.button(
        ui,
        "studio.discovery.reset",
        "Reset to the same checked starting example",
        enabled && view.resettable,
    ) {
        action = Some(Action::DiscoveryReset);
    }
    if let Some(ticket) = &view.ticket {
        for (index, label) in [
            format!("Choose {}", view.labels[0]),
            format!("Choose {}", view.labels[1]),
            "Either is acceptable".into(),
            "I need both".into(),
            "Neither fits".into(),
            "Decide later".into(),
        ]
        .iter()
        .enumerate()
        {
            if trace.button(
                ui,
                &format!("studio.discovery.choose.{index}"),
                label,
                enabled && view.available[index],
            ) {
                action = Some(Action::DiscoveryChoose {
                    ticket: ticket.clone(),
                    choice: index,
                });
            }
        }
        trace.label(ui, "These choices use the checked current-work copies. The four unresolved choices keep both experiences without activating either design.");
    }
    if !current {
        trace.label(
            ui,
            "Today's date changed. Return to saved work before making a fresh choice.",
        );
    }
    if trace.button(
        ui,
        "studio.discovery.report",
        "Back to the complete report",
        !busy && !pending,
    ) {
        action = Some(Action::DiscoveryReport);
    }
    let return_label = if pending {
        "Discard copied edits and return"
    } else {
        "Return to saved work"
    };
    #[cfg(test)]
    trace.text.push(return_label.into());
    if trace.button(ui, "studio.return", return_label, !busy) {
        action = Some(Action::ReturnDaily);
    }
    ui.columns(2, |columns| {
        trace.label(&mut columns[0], &view.labels[0]);
        let mut model = view.runs[0].clone();
        model.read_only |= frozen || right.pending_edits() || !current;
        let output = left.show(
            &mut columns[0],
            &model,
            "discovery-left",
            !busy && action.is_none(),
            false,
            shortcuts,
        );
        trace.append(output.trace);
        let dispatched = output.input.is_some();
        if action.is_none() {
            action = output.input.map(|input| Action::DiscoveryTrial { input });
        }
        trace.label(&mut columns[1], &view.labels[1]);
        let mut model = view.runs[1].clone();
        model.read_only |= frozen || dispatched || left.pending_edits() || !current;
        let output = right.show(
            &mut columns[1],
            &model,
            "discovery-right",
            !busy && action.is_none(),
            false,
            &[],
        );
        trace.append(output.trace);
        if action.is_none() {
            action = output.input.map(|input| Action::DiscoveryTrial { input });
        }
    });
    action
}
