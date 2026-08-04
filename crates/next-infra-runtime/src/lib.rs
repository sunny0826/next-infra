//! Tauri-independent runtime composition boundary for Next Infra.
//!
//! The runtime owns lifecycle admission and deterministic scheduling decisions.
//! IO, timers, and threads stay behind injected traits so Desktop Host tests can
//! exercise the same state machine without starting Tauri.

use next_infra_core::{
    CommitResult, Connection, ConnectionId, Fingerprint, MissingEvidenceState, Relation,
    RelationId, Resource, ResourceId, Scope, StoreReader, StoreWriter, SyncCommit, SyncCursor,
    SyncRun, SyncRunId, Timestamp,
};
use next_infra_query::service::QueryService;
use next_infra_store::{Store, StoreError};
use next_infra_sync::SyncEngine;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

mod query_source;
pub use query_source::*;

const MAX_TIMESTAMP_MILLIS: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    Interactive,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    New,
    Starting(RuntimeMode),
    Running(RuntimeMode),
    Sleeping(RuntimeMode),
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupReport {
    pub mode: RuntimeMode,
    pub interrupted_runs: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    pub writer_drained: bool,
    pub store_checkpointed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeError<E> {
    InvalidState {
        operation: &'static str,
        state: RuntimeState,
    },
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for RuntimeError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState { operation, state } => {
                write!(formatter, "cannot {operation} while runtime is {state:?}")
            }
            Self::Backend(error) => write!(formatter, "runtime backend error: {error}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for RuntimeError<E> {}

/// Runtime-owned lifecycle hooks for recovery and graceful shutdown.
pub trait RuntimeBackend {
    type Error;

    /// Mark any persisted running runs interrupted before admission opens.
    fn recover_startup(&mut self, at: Timestamp) -> Result<usize, Self::Error>;

    /// Stop accepting work at the runtime boundary, then drain queued writes.
    fn drain_writer(&mut self) -> Result<(), Self::Error>;

    /// Checkpoint the store after the writer has drained.
    fn checkpoint_store(&mut self) -> Result<(), Self::Error>;
}

/// Concrete single-owner backend used by the Desktop composition.
///
/// The SyncEngine owns the only WriterQueue and SQLite write connection.
/// QueryService remains a separate Runtime handle so Desktop and MCP adapters
/// consume the same bounded query semantics without gaining write access.
#[derive(Clone)]
pub struct SharedStore {
    inner: Arc<Mutex<Store>>,
}

impl SharedStore {
    /// Open the single SQLite owner and wrap it in a cloneable shared handle.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Store::open(path).map(Self::new)
    }

    pub fn new(store: Store) -> Self {
        Self {
            inner: Arc::new(Mutex::new(store)),
        }
    }

    pub fn read<T>(
        &self,
        read: impl FnOnce(&Store) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let store = self.lock()?;
        read(&store)
    }

    pub fn write<T>(
        &self,
        write: impl FnOnce(&mut Store) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut store = self.lock()?;
        write(&mut store)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Store>, StoreError> {
        self.inner
            .lock()
            .map_err(|_| StoreError::Contract("shared store lock is poisoned".into()))
    }
}

impl StoreReader for SharedStore {
    type Error = StoreError;

    fn get_connection(&self, id: &ConnectionId) -> Result<Option<Connection>, Self::Error> {
        self.read(|store| store.get_connection(id))
    }

    fn get_resource(&self, id: &ResourceId) -> Result<Option<Resource>, Self::Error> {
        self.read(|store| store.get_resource(id))
    }

    fn get_relation(&self, id: &RelationId) -> Result<Option<Relation>, Self::Error> {
        self.read(|store| store.get_relation(id))
    }

    fn latest_relation_version_fingerprint(
        &self,
        id: &RelationId,
    ) -> Result<Option<Fingerprint>, Self::Error> {
        self.read(|store| store.latest_relation_version_fingerprint(id))
    }

    fn get_sync_run(&self, id: &SyncRunId) -> Result<Option<SyncRun>, Self::Error> {
        self.read(|store| store.get_sync_run(id))
    }

    fn sync_cursor(&self, connection_id: &ConnectionId) -> Result<Option<SyncCursor>, Self::Error> {
        self.read(|store| store.sync_cursor(connection_id))
    }

    fn list_resources_for_scope(
        &self,
        connection_id: &ConnectionId,
        scope: &Scope,
    ) -> Result<Vec<Resource>, Self::Error> {
        self.read(|store| store.list_resources_for_scope(connection_id, scope))
    }

    fn missing_evidence_state(
        &self,
        connection_id: &ConnectionId,
        scope: &Scope,
    ) -> Result<Option<MissingEvidenceState>, Self::Error> {
        self.read(|store| store.missing_evidence_state(connection_id, scope))
    }
}

impl StoreWriter for SharedStore {
    type Error = StoreError;

    fn upsert_connection(&mut self, connection: Connection) -> Result<(), Self::Error> {
        self.write(|store| store.upsert_connection(connection))
    }

    fn start_sync_run(&mut self, sync_run: SyncRun) -> Result<(), Self::Error> {
        self.write(|store| store.start_sync_run(sync_run))
    }

    fn commit_sync(&mut self, commit: SyncCommit) -> Result<CommitResult, Self::Error> {
        self.write(|store| store.commit_sync(commit))
    }

    fn mark_running_syncs_interrupted(&mut self, at: Timestamp) -> Result<usize, Self::Error> {
        self.write(|store| store.mark_running_syncs_interrupted(at))
    }
}

pub struct SqliteRuntimeBackend {
    sync_engine: SyncEngine<SharedStore>,
}

impl SqliteRuntimeBackend {
    pub fn new(store: Store) -> Self {
        Self::from_shared_store(SharedStore::new(store))
    }

    pub fn from_shared_store(store: SharedStore) -> Self {
        Self {
            sync_engine: SyncEngine::new(store),
        }
    }

    pub fn shared_store(&self) -> SharedStore {
        self.sync_engine.writer().store().clone()
    }

    pub fn sync_engine(&self) -> &SyncEngine<SharedStore> {
        &self.sync_engine
    }

    pub fn sync_engine_mut(&mut self) -> &mut SyncEngine<SharedStore> {
        &mut self.sync_engine
    }
}

impl RuntimeBackend for SqliteRuntimeBackend {
    type Error = StoreError;

    fn recover_startup(&mut self, at: Timestamp) -> Result<usize, Self::Error> {
        use next_infra_core::StoreWriter;
        self.sync_engine
            .writer_mut()
            .store_mut()
            .mark_running_syncs_interrupted(at)
    }

    fn drain_writer(&mut self) -> Result<(), Self::Error> {
        self.sync_engine.writer_mut().flush().map(|_| ())
    }

    fn checkpoint_store(&mut self) -> Result<(), Self::Error> {
        self.sync_engine
            .writer()
            .store()
            .read(Store::checkpoint_wal)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleEntry {
    pub connection_id: ConnectionId,
    pub interval_millis: u64,
    pub next_due_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledSync {
    pub connection_id: ConnectionId,
    pub scheduled_at: Timestamp,
    pub run_at: Timestamp,
    pub catch_up: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulerError {
    InvalidInterval,
}

/// Deterministic scheduler state. Each due evaluation emits at most one run
/// per connection and moves the next due time after the evaluation time, so a
/// long sleep never replays every missed interval.
#[derive(Clone, Debug, Default)]
pub struct Scheduler {
    entries: BTreeMap<ConnectionId, ScheduleEntry>,
}

impl Scheduler {
    pub fn register(
        &mut self,
        connection_id: ConnectionId,
        interval_millis: u64,
        next_due_at: Timestamp,
    ) -> Result<(), SchedulerError> {
        if interval_millis == 0 {
            return Err(SchedulerError::InvalidInterval);
        }
        self.entries.insert(
            connection_id.clone(),
            ScheduleEntry {
                connection_id,
                interval_millis,
                next_due_at,
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, connection_id: &ConnectionId) -> Option<ScheduleEntry> {
        self.entries.remove(connection_id)
    }

    pub fn entry(&self, connection_id: &ConnectionId) -> Option<&ScheduleEntry> {
        self.entries.get(connection_id)
    }

    pub fn entries(&self) -> impl Iterator<Item = &ScheduleEntry> {
        self.entries.values()
    }

    /// Plan normal scheduler work at `now`.
    pub fn plan_due(&mut self, now: Timestamp) -> Vec<ScheduledSync> {
        self.plan(now, false)
    }

    /// Plan wake catch-up work. A connection can contribute at most one plan,
    /// regardless of how many intervals elapsed while the device slept.
    pub fn plan_wake_catch_up(&mut self, now: Timestamp) -> Vec<ScheduledSync> {
        self.plan(now, true)
    }

    fn plan(&mut self, now: Timestamp, catch_up: bool) -> Vec<ScheduledSync> {
        let mut plans = Vec::new();
        for entry in self.entries.values_mut() {
            if now < entry.next_due_at {
                continue;
            }
            plans.push(ScheduledSync {
                connection_id: entry.connection_id.clone(),
                scheduled_at: entry.next_due_at,
                run_at: now,
                catch_up,
            });
            entry.next_due_at = add_interval(now, entry.interval_millis);
        }
        plans
    }
}

fn add_interval(at: Timestamp, interval_millis: u64) -> Timestamp {
    let interval = interval_millis.min(MAX_TIMESTAMP_MILLIS) as i64;
    Timestamp::from_unix_millis(at.unix_millis().saturating_add(interval))
        .expect("timestamp addition preserves non-negative value")
}

/// The single Tauri-independent Control Plane Runtime state machine.
pub struct Runtime<B, Q> {
    backend: B,
    query: QueryService<Q>,
    scheduler: Scheduler,
    state: RuntimeState,
    interrupted_runs: usize,
}

impl<B, Q> Runtime<B, Q>
where
    B: RuntimeBackend,
{
    pub fn new(backend: B, query: QueryService<Q>, scheduler: Scheduler) -> Self {
        Self {
            backend,
            query,
            scheduler,
            state: RuntimeState::New,
            interrupted_runs: 0,
        }
    }

    pub fn state(&self) -> RuntimeState {
        self.state
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn query(&self) -> &QueryService<Q> {
        &self.query
    }

    pub fn query_mut(&mut self) -> &mut QueryService<Q> {
        &mut self.query
    }

    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    pub fn interrupted_runs(&self) -> usize {
        self.interrupted_runs
    }

    pub fn start(
        &mut self,
        mode: RuntimeMode,
        at: Timestamp,
    ) -> Result<StartupReport, RuntimeError<B::Error>> {
        if self.state != RuntimeState::New {
            return Err(RuntimeError::InvalidState {
                operation: "start",
                state: self.state,
            });
        }
        self.state = RuntimeState::Starting(mode);
        match self.backend.recover_startup(at) {
            Ok(interrupted_runs) => {
                self.interrupted_runs = interrupted_runs;
                self.state = RuntimeState::Running(mode);
                Ok(StartupReport {
                    mode,
                    interrupted_runs,
                })
            }
            Err(error) => {
                self.state = RuntimeState::Failed;
                Err(RuntimeError::Backend(error))
            }
        }
    }

    pub fn start_interactive(
        &mut self,
        at: Timestamp,
    ) -> Result<StartupReport, RuntimeError<B::Error>> {
        self.start(RuntimeMode::Interactive, at)
    }

    pub fn start_background(
        &mut self,
        at: Timestamp,
    ) -> Result<StartupReport, RuntimeError<B::Error>> {
        self.start(RuntimeMode::Background, at)
    }

    pub fn ensure_admission(&self) -> Result<(), RuntimeError<B::Error>> {
        if matches!(self.state, RuntimeState::Running(_)) {
            Ok(())
        } else {
            Err(RuntimeError::InvalidState {
                operation: "admit sync",
                state: self.state,
            })
        }
    }

    pub fn plan_due(
        &mut self,
        now: Timestamp,
    ) -> Result<Vec<ScheduledSync>, RuntimeError<B::Error>> {
        self.ensure_admission()?;
        Ok(self.scheduler.plan_due(now))
    }

    pub fn sleep(&mut self) -> Result<(), RuntimeError<B::Error>> {
        let RuntimeState::Running(mode) = self.state else {
            return Err(RuntimeError::InvalidState {
                operation: "sleep",
                state: self.state,
            });
        };
        self.state = RuntimeState::Sleeping(mode);
        Ok(())
    }

    pub fn wake(&mut self, now: Timestamp) -> Result<Vec<ScheduledSync>, RuntimeError<B::Error>> {
        let RuntimeState::Sleeping(mode) = self.state else {
            return Err(RuntimeError::InvalidState {
                operation: "wake",
                state: self.state,
            });
        };
        self.state = RuntimeState::Running(mode);
        Ok(self.scheduler.plan_wake_catch_up(now))
    }

    pub fn stop(&mut self) -> Result<ShutdownReport, RuntimeError<B::Error>> {
        if !matches!(
            self.state,
            RuntimeState::Running(_) | RuntimeState::Sleeping(_)
        ) {
            return Err(RuntimeError::InvalidState {
                operation: "stop",
                state: self.state,
            });
        }
        self.state = RuntimeState::Stopping;
        if let Err(error) = self.backend.drain_writer() {
            self.state = RuntimeState::Failed;
            return Err(RuntimeError::Backend(error));
        }
        if let Err(error) = self.backend.checkpoint_store() {
            self.state = RuntimeState::Failed;
            return Err(RuntimeError::Backend(error));
        }
        self.state = RuntimeState::Stopped;
        Ok(ShutdownReport {
            writer_drained: true,
            store_checkpointed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_core::{
        Connection, ConnectorHealth, ConnectorType, DOMAIN_SCHEMA_VERSION, Scope, StoreReader,
        StoreWriter, SyncCommit, SyncCoverage, SyncMode, SyncRun, SyncRunCounts, SyncRunId,
        SyncRunStatus, SyncTrigger,
    };
    use next_infra_query::service::QueryService;
    use serde_json::json;

    #[derive(Default)]
    struct FakeBackend {
        events: Vec<&'static str>,
        interrupted_runs: usize,
        fail_recovery: bool,
        fail_drain: bool,
        fail_checkpoint: bool,
    }

    impl RuntimeBackend for FakeBackend {
        type Error = &'static str;

        fn recover_startup(&mut self, _at: Timestamp) -> Result<usize, Self::Error> {
            self.events.push("recover");
            if self.fail_recovery {
                Err("recovery failed")
            } else {
                Ok(self.interrupted_runs)
            }
        }

        fn drain_writer(&mut self) -> Result<(), Self::Error> {
            self.events.push("drain");
            if self.fail_drain {
                Err("drain failed")
            } else {
                Ok(())
            }
        }

        fn checkpoint_store(&mut self) -> Result<(), Self::Error> {
            self.events.push("checkpoint");
            if self.fail_checkpoint {
                Err("checkpoint failed")
            } else {
                Ok(())
            }
        }
    }

    fn timestamp(value: i64) -> Timestamp {
        Timestamp::from_unix_millis(value).unwrap()
    }

    fn connection_id(value: &str) -> ConnectionId {
        ConnectionId::new(value).unwrap()
    }

    fn runtime(backend: FakeBackend, scheduler: Scheduler) -> Runtime<FakeBackend, ()> {
        Runtime::new(backend, QueryService::new(()), scheduler)
    }

    #[test]
    fn startup_runs_recovery_before_admission_for_both_modes() {
        let mut interactive = runtime(
            FakeBackend {
                interrupted_runs: 2,
                ..FakeBackend::default()
            },
            Scheduler::default(),
        );
        let report = interactive.start_interactive(timestamp(1)).unwrap();
        assert_eq!(report.mode, RuntimeMode::Interactive);
        assert_eq!(report.interrupted_runs, 2);
        assert_eq!(
            interactive.state(),
            RuntimeState::Running(RuntimeMode::Interactive)
        );
        assert_eq!(interactive.backend().events, ["recover"]);
        interactive.ensure_admission().unwrap();

        let mut background = runtime(FakeBackend::default(), Scheduler::default());
        background.start_background(timestamp(1)).unwrap();
        assert_eq!(
            background.state(),
            RuntimeState::Running(RuntimeMode::Background)
        );
    }

    #[test]
    fn recovery_failure_blocks_runtime_start() {
        let mut runtime = runtime(
            FakeBackend {
                fail_recovery: true,
                ..FakeBackend::default()
            },
            Scheduler::default(),
        );
        assert_eq!(
            runtime.start_interactive(timestamp(1)),
            Err(RuntimeError::Backend("recovery failed"))
        );
        assert_eq!(runtime.state(), RuntimeState::Failed);
        assert!(runtime.ensure_admission().is_err());
    }

    #[test]
    fn scheduler_orders_connections_and_bounds_wake_catch_up() {
        let first = connection_id("fixture-a");
        let second = connection_id("fixture-b");
        let mut scheduler = Scheduler::default();
        scheduler
            .register(first.clone(), 100, timestamp(100))
            .unwrap();
        scheduler
            .register(second.clone(), 200, timestamp(100))
            .unwrap();

        let due = scheduler.plan_due(timestamp(100));
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].connection_id, first);
        assert!(!due[0].catch_up);
        assert_eq!(due[0].run_at, timestamp(100));

        let wake = scheduler.plan_wake_catch_up(timestamp(1_000));
        assert_eq!(wake.len(), 2);
        assert!(wake.iter().all(|plan| plan.catch_up));
        assert_eq!(
            scheduler
                .entry(&connection_id("fixture-a"))
                .unwrap()
                .next_due_at,
            timestamp(1_100)
        );
        assert_eq!(
            scheduler
                .entry(&connection_id("fixture-b"))
                .unwrap()
                .next_due_at,
            timestamp(1_200)
        );
        assert!(scheduler.plan_wake_catch_up(timestamp(1_050)).is_empty());
    }

    #[test]
    fn scheduler_order_is_stable_across_registration_order() {
        let first = connection_id("fixture-a");
        let second = connection_id("fixture-b");

        let mut forward = Scheduler::default();
        forward
            .register(first.clone(), 100, timestamp(100))
            .unwrap();
        forward
            .register(second.clone(), 200, timestamp(100))
            .unwrap();

        let mut reverse = Scheduler::default();
        reverse.register(second, 200, timestamp(100)).unwrap();
        reverse.register(first, 100, timestamp(100)).unwrap();

        assert_eq!(
            forward.plan_due(timestamp(100)),
            reverse.plan_due(timestamp(100))
        );
    }

    #[test]
    fn sleep_wake_and_stop_preserve_admission_and_ordering() {
        let mut runtime = runtime(FakeBackend::default(), Scheduler::default());
        runtime.start_interactive(timestamp(1)).unwrap();
        runtime.sleep().unwrap();
        assert_eq!(
            runtime.state(),
            RuntimeState::Sleeping(RuntimeMode::Interactive)
        );
        assert!(runtime.ensure_admission().is_err());

        runtime.wake(timestamp(2)).unwrap();
        runtime.ensure_admission().unwrap();
        let report = runtime.stop().unwrap();
        assert!(report.writer_drained);
        assert!(report.store_checkpointed);
        assert_eq!(runtime.state(), RuntimeState::Stopped);
        assert_eq!(runtime.backend().events, ["recover", "drain", "checkpoint"]);
        assert!(runtime.ensure_admission().is_err());
        assert!(runtime.stop().is_err());
    }

    #[test]
    fn stop_failure_never_reports_checkpoint_as_complete() {
        let mut runtime = runtime(
            FakeBackend {
                fail_drain: true,
                ..FakeBackend::default()
            },
            Scheduler::default(),
        );
        runtime.start_interactive(timestamp(1)).unwrap();
        assert_eq!(runtime.stop(), Err(RuntimeError::Backend("drain failed")));
        assert_eq!(runtime.state(), RuntimeState::Failed);
        assert_eq!(runtime.backend().events, ["recover", "drain"]);
    }

    #[test]
    fn checkpoint_failure_happens_after_drain_and_marks_runtime_failed() {
        let mut runtime = runtime(
            FakeBackend {
                fail_checkpoint: true,
                ..FakeBackend::default()
            },
            Scheduler::default(),
        );
        runtime.start_interactive(timestamp(1)).unwrap();

        assert_eq!(
            runtime.stop(),
            Err(RuntimeError::Backend("checkpoint failed"))
        );
        assert_eq!(runtime.state(), RuntimeState::Failed);
        assert_eq!(runtime.backend().events, ["recover", "drain", "checkpoint"]);
        assert!(runtime.ensure_admission().is_err());
    }

    #[test]
    fn invalid_scheduler_interval_is_rejected() {
        let mut scheduler = Scheduler::default();
        assert_eq!(
            scheduler.register(connection_id("fixture"), 0, timestamp(1)),
            Err(SchedulerError::InvalidInterval)
        );
    }

    fn fixture_connection() -> Connection {
        Connection {
            connection_id: connection_id("fixture-runtime-connection"),
            connector_type: ConnectorType::new("fixture").unwrap(),
            display_name: "Fixture Runtime Connection".into(),
            enabled: true,
            config: json!({}),
            secret_ref: None,
            health: ConnectorHealth::Healthy,
            last_success_at: None,
            last_attempt_at: None,
            config_schema_version: DOMAIN_SCHEMA_VERSION,
            deleted_at: None,
        }
    }

    fn fixture_run(status: SyncRunStatus, finished_at: Option<Timestamp>) -> SyncRun {
        SyncRun {
            sync_run_id: SyncRunId::new("fixture-runtime-run").unwrap(),
            connection_id: connection_id("fixture-runtime-connection"),
            mode: SyncMode::Full,
            trigger: SyncTrigger::User,
            started_at: timestamp(1),
            finished_at,
            status,
            coverage: SyncCoverage::AuthoritativeFull {
                scope: Scope::new("fixture-runtime-scope").unwrap(),
            },
            cursor_before: None,
            cursor_after: None,
            counts: SyncRunCounts::default(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn sqlite_backend_recovers_drains_and_checkpoints_the_real_single_writer() {
        let directory = tempfile::TempDir::new().unwrap();
        let database = directory.path().join("data").join("runtime.db");
        let mut store = Store::open(&database).unwrap();
        store.upsert_connection(fixture_connection()).unwrap();
        store
            .start_sync_run(fixture_run(SyncRunStatus::Running, None))
            .unwrap();

        let shared_store = SharedStore::new(store);
        let backend = SqliteRuntimeBackend::from_shared_store(shared_store.clone());
        let mut runtime = Runtime::new(backend, QueryService::new(()), Scheduler::default());
        let startup = runtime.start_background(timestamp(2)).unwrap();
        assert_eq!(startup.interrupted_runs, 1);

        let recovered = shared_store
            .get_sync_run(&SyncRunId::new("fixture-runtime-run").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, SyncRunStatus::Interrupted);

        runtime
            .backend_mut()
            .sync_engine_mut()
            .writer_mut()
            .enqueue(SyncCommit {
                sync_run: fixture_run(SyncRunStatus::Succeeded, Some(timestamp(3))),
                resources: Vec::new(),
                resource_versions: Vec::new(),
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: None,
                missing_evidence: None,
            });

        let report = runtime.stop().unwrap();
        assert!(report.writer_drained);
        assert!(report.store_checkpointed);
        assert_eq!(runtime.backend().sync_engine().writer().pending_len(), 0);
        let committed = shared_store
            .get_sync_run(&SyncRunId::new("fixture-runtime-run").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(committed.status, SyncRunStatus::Succeeded);
    }
}
