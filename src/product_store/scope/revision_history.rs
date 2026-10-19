//! Conservative feasibility bounds for revisions hidden by event coalescing.
//! A feasible allocation is not an authenticated count of the original writes.
use super::common_fact_admission::{refuse, ProofMeter};
use super::*;
use std::collections::VecDeque;
type Key = (Id, Id);
fn err(reason: &str) -> StoreError {
    refuse("coalesced revision history", reason)
}
pub(super) struct RevisionHistory {
    minimum: BTreeMap<Key, u64>,
    events: Vec<(Vec<Key>, u64)>,
}
impl RevisionHistory {
    pub fn new(seed: &DataSnapshot, shared: impl Fn(&Record) -> bool) -> Self {
        Self {
            minimum: seed
                .records
                .iter()
                .filter(|r| shared(r))
                .map(|r| ((r.entity.clone(), r.id.clone()), r.revision))
                .collect(),
            events: Vec::new(),
        }
    }
    pub fn event(&mut self, shared: Vec<Key>, exact: u64, meter: &mut ProofMeter) -> Result<()> {
        meter.charge(shared.len() as u64 + 1)?;
        let mandatory = exact
            .checked_add(shared.len() as u64)
            .ok_or_else(|| err("mandatory write count overflow"))?;
        let remaining = (RuntimeLimits::default().transaction_writes as u64)
            .checked_sub(mandatory)
            .ok_or_else(|| err("event exceeds transaction-wide writes"))?;
        let mut seen = BTreeSet::new();
        for key in &shared {
            if !seen.insert(key) {
                return Err(err("duplicate coalesced row change"));
            }
            let minimum = self.minimum.entry(key.clone()).or_insert(0);
            *minimum = minimum
                .checked_add(1)
                .ok_or_else(|| err("shared revision overflow"))?;
        }
        self.events.push((shared, remaining));
        Ok(())
    }
    pub fn validate(&self, data: &DataSnapshot, meter: &mut ProofMeter) -> Result<()> {
        meter.charge((self.minimum.len() + self.events.len() + data.records.len()) as u64)?;
        let actual: BTreeMap<_, _> = data
            .records
            .iter()
            .map(|r| ((r.entity.clone(), r.id.clone()), r.revision))
            .collect();
        let mut rows = BTreeMap::new();
        let mut demand = Vec::new();
        for (key, minimum) in &self.minimum {
            let extra = actual
                .get(key)
                .and_then(|n| n.checked_sub(*minimum))
                .ok_or_else(|| err("shared row lacks its minimum recorded writes"))?;
            if extra > 0 {
                rows.insert(key, demand.len());
                demand.push(extra);
            }
        }
        let events = self
            .events
            .iter()
            .map(|(keys, capacity)| {
                (
                    keys.iter()
                        .filter_map(|key| rows.get(key).copied())
                        .collect(),
                    *capacity,
                )
            })
            .collect::<Vec<_>>();
        allocate(&demand, &events, meter)
    }
}
#[derive(Clone)]
struct Edge {
    to: usize,
    reverse: usize,
    capacity: u64,
}
fn edge(graph: &mut [Vec<Edge>], from: usize, to: usize, capacity: u64) {
    let forward = graph[from].len();
    let reverse = graph[to].len();
    graph[from].push(Edge {
        to,
        reverse,
        capacity,
    });
    graph[to].push(Edge {
        to: from,
        reverse: forward,
        capacity: 0,
    });
}
// Row demands may span several events. Residual paths avoid falsely refusing a
// valid allocation merely because an earlier row used another row's only event.
fn allocate(demand: &[u64], events: &[(Vec<usize>, u64)], meter: &mut ProofMeter) -> Result<()> {
    meter.charge((demand.len() + events.len() + 1) as u64)?;
    let source = 0;
    let event_start = 1 + demand.len();
    let sink = event_start + events.len();
    let mut graph = vec![Vec::new(); sink + 1];
    let mut needed = 0u64;
    for (row, amount) in demand.iter().enumerate() {
        needed = needed
            .checked_add(*amount)
            .ok_or_else(|| err("revision demand overflow"))?;
        edge(&mut graph, source, 1 + row, *amount);
    }
    for (event, (rows, capacity)) in events.iter().enumerate() {
        let node = event_start + event;
        edge(&mut graph, node, sink, *capacity);
        for row in rows {
            meter.charge(1)?;
            edge(&mut graph, 1 + row, node, *capacity);
        }
    }
    while needed > 0 {
        meter.charge(1)?;
        let mut parent = vec![None; graph.len()];
        parent[source] = Some((source, 0));
        let mut queue = VecDeque::from([source]);
        while let Some(node) = queue.pop_front() {
            for (index, edge) in graph[node].iter().enumerate() {
                meter.charge(1)?;
                if edge.capacity > 0 && parent[edge.to].is_none() {
                    parent[edge.to] = Some((node, index));
                    queue.push_back(edge.to);
                }
            }
            if parent[sink].is_some() {
                break;
            }
        }
        if parent[sink].is_none() {
            return Err(err("revisions exceed shared event write allowances"));
        }
        let mut amount = needed;
        let mut node = sink;
        while node != source {
            meter.charge(1)?;
            let (from, index) =
                parent[node].ok_or_else(|| err("revision allocation path missing"))?;
            amount = amount.min(graph[from][index].capacity);
            node = from;
        }
        node = sink;
        while node != source {
            meter.charge(1)?;
            let (from, index) =
                parent[node].ok_or_else(|| err("revision allocation path missing"))?;
            let reverse = graph[from][index].reverse;
            graph[from][index].capacity -= amount;
            graph[node][reverse].capacity += amount;
            node = from;
        }
        needed -= amount;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    fn meter() -> ProofMeter {
        ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap()
    }
    #[test]
    fn coalesced_allocation_reassigns_capacity_and_rejects_overloaded_event_subsets() {
        allocate(&[1, 1], &[(vec![0, 1], 1), (vec![0], 1)], &mut meter()).unwrap();
        // Enough total capacity, but both rows compete for the first event.
        assert!(allocate(&[1, 1], &[(vec![0, 1], 1), (vec![], 2)], &mut meter()).is_err());
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut cancelled = ProofMeter::new(ProofBudget::default(), cancellation.clone()).unwrap();
        cancellation.store(true, Ordering::Relaxed);
        assert!(allocate(&[1, 1], &[(vec![0, 1], 2)], &mut cancelled).is_err());
        let mut exhausted = ProofMeter::new(
            ProofBudget {
                units: 4,
                elapsed_millis: 60_000,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(allocate(&[1, 1], &[(vec![0, 1], 2)], &mut exhausted).is_err());
    }
    #[test]
    fn exact_epoch_endpoint_closes_shared_write_capacity_before_reseed() {
        // This is an isolated feasibility model, never a durable-store fixture.
        let key = ("shared".to_owned(), "row".to_owned());
        let seed = DataSnapshot {
            version: CONTRACT_VERSION,
            project_id: "epoch-revisions".into(),
            generation: 0,
            schema: vec![],
            records: vec![Record {
                entity: key.0.clone(),
                id: key.1.clone(),
                revision: 1,
                created_program: canonical_digest(IdentityDomain::Source, &"revision-model")
                    .unwrap(),
                archived: false,
                values: Values::new(),
            }],
            events: vec![],
        };
        let cap = RuntimeLimits::default().transaction_writes as u64;
        let mut first = RevisionHistory::new(&seed, |_| true);
        first
            .event(vec![key.clone()], cap - 2, &mut meter())
            .unwrap();
        let mut boundary = seed.clone();
        boundary.records[0].revision = 3; // One coalesced row, two real writes.
        first.validate(&boundary, &mut meter()).unwrap();
        let mut second = RevisionHistory::new(&boundary, |_| true);
        second.event(vec![key], cap - 1, &mut meter()).unwrap();
        let mut endpoint = boundary.clone();
        endpoint.records[0].revision = 4;
        second.validate(&endpoint, &mut meter()).unwrap();
        endpoint.records[0].revision = 5;
        assert!(second.validate(&endpoint, &mut meter()).is_err());
        boundary.records[0].revision = 4;
        assert!(first.validate(&boundary, &mut meter()).is_err());
    }
}
