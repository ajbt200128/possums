//! Bounded local aggregation. Real-traffic release remains closed pending review.
//! Runtime export ownership is independent of inference/accounting.
pub mod hooks;
mod infrastructure;
mod labels;
pub mod runtime;
mod tables;
pub use infrastructure::*;
pub use labels::*;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex, MutexGuard,
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
pub use tables::*;

const SECOND: u64 = 1_000_000_000;
const REQUEST_WINDOW: u64 = 300 * SECOND;
const INFRA_WINDOW: u64 = 60 * SECOND;
const POOL: usize = 256;

/// Trusted process clock input, not a request timestamp or a telemetry label.
#[derive(Clone, Copy)]
pub struct ClockReading {
    pub monotonic_ns: u64,
    pub wall_ns: u64,
}
pub trait Clock: Send + Sync {
    fn now(&self) -> Option<ClockReading>;
}
pub struct SystemClock {
    start: Instant,
    #[cfg(test)]
    fixed: Option<ClockReading>,
}
impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            #[cfg(test)]
            fixed: None,
        }
    }
}
impl Clock for SystemClock {
    fn now(&self) -> Option<ClockReading> {
        #[cfg(test)]
        if let Some(now) = self.fixed {
            return Some(now);
        }
        Some(ClockReading {
            monotonic_ns: self.start.elapsed().as_nanos().try_into().ok()?,
            wall_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos()
                .try_into()
                .ok()?,
        })
    }
}
/// Only a deployment owner may choose isolation. There is no request-field API.
#[derive(Clone, Copy, Default)]
pub enum Deployment {
    #[default]
    Off,
    NonIsolated,
    #[cfg(test)]
    IsolatedSynthetic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start_ns: u64,
    pub end_ns: u64,
}
struct Slots<T> {
    active: Box<T>,
    frozen: Option<Box<T>>,
    start: u64,
    eligible: u64,
    attempted: u64,
    pending: Option<(Window, u64)>,
}
impl<T: Default> Default for Slots<T> {
    fn default() -> Self {
        Self {
            active: Box::default(),
            frozen: Some(Box::default()),
            start: 0,
            eligible: u64::MAX,
            attempted: 0,
            pending: None,
        }
    }
}
impl<T: Default> Slots<T> {
    fn reset(&mut self, now: u64, width: u64) -> Option<()> {
        self.start = now / width * width;
        self.eligible = self.start.checked_add(width)?.max(self.attempted);
        *self.active = T::default();
        self.pending = None;
        if let Some(frozen) = &mut self.frozen {
            **frozen = T::default();
        }
        Some(())
    }
    fn freeze(&mut self, epoch: u64, end: u64, allowed: bool) {
        self.attempted = self.attempted.max(end);
        if allowed && self.pending.is_none() && self.frozen.is_some() {
            std::mem::swap(&mut self.active, self.frozen.as_mut().unwrap());
            self.pending = Some((
                Window {
                    start_ns: self.start,
                    end_ns: end,
                },
                epoch,
            ));
        }
        *self.active = T::default();
        self.start = end;
    }
    fn discard(&mut self) {
        *self.active = T::default();
        if let Some(frozen) = &mut self.frozen {
            **frozen = T::default();
        }
        self.pending = None;
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Empty,
    Http,
    Generation,
    Delivery,
    Lease,
}
#[derive(Clone, Copy)]
struct Record {
    key: u64,
    epoch: u64,
    start: u64,
    dispatch: Option<u64>,
    lease_window: u64,
    kind: Kind,
    endpoint: Endpoint,
    model: QualifiedModel,
    lane: Lane,
    first: Option<u8>,
    terminal: bool,
    disposition: Disposition,
}
impl Default for Record {
    fn default() -> Self {
        Self {
            key: 0,
            epoch: 0,
            start: 0,
            dispatch: None,
            lease_window: u64::MAX,
            kind: Kind::Empty,
            endpoint: Endpoint::Other,
            model: QualifiedModel(2),
            lane: Lane::Control,
            first: None,
            terminal: false,
            disposition: Disposition::Unknown,
        }
    }
}
struct State {
    epoch: u64,
    anchor: Option<ClockReading>,
    last: Option<ClockReading>,
    serial: u64,
    requests: Slots<RequestTables>,
    infrastructure: Slots<Infrastructure>,
    records: [Record; POOL],
}
impl Default for State {
    fn default() -> Self {
        Self {
            epoch: 0,
            anchor: None,
            last: None,
            serial: 0,
            requests: Slots::default(),
            infrastructure: Slots::default(),
            records: [Record::default(); POOL],
        }
    }
}

pub struct AggregateMetrics<C: Clock = SystemClock> {
    clock: C,
    // Odd = enabled; changing this token invalidates ALL open/pending observations.
    // A failed try_lock changes it before returning. Closure and consumption must
    // validate the same token, including after their last mutation.
    epoch: std::sync::Arc<AtomicU64>,
    handoff: handoff::Exchange,
    entrants: AtomicU64,
    retired: [AtomicU64; 4],
    untracked_lease: AtomicBool,
    state: Mutex<State>,
}
impl Default for AggregateMetrics {
    fn default() -> Self {
        Self::new(Deployment::Off, SystemClock::default())
    }
}
impl<C: Clock> AggregateMetrics<C> {
    pub fn new(mode: Deployment, clock: C) -> Self {
        let result = Self {
            clock,
            epoch: std::sync::Arc::new(AtomicU64::new(0)),
            handoff: handoff::Exchange::default(),
            entrants: AtomicU64::new(0),
            retired: std::array::from_fn(|_| AtomicU64::new(0)),
            untracked_lease: AtomicBool::new(false),
            state: Mutex::new(State::default()),
        };
        result.enable(mode);
        result
    }
    fn invalidate(&self) {
        let changed = self
            .epoch
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |epoch| {
                (epoch % 2 == 1).then(|| epoch.saturating_add(1))
            })
            .is_ok();
        if changed {
            self.handoff.changed.send_replace(());
        }
    }
    fn lock(&self) -> Option<StateGuard<'_, C>> {
        // Register BEFORE attempting state access: a preempted losing observer
        // is already visible to closure. Never wait for it, and never publish
        // while it can still invalidate the family.
        if self.entrants.fetch_add(1, Ordering::SeqCst) != 0 {
            self.invalidate();
            self.entrants.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        match self.state.try_lock() {
            Ok(mut state) => {
                self.retire(&mut state);
                self.handoff.reclaim(&mut state);
                if state.epoch != self.epoch.load(Ordering::SeqCst) {
                    state.requests.discard();
                    state.infrastructure.discard();
                    state.anchor = None;
                }
                Some(StateGuard {
                    owner: self,
                    guard: Some(state),
                })
            }
            Err(_) => {
                self.invalidate();
                self.entrants.fetch_sub(1, Ordering::SeqCst);
                None
            }
        }
    }
    fn eligible_token(&self, epoch: u64) -> bool {
        self.entrants.load(Ordering::SeqCst) == 1 && epoch == self.epoch.load(Ordering::SeqCst)
    }
    fn retire(&self, state: &mut State) {
        for (word, retired) in self.retired.iter().enumerate() {
            let bits = retired.swap(0, Ordering::AcqRel);
            for bit in 0..64 {
                if bits & (1 << bit) != 0 {
                    state.records[word * 64 + bit] = Record::default();
                }
            }
        }
    }
    /// Local acknowledgement only. False means a concurrent local operation is
    /// still disposing state; retry control polling, never block inference.
    /// Handoff also requires actual owned attempt disposal. This is
    /// nonblocking control polling, not exporter shutdown or a network flush.
    pub fn off(&self) -> bool {
        self.invalidate();
        if let Some(mut state) = self.lock() {
            state.requests.discard();
            state.infrastructure.discard();
            state.anchor = None;
            if self.handoff.busy.load(Ordering::SeqCst) {
                return false;
            }
            self.handoff.reclaim(&mut state);
            true
        } else {
            false
        }
    }
    pub fn enable(&self, mode: Deployment) -> bool {
        if !self.off() {
            return false;
        }
        let fixture = match mode {
            #[cfg(test)]
            Deployment::IsolatedSynthetic => true,
            _ => false,
        };
        if !fixture {
            return true;
        }
        // A lost lease cannot be reconstructed without owning the real permit.
        // Restart is required; never allocate an out-of-pool lifetime marker.
        if self.untracked_lease.load(Ordering::Acquire) {
            return false;
        }
        let Some(mut state) = self.lock() else {
            return false;
        };
        let Some(now) = self.clock.now() else {
            return false;
        };
        let old = self.epoch.load(Ordering::SeqCst);
        let Some(epoch) = old.checked_add(1).filter(|e| e % 2 == 1 && *e < u64::MAX) else {
            return false;
        };
        if !state.restart(now, epoch) {
            return false;
        }
        self.epoch
            .compare_exchange(old, epoch, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
    fn time(&self, state: &mut State) -> Option<(u64, u64)> {
        if state.epoch != self.epoch.load(Ordering::SeqCst) || state.epoch % 2 == 0 {
            return None;
        }
        let result = self.clock.now().and_then(|now| state.check_clock(now));
        if result.is_none() {
            self.invalidate();
            state.requests.discard();
            state.infrastructure.discard();
        }
        result
    }
    fn advance(&self, state: &mut State, mapped: u64) -> bool {
        let Some(request_end) = state.requests.start.checked_add(REQUEST_WINDOW) else {
            self.invalidate();
            return false;
        };
        let Some(infra_end) = state.infrastructure.start.checked_add(INFRA_WINDOW) else {
            self.invalidate();
            return false;
        };
        if mapped.saturating_sub(request_end) >= REQUEST_WINDOW
            || mapped.saturating_sub(infra_end) >= INFRA_WINDOW
        {
            // Scheduling recovery is distinct from clock-fault recovery. No catch-up.
            state.requests.attempted = state
                .requests
                .attempted
                .max(mapped / REQUEST_WINDOW * REQUEST_WINDOW);
            state.infrastructure.attempted = state
                .infrastructure
                .attempted
                .max(mapped / INFRA_WINDOW * INFRA_WINDOW);
            let old = state.epoch;
            let Some(epoch) = old.checked_add(2).filter(|e| *e < u64::MAX) else {
                self.invalidate();
                return false;
            };
            if self
                .epoch
                .compare_exchange(old, epoch, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return false;
            }
            self.handoff.changed.send_replace(());
            let now = state.last.unwrap();
            if !state.restart(
                ClockReading {
                    wall_ns: mapped,
                    ..now
                },
                epoch,
            ) {
                self.invalidate();
            }
            return false;
        }
        if mapped >= request_end {
            let allowed = mapped - request_end <= SECOND
                && state.requests.start >= state.requests.eligible
                && state.requests.active.releasable();
            state.requests.freeze(state.epoch, request_end, allowed);
            state.mark_leases();
        }
        if mapped >= infra_end {
            let allowed = mapped - infra_end <= SECOND
                && state.infrastructure.start >= state.infrastructure.eligible
                && state.infrastructure.active.series_count() > 0;
            state.infrastructure.freeze(state.epoch, infra_end, allowed);
        }
        if mapped
            >= state.requests.start
                + u64::from(state.requests.active.ticks) * SECOND
                + SECOND / 2
                + SECOND
        {
            state.requests.active.valid = false;
        }
        // Unsent local slots expire without replay; exporter timing remains 2C.
        if state
            .requests
            .pending
            .is_some_and(|(w, _)| mapped.saturating_sub(w.end_ns) >= 2 * SECOND)
        {
            state.requests.pending = None;
            **state.requests.frozen.as_mut().unwrap() = RequestTables::default();
        }
        if state
            .infrastructure
            .pending
            .is_some_and(|(w, _)| mapped.saturating_sub(w.end_ns) >= 2 * SECOND)
        {
            state.infrastructure.pending = None;
            **state.infrastructure.frozen.as_mut().unwrap() = Infrastructure::default();
        }
        true
    }
    /// Sample once at/after each second midpoint, before the next midpoint.
    /// This records actual occupancy at polling time, never backfills a missed
    /// period. Duplicate polls do not sample; missed periods invalidate.
    pub fn poll(&self) {
        let Some(mut state) = self.lock() else {
            return;
        };
        let Some((_, mapped)) = self.time(&mut state) else {
            return;
        };
        if !self.advance(&mut state, mapped) {
            return;
        }
        let due =
            state.requests.start + u64::from(state.requests.active.ticks) * SECOND + SECOND / 2;
        if mapped >= due && mapped < due.saturating_add(SECOND) && state.requests.active.ticks < 300
        {
            state.mark_leases();
            let mut occupied = [0_u64; 6];
            for record in &state.records {
                if record.kind == Kind::Lease && !record.terminal {
                    occupied[record.lane as usize] += 1;
                }
            }
            let table = &mut state.requests.active;
            for lane in 0..6 {
                if occupied[lane] > CAPACITIES[lane] {
                    table.valid = false;
                }
                let bucket = OCCUPANCY_BOUNDS.partition_point(|bound| *bound < occupied[lane]);
                increment(&mut table.occupancy[lane].0[bucket], &mut table.valid);
            }
            table.ticks += 1;
        }
    }
    pub fn http(&self, endpoint: Endpoint) -> Observation<'_, C> {
        self.create(Kind::Http, endpoint, QualifiedModel(2), Lane::Control, None)
    }
    pub fn generation(
        &self,
        parent: &Observation<'_, C>,
        model: QualifiedModel,
    ) -> Observation<'_, C> {
        self.child(parent, Kind::Generation, model)
    }
    pub fn delivery(
        &self,
        parent: &Observation<'_, C>,
        model: QualifiedModel,
    ) -> Observation<'_, C> {
        self.child(parent, Kind::Delivery, model)
    }
    fn child(
        &self,
        parent: &Observation<'_, C>,
        kind: Kind,
        model: QualifiedModel,
    ) -> Observation<'_, C> {
        if !std::ptr::eq(self, parent.owner) {
            self.invalidate();
            return self.inert();
        }
        self.create(
            kind,
            Endpoint::Other,
            model,
            Lane::Control,
            Some(parent.key),
        )
    }
    /// Marker only: never owns, acquires, releases or alters an actual permit.
    pub fn lease(&self, lane: Lane) -> Observation<'_, C> {
        self.create(Kind::Lease, Endpoint::Other, QualifiedModel(2), lane, None)
    }
    fn inert(&self) -> Observation<'_, C> {
        Observation {
            owner: self,
            key: 0,
        }
    }
    fn lost(&self, kind: Kind) -> Observation<'_, C> {
        self.invalidate();
        if kind == Kind::Lease {
            self.untracked_lease.store(true, Ordering::Release);
        }
        self.inert()
    }
    fn create(
        &self,
        kind: Kind,
        mut endpoint: Endpoint,
        model: QualifiedModel,
        lane: Lane,
        parent: Option<u64>,
    ) -> Observation<'_, C> {
        let Some(mut state) = self.lock() else {
            return self.lost(kind);
        };
        let time = self.time(&mut state);
        if let Some((_, mapped)) = time {
            self.advance(&mut state, mapped);
        }
        let eligible = time.is_some_and(|(_, mapped)| mapped >= state.requests.eligible)
            && state.epoch == self.epoch.load(Ordering::SeqCst)
            && state.anchor.is_some();
        let parent_eligible = match parent {
            None => true,
            Some(key) => state.record(key).is_some_and(|r| {
                endpoint = r.endpoint;
                r.epoch == state.epoch
                    && !r.terminal
                    && matches!(r.kind, Kind::Http | Kind::Generation)
            }),
        };
        if kind != Kind::Lease && (!eligible || !parent_eligible) {
            return self.inert();
        }
        if matches!(kind, Kind::Generation | Kind::Delivery) && endpoint.chat().is_none() {
            self.invalidate();
            return self.inert();
        }
        let Some(index) = state.records.iter().position(|r| r.kind == Kind::Empty) else {
            return self.lost(kind);
        };
        let Some(serial) = state.serial.checked_add(1).filter(|n| *n < (1 << 56)) else {
            return self.lost(kind);
        };
        state.serial = serial;
        let key = (serial << 8) | index as u64;
        state.records[index] = Record {
            key,
            epoch: if eligible { state.epoch } else { 0 },
            start: time.map_or(0, |(mono, _)| mono),
            kind,
            endpoint,
            model,
            lane,
            ..Record::default()
        };
        let table = &mut state.requests.active;
        match kind {
            Kind::Http => increment(&mut table.http_starts[endpoint as usize], &mut table.valid),
            Kind::Generation => increment(
                &mut table.generation_starts[generation_index(endpoint, model).unwrap()],
                &mut table.valid,
            ),
            Kind::Lease if eligible => state.mark_leases(),
            _ => (),
        }
        Observation { owner: self, key }
    }
    pub fn reject(&self, endpoint: Option<Endpoint>, model: AdmissionModel, reason: Rejection) {
        let Some(mut state) = self.lock() else {
            return;
        };
        let Some((_, mapped)) = self.time(&mut state) else {
            return;
        };
        if !self.advance(&mut state, mapped) || mapped < state.requests.eligible {
            return;
        }
        let pre_router = (reason as usize) < 4;
        if pre_router != endpoint.is_none()
            || (pre_router && model != AdmissionModel::NotApplicable)
            || (endpoint.is_some_and(|e| e.chat().is_none())
                && model != AdmissionModel::NotApplicable)
        {
            state.requests.active.valid = false;
            return;
        }
        let index =
            (endpoint.map_or(16, |e| e as usize) * 5 + model.index()) * 14 + reason as usize;
        let table = &mut state.requests.active;
        increment(&mut table.rejected[index], &mut table.valid);
    }
    fn update(&self, key: u64, update: Update) {
        if key == 0 {
            return;
        }
        let Some(mut state) = self.lock() else {
            return;
        };
        let time = self.time(&mut state);
        // Half-open lease lifetime: release exactly on the boundary must not
        // touch the new window when advance marks still-held old leases.
        if matches!(update, Update::Release)
            && time.is_some_and(|(_, mapped)| mapped % REQUEST_WINDOW == 0)
            && state
                .record(key)
                .is_some_and(|r| r.kind == Kind::Lease && !r.terminal)
        {
            let record = state.record(key).copied().unwrap();
            if time.is_some_and(|(_, mapped)| record.lease_window == mapped) {
                let table = &mut state.requests.active;
                let count = &mut table.contributors[record.lane as usize];
                if let Some(remaining) = count.checked_sub(1) {
                    *count = remaining;
                } else {
                    table.valid = false;
                }
            }
            state.records[key as usize & 255].terminal = true;
        }
        if let Some((_, mapped)) = time {
            self.advance(&mut state, mapped);
        }
        let Some(record) = state.record(key).copied() else {
            return;
        };
        if record.terminal {
            return;
        }
        if matches!(update, Update::Release) && record.kind == Kind::Lease {
            state.mark_leases();
            state.records[key as usize & 255].terminal = true;
            return;
        }
        if record.epoch != state.epoch || state.epoch != self.epoch.load(Ordering::SeqCst) {
            return;
        }
        let Some((mono, mapped)) = time.filter(|(_, mapped)| *mapped >= state.requests.eligible)
        else {
            return;
        };
        if mapped < state.requests.start {
            self.invalidate();
            return;
        }
        let result = state.apply(record, mono, update);
        if !result {
            state.requests.active.valid = false;
        }
    }
    fn release(&self, key: u64) {
        if key == 0 {
            return;
        }
        self.update(key, Update::Release);
        // Slots cannot be reused until the next holder drains this bitmap. Only
        // the non-cloneable owner retires a key; duplicate terminal calls do not.
        let index = key as usize & 255;
        self.retired[index / 64].fetch_or(1 << (index % 64), Ordering::Release);
    }
    /// Synthetic interval ingestion. Call before boundary poll for the last
    /// interval; attempted minutes are never reopened for late/revised input.
    pub fn resource(&self, end_ns: u64, scope: ResourceScope, metric: ResourceMetric, value: f64) {
        self.infrastructure_sample(end_ns, |table, interval| {
            table.observe(scope, metric, interval, value)
        });
    }
    pub fn configured_capacity(&self, end_ns: u64, lane: Lane, capacity: u64) {
        self.infrastructure_sample(end_ns, |table, interval| {
            table.configuration(lane, interval, capacity)
        });
    }
    fn infrastructure_sample(&self, end_ns: u64, observe: impl FnOnce(&mut Infrastructure, usize)) {
        let Some(mut state) = self.lock() else {
            return;
        };
        let Some((_, mapped)) = self.time(&mut state) else {
            return;
        };
        if state
            .infrastructure
            .start
            .checked_add(INFRA_WINDOW)
            .is_none_or(|end| mapped > end)
            && !self.advance(&mut state, mapped)
        {
            return;
        }
        let start = state.infrastructure.start;
        // No implicit closure here: all series at the sixth interval can arrive
        // before the explicit boundary poll. Stale intervals never fill holes.
        if start >= state.infrastructure.eligible
            && end_ns > start
            && Some(end_ns) <= start.checked_add(INFRA_WINDOW)
            && (end_ns - start) % (10 * SECOND) == 0
        {
            observe(
                &mut state.infrastructure.active,
                if mapped == end_ns {
                    ((end_ns - start) / (10 * SECOND) - 1) as usize
                } else {
                    6
                },
            );
        }
    }
    /// Local immutable inspection, not a send permit. Packet 2C must coordinate
    /// in-flight cancellation with the epoch and may not retain a borrowed view.
    pub fn request(&self) -> Option<RequestView<'_, C>> {
        self.poll();
        let guard = self.lock()?;
        let (window, epoch) = guard.requests.pending?;
        self.eligible_token(epoch).then_some(RequestView {
            owner: self,
            guard,
            window,
            epoch,
        })
    }
    pub fn infrastructure(&self) -> Option<InfrastructureView<'_, C>> {
        self.poll();
        let guard = self.lock()?;
        let (window, epoch) = guard.infrastructure.pending?;
        self.eligible_token(epoch).then_some(InfrastructureView {
            owner: self,
            guard,
            window,
            epoch,
        })
    }
}

impl State {
    fn restart(&mut self, now: ClockReading, epoch: u64) -> bool {
        if self.requests.reset(now.wall_ns, REQUEST_WINDOW).is_none()
            || self
                .infrastructure
                .reset(now.wall_ns, INFRA_WINDOW)
                .is_none()
        {
            return false;
        }
        // Off/re-enable may revisit a never-attempted grid interval after a
        // backward clock reset. A prior epoch's lease marker is not a fresh
        // contributor, even when its numeric window start happens to match.
        for record in &mut self.records {
            record.lease_window = u64::MAX;
        }
        self.epoch = epoch;
        self.anchor = Some(now);
        self.last = Some(now);
        true
    }
    fn check_clock(&mut self, now: ClockReading) -> Option<(u64, u64)> {
        let anchor = self.anchor?;
        let last = self.last?;
        if now.monotonic_ns < last.monotonic_ns || now.wall_ns < last.wall_ns {
            return None;
        }
        let elapsed = now.monotonic_ns.checked_sub(anchor.monotonic_ns)?;
        let mapped = anchor.wall_ns.checked_add(elapsed)?;
        if now.wall_ns.abs_diff(mapped) > SECOND {
            return None;
        }
        self.last = Some(now);
        Some((now.monotonic_ns, mapped))
    }
    fn record(&self, key: u64) -> Option<&Record> {
        let record = &self.records[key as usize & 255];
        (key != 0 && record.key == key && record.kind != Kind::Empty).then_some(record)
    }
    fn mark_leases(&mut self) {
        if self.requests.start < self.requests.eligible {
            return;
        }
        let mut occupied = [0_u64; 6];
        for record in &mut self.records {
            if record.kind == Kind::Lease && !record.terminal {
                occupied[record.lane as usize] += 1;
            }
            if record.kind == Kind::Lease
                && !record.terminal
                && record.lease_window != self.requests.start
            {
                increment(
                    &mut self.requests.active.contributors[record.lane as usize],
                    &mut self.requests.active.valid,
                );
                record.lease_window = self.requests.start;
            }
        }
        if occupied
            .iter()
            .zip(CAPACITIES)
            .any(|(used, capacity)| *used > capacity)
        {
            self.requests.active.valid = false;
        }
    }
    fn apply(&mut self, record: Record, mono: u64, update: Update) -> bool {
        let index = record.key as usize & 255;
        let table = &mut self.requests.active;
        match (record.kind, update) {
            (Kind::Http, Update::Disposition(disposition)) => {
                self.records[index].disposition = disposition;
            }
            (Kind::Http, Update::HttpSelected(status, terminal)) => {
                return self.apply(
                    record,
                    mono,
                    Update::Http(status, terminal, record.disposition),
                );
            }
            (Kind::Generation, Update::Dispatch) => {
                if record.dispatch.is_none() && record.first.is_none() {
                    self.records[index].dispatch = Some(mono);
                }
            }
            (Kind::Generation, Update::First) => {
                if record.first.is_none() {
                    let Some(bucket) = record
                        .dispatch
                        .and_then(|start| duration_bucket(start, mono))
                    else {
                        return false;
                    };
                    self.records[index].first = Some(bucket);
                    // Discard dispatch once quantized; no exact first-output time.
                    self.records[index].dispatch = None;
                }
            }
            (Kind::Http, Update::Http(status, terminal, disposition)) => {
                if record.endpoint.chat().is_none()
                    && matches!(
                        disposition,
                        Disposition::NewGeneration | Disposition::Duplicate
                    )
                {
                    return false;
                }
                let Some(bucket) = duration_bucket(record.start, mono) else {
                    return false;
                };
                let cell = http_index(record.endpoint, status, terminal);
                increment(&mut table.http_completed[cell], &mut table.valid);
                increment(
                    &mut table.http_duration[cell].0[bucket as usize],
                    &mut table.valid,
                );
                increment(
                    &mut table.dispositions[cell][disposition as usize],
                    &mut table.valid,
                );
                self.records[index].terminal = true;
            }
            (Kind::Generation, Update::Generation(terminal)) => {
                let Some(bucket) = duration_bucket(record.start, mono) else {
                    return false;
                };
                let cell = generation_index(record.endpoint, record.model).unwrap() * 12
                    + terminal as usize;
                increment(&mut table.generation_completed[cell], &mut table.valid);
                increment(
                    &mut table.generation_duration[cell].0[bucket as usize],
                    &mut table.valid,
                );
                if let Some(first) = record.first {
                    increment(
                        &mut table.first_output[cell].0[first as usize],
                        &mut table.valid,
                    );
                }
                self.records[index].terminal = true;
            }
            (Kind::Delivery, Update::Delivery(terminal)) => {
                let cell = generation_index(record.endpoint, record.model).unwrap() * 3
                    + terminal as usize;
                increment(&mut table.delivery[cell], &mut table.valid);
                self.records[index].terminal = true;
            }
            (_, Update::Release) => {
                return false;
            } // lost eligible lifecycle, not a fabricated terminal
            _ => return false,
        }
        if self.records[index].terminal {
            self.records[index].start = 0;
            self.records[index].dispatch = None;
            self.records[index].first = None;
        }
        true
    }
}
enum Update {
    Disposition(Disposition),
    HttpSelected(Status, HttpTerminal),
    Dispatch,
    First,
    Http(Status, HttpTerminal, Disposition),
    Generation(GenerationTerminal),
    Delivery(DeliveryTerminal),
    Release,
}

/// Non-cloneable 16-byte pool handle; no request data or permit ownership.
pub struct Observation<'a, C: Clock = SystemClock> {
    owner: &'a AggregateMetrics<C>,
    key: u64,
}
impl<C: Clock> Observation<'_, C> {
    pub fn dispatch(&mut self) {
        self.owner.update(self.key, Update::Dispatch);
    }
    pub fn first_output(&mut self) {
        self.owner.update(self.key, Update::First);
    }
    pub fn finish_http(
        &mut self,
        status: Status,
        terminal: HttpTerminal,
        disposition: Disposition,
    ) {
        self.owner
            .update(self.key, Update::Http(status, terminal, disposition));
    }
    pub fn finish_generation(&mut self, terminal: GenerationTerminal) {
        self.owner.update(self.key, Update::Generation(terminal));
    }
    pub fn finish_delivery(&mut self, terminal: DeliveryTerminal) {
        self.owner.update(self.key, Update::Delivery(terminal));
    }
}
impl<C: Clock> Drop for Observation<'_, C> {
    fn drop(&mut self) {
        self.owner.release(self.key);
    }
}

/// Arc-backed linear owner for detached application lifetimes. Conversion moves
/// the original pool key; it does not start another observation.
pub struct OwnedObservation<C: Clock = SystemClock> {
    owner: std::sync::Arc<AggregateMetrics<C>>,
    key: u64,
}
impl<C: Clock> Observation<'_, C> {
    pub fn into_owned(
        mut self,
        owner: &std::sync::Arc<AggregateMetrics<C>>,
    ) -> OwnedObservation<C> {
        let key = if std::ptr::eq(self.owner, owner.as_ref()) {
            std::mem::take(&mut self.key)
        } else {
            owner.invalidate();
            0
        };
        OwnedObservation {
            owner: owner.clone(),
            key,
        }
    }
}
impl<C: Clock> OwnedObservation<C> {
    fn update(&mut self, update: Update) {
        self.owner.update(self.key, update);
    }
    pub fn dispatch(&mut self) {
        self.update(Update::Dispatch);
    }
    pub fn first_output(&mut self) {
        self.update(Update::First);
    }
    pub fn finish_generation(&mut self, terminal: GenerationTerminal) {
        self.update(Update::Generation(terminal));
    }
    pub fn finish_delivery(&mut self, terminal: DeliveryTerminal) {
        self.update(Update::Delivery(terminal));
    }
    pub(crate) fn delivery(&self, model: QualifiedModel) -> Self {
        self.owner
            .create(
                Kind::Delivery,
                Endpoint::Other,
                model,
                Lane::Control,
                Some(self.key),
            )
            .into_owned(&self.owner)
    }
}
impl<C: Clock> Drop for OwnedObservation<C> {
    fn drop(&mut self) {
        self.owner.release(self.key);
    }
}

pub struct RequestView<'a, C: Clock> {
    owner: &'a AggregateMetrics<C>,
    guard: StateGuard<'a, C>,
    window: Window,
    epoch: u64,
}
impl<C: Clock> RequestView<'_, C> {
    pub fn window(&self) -> Window {
        self.window
    }
    pub fn tables(&self) -> Option<&RequestTables> {
        self.owner
            .eligible_token(self.epoch)
            .then(|| self.guard.requests.frozen.as_deref())
            .flatten()
    }
}
impl<C: Clock> Drop for RequestView<'_, C> {
    fn drop(&mut self) {
        self.guard.requests.pending = None;
        **self.guard.requests.frozen.as_mut().unwrap() = RequestTables::default();
    }
}
pub struct InfrastructureView<'a, C: Clock> {
    owner: &'a AggregateMetrics<C>,
    guard: StateGuard<'a, C>,
    window: Window,
    epoch: u64,
}
impl<C: Clock> InfrastructureView<'_, C> {
    pub fn window(&self) -> Window {
        self.window
    }
    pub fn tables(&self) -> Option<&Infrastructure> {
        self.owner
            .eligible_token(self.epoch)
            .then(|| self.guard.infrastructure.frozen.as_deref())
            .flatten()
    }
}
impl<C: Clock> Drop for InfrastructureView<'_, C> {
    fn drop(&mut self) {
        self.guard.infrastructure.pending = None;
        **self.guard.infrastructure.frozen.as_mut().unwrap() = Infrastructure::default();
    }
}

// One nonblocking state owner. Entrants cannot acquire the mutex until this
// guard has disposed poisoned state AND released it; no post-unlock dirty gap.
struct StateGuard<'a, C: Clock> {
    owner: &'a AggregateMetrics<C>,
    guard: Option<MutexGuard<'a, State>>,
}
impl<C: Clock> std::ops::Deref for StateGuard<'_, C> {
    type Target = State;
    fn deref(&self) -> &State {
        self.guard.as_deref().unwrap()
    }
}
impl<C: Clock> std::ops::DerefMut for StateGuard<'_, C> {
    fn deref_mut(&mut self) -> &mut State {
        self.guard.as_deref_mut().unwrap()
    }
}
impl<C: Clock> Drop for StateGuard<'_, C> {
    fn drop(&mut self) {
        if !self.owner.eligible_token(self.epoch) {
            self.owner.invalidate();
            self.requests.discard();
            self.infrastructure.discard();
            self.anchor = None;
        }
        drop(self.guard.take());
        if self.owner.entrants.fetch_sub(1, Ordering::SeqCst) != 1 {
            self.owner.invalidate();
        }
    }
}

mod export;
mod handoff;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod lifecycle_tests;
