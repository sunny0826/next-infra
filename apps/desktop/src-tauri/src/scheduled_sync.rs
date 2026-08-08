//! Scheduled sync driver for Desktop Host.
//!
//! This module provides a testable driver that periodically evaluates due
//! scheduled syncs and dispatches them through the existing bounded sync path.
//!
//! Architecture (per plan §2.2-2.3):
//! - Pure-logic `ScheduledSyncDriver` layer with injectable clock, resolve,
//!   and enqueue closures — fully unit-testable without Tauri or network.
//! - Tauri wrapper layer: a std thread that ticks every TICK_MILLIS ms,
//!   calling the pure driver and dispatching via the real AppState enqueue path.

use next_infra_connector_github::{GitHubConnector, ReqwestGitHubTransport};
use next_infra_connector_ssh::{OpenSshClient, SshConnector};
use next_infra_core::{Connection, ConnectionId, SyncTrigger, Timestamp};
use next_infra_runtime::{Runtime, ScheduledSync};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Driver tick interval: 10 seconds (plan §2.3).
pub const TICK_MILLIS: u64 = 10_000;

/// Errors returned by the enqueue callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnqueueError {
    /// Another sync for this connection is already running.
    SyncInProgress,
    /// Connection or secrets are unavailable.
    Unavailable,
}

/// Handle to stop the driver thread and wait for it to join.
pub struct DriverHandle {
    pub(crate) stop: std::sync::mpsc::Sender<()>,
    pub(crate) join: Option<std::thread::JoinHandle<()>>,
}

impl DriverHandle {
    /// Signal the driver to stop and wait for it to terminate.
    /// Does not block longer than one tick interval.
    pub fn stop(self) {
        let _ = self.stop.send(());
        if let Some(join) = self.join {
            let _ = join.join();
        }
    }
}

/// Pure-logic driver that evaluates due scheduled syncs and dispatches them.
///
/// Type parameters:
/// - `Resolve`: connection resolver, `Fn(&ConnectionId) -> Option<Connection>`
/// - `Enqueue`: enqueue callback, `FnMut(Connection, SyncTrigger) -> Result<(), EnqueueError>`
///
/// Time is passed per-tick via `at`, so tests can drive deterministic ticks.
pub struct ScheduledSyncDriver<Resolve, Enqueue> {
    resolve: Resolve,
    enqueue: Enqueue,
}

impl<Resolve, Enqueue> ScheduledSyncDriver<Resolve, Enqueue>
where
    Resolve: Fn(&ConnectionId) -> Option<Connection>,
    Enqueue: FnMut(Connection, SyncTrigger) -> Result<(), EnqueueError>,
{
    /// Construct a driver with injectable dependencies.
    pub fn new(resolve: Resolve, enqueue: Enqueue) -> Self {
        Self { resolve, enqueue }
    }

    /// Drive one tick: consume pending catch-up plans, then evaluate due plans.
    ///
    /// Errors from `runtime.plan_due` are tolerated when the runtime is Sleeping
    /// (returns `InvalidState`) — in that case the tick is a no-op.
    pub fn tick<Q>(
        &mut self,
        runtime: &mut Runtime<next_infra_runtime::SqliteRuntimeBackend, Q>,
        pending: &mut VecDeque<ScheduledSync>,
        at: Timestamp,
    ) {
        // 1. Drain pending (wake catch-up) plans into local list.
        let mut plans: Vec<ScheduledSync> = pending.drain(..).collect();

        // 2. Ask runtime for newly-due plans (tolerate Sleeping state).
        if let Ok(due) = runtime.plan_due(at) {
            plans.extend(due);
        }

        // 3. Dispatch each plan with single-flight guard via enqueue callback.
        //    Both catch_up and normal plans dispatch identically as Schedule.
        //    See plan §2.5 race safety argument.
        for plan in plans {
            let Some(connection) = (self.resolve)(&plan.connection_id) else {
                // Connection deleted — skip.
                continue;
            };

            // Skip non-live or disabled connections.
            if !connection.enabled
                || !crate::composition::has_live_sync_path(&connection.connector_type)
            {
                continue;
            }

            // Attempt to acquire single-flight guard.
            // The callback returns SyncInProgress when already running — skip
            // to the next interval (plan_due already advanced next_due_at).
            match (self.enqueue)(connection, SyncTrigger::Schedule) {
                Ok(()) => {}
                Err(EnqueueError::SyncInProgress) => {
                    // Skip — next interval naturally retries.
                }
                Err(EnqueueError::Unavailable) => {
                    // Connection/secrets unavailable — skip without error.
                }
            }
        }
    }

    /// Register a connection with the scheduler.
    /// Returns an error if interval is zero.
    pub fn register_connection<Q>(
        &mut self,
        runtime: &mut Runtime<next_infra_runtime::SqliteRuntimeBackend, Q>,
        connection: &Connection,
        at: Timestamp,
    ) -> Result<(), next_infra_runtime::SchedulerError> {
        let interval = crate::composition::query_sync_interval_millis(&connection.connector_type);
        let next_due = at.unix_millis().saturating_add(interval as i64);
        let next_due = Timestamp::from_unix_millis(next_due).expect("timestamp is valid");
        runtime
            .scheduler_mut()
            .register(connection.connection_id.clone(), interval, next_due)
    }

    /// Remove a connection from the scheduler.
    pub fn remove_connection<Q>(
        &mut self,
        runtime: &mut Runtime<next_infra_runtime::SqliteRuntimeBackend, Q>,
        connection_id: &ConnectionId,
    ) {
        runtime.scheduler_mut().remove(connection_id);
    }
}

/// Spawn the GitHub sync task asynchronously.
///
/// This is the free function extracted from AppState::enqueue_github_sync
/// so the driver thread can dispatch syncs without requiring AppState itself
/// to be Send.
///
/// See plan §2.5 for race safety argument (resolve → begin → enqueue order).
pub fn spawn_github_sync(
    store: next_infra_runtime::SharedStore,
    running: Arc<AtomicBool>,
    connector: Arc<GitHubConnector<ReqwestGitHubTransport>>,
    connection: Connection,
    trigger: SyncTrigger,
    sync_run_id: next_infra_core::SyncRunId,
) -> Result<String, next_infra_query::dto::ErrorEnvelope> {
    let store = store.clone();
    let running = running.clone();
    let queued_id = sync_run_id.as_str().to_owned();
    tauri::async_runtime::spawn(async move {
        let _ = crate::composition::sync_github(store, connector, connection, trigger, sync_run_id)
            .await;
        running.store(false, Ordering::Release);
    });
    Ok(queued_id)
}

/// Begin a GitHub sync, acquiring the single-flight guard.
///
/// Returns `Ok(())` if the guard was acquired; the caller must ensure the
/// guard is released (by calling `spawn_github_sync` or resetting `running`).
/// Returns `Err(EnqueueError::SyncInProgress)` if another sync is already running.
pub fn begin_github_sync(running: &AtomicBool) -> Result<(), EnqueueError> {
    if running.swap(true, Ordering::AcqRel) {
        Err(EnqueueError::SyncInProgress)
    } else {
        Ok(())
    }
}

pub fn spawn_ssh_sync(
    store: next_infra_runtime::SharedStore,
    running: Arc<AtomicBool>,
    connector: Arc<SshConnector<OpenSshClient>>,
    connection: Connection,
    trigger: SyncTrigger,
    sync_run_id: next_infra_core::SyncRunId,
) -> Result<String, next_infra_query::dto::ErrorEnvelope> {
    let store = store.clone();
    let running = running.clone();
    let queued_id = sync_run_id.as_str().to_owned();
    tauri::async_runtime::spawn(async move {
        let _ =
            crate::composition::sync_ssh(store, connector, connection, trigger, sync_run_id).await;
        running.store(false, Ordering::Release);
    });
    Ok(queued_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_core::{ConnectorHealth, ConnectorType, SchemaVersion};
    use next_infra_runtime::{Scheduler, SqliteRuntimeBackend};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    fn github_connector_type() -> ConnectorType {
        ConnectorType::new("github").unwrap()
    }

    fn make_connection(id: &str) -> Connection {
        Connection {
            connection_id: ConnectionId::new(id).unwrap(),
            connector_type: github_connector_type(),
            display_name: "Test".into(),
            enabled: true,
            config: serde_json::json!({"selected_repository_ids": ["42"]}),
            secret_ref: None,
            health: ConnectorHealth::Healthy,
            last_success_at: None,
            last_attempt_at: None,
            config_schema_version: SchemaVersion::new(1).unwrap(),
            deleted_at: None,
        }
    }

    fn timestamp(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms).unwrap()
    }

    fn make_runtime() -> Runtime<SqliteRuntimeBackend, ()> {
        let directory = TempDir::new().unwrap();
        let db_path = directory.path().join("test.db");
        let store = next_infra_runtime::SharedStore::open(&db_path).unwrap();
        let backend = SqliteRuntimeBackend::from_shared_store(store);
        let query = next_infra_query::service::QueryService::new(());
        let scheduler = Scheduler::default();
        let mut runtime = Runtime::new(backend, query, scheduler);
        runtime.start_interactive(timestamp(0)).unwrap();
        runtime
    }

    /// Test: a due plan is dispatched with SyncTrigger::Schedule.
    #[test]
    fn due_plan_dispatched_with_schedule_trigger() {
        let connection = make_connection("test-1");
        let connection_id = connection.connection_id.clone();
        let mut runtime = make_runtime();

        // Register connection.
        runtime
            .scheduler_mut()
            .register(connection_id.clone(), 100, timestamp(50))
            .unwrap();

        let mut dispatched: Vec<(ConnectionId, SyncTrigger)> = Vec::new();
        let resolve = |id: &ConnectionId| {
            if id == &connection_id {
                Some(connection.clone())
            } else {
                None
            }
        };
        let enqueue = |conn: Connection, trigger: SyncTrigger| {
            dispatched.push((conn.connection_id.clone(), trigger));
            Ok(())
        };

        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        let mut pending = VecDeque::new();
        driver.tick(&mut runtime, &mut pending, timestamp(100));

        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].0, connection_id);
        assert!(matches!(dispatched[0].1, SyncTrigger::Schedule));
    }

    /// Test: SyncInProgress causes skip, next interval refires.
    #[test]
    fn sync_in_progress_skips_and_next_interval_retries() {
        let connection = make_connection("test-2");
        let connection_id = connection.connection_id.clone();
        let mut runtime = make_runtime();

        // Register with interval 100, due at t=100.
        runtime
            .scheduler_mut()
            .register(connection_id.clone(), 100, timestamp(100))
            .unwrap();

        let resolve = |id: &ConnectionId| {
            if id == &connection_id {
                Some(connection.clone())
            } else {
                None
            }
        };

        // First call: reject with SyncInProgress; second call: accept.
        let attempts = Arc::new(Mutex::new(0));
        let attempts_clone = attempts.clone();
        let enqueue = move |_: Connection, _: SyncTrigger| {
            let mut n = attempts_clone.lock().unwrap();
            *n += 1;
            if *n == 1 {
                Err(EnqueueError::SyncInProgress)
            } else {
                Ok(())
            }
        };

        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        let mut pending = VecDeque::new();

        // First tick: t=100, due, rejected.
        driver.tick(&mut runtime, &mut pending, timestamp(100));
        assert_eq!(*attempts.lock().unwrap(), 1);
        // next_due_at is now 200 (100 + 100 interval).
        assert_eq!(
            runtime
                .scheduler()
                .entry(&connection_id)
                .unwrap()
                .next_due_at,
            timestamp(200)
        );

        // Second tick: t=200, due again, accepted.
        driver.tick(&mut runtime, &mut pending, timestamp(200));
        assert_eq!(*attempts.lock().unwrap(), 2);
    }

    /// Test: two due plans same tick — both attempted in order, second blocked
    /// by first via the closure's own guard state.
    #[test]
    fn two_due_plans_same_tick_both_attempted_sequential_guard() {
        let conn_a = make_connection("test-a");
        let conn_b = make_connection("test-b");
        let id_a = conn_a.connection_id.clone();
        let id_b = conn_b.connection_id.clone();
        let mut runtime = make_runtime();

        // Both due at t=100.
        runtime
            .scheduler_mut()
            .register(id_a.clone(), 100, timestamp(100))
            .unwrap();
        runtime
            .scheduler_mut()
            .register(id_b.clone(), 100, timestamp(100))
            .unwrap();

        let resolve = |id: &ConnectionId| match id {
            _ if id == &id_a => Some(conn_a.clone()),
            _ if id == &id_b => Some(conn_b.clone()),
            _ => None,
        };

        // Simulate single-flight: only one succeeds at a time.
        let guard = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let enqueue = {
            let guard = guard.clone();
            let attempts = attempts.clone();
            move |conn: Connection, _: SyncTrigger| {
                attempts.lock().unwrap().push(conn.connection_id.clone());
                if guard.swap(true, Ordering::AcqRel) {
                    Err(EnqueueError::SyncInProgress)
                } else {
                    Ok(())
                }
            }
        };

        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        let mut pending = VecDeque::new();

        // Single tick: both plans evaluated in registration order (a, b);
        // the second is rejected by the guard and skipped without panicking.
        driver.tick(&mut runtime, &mut pending, timestamp(100));

        let attempted = attempts.lock().unwrap();
        assert_eq!(attempted.len(), 2);
        assert_eq!(attempted[0], id_a);
        assert_eq!(attempted[1], id_b);
    }

    /// Test: sleeping state — plan_due error tolerated, tick no-ops.
    #[test]
    fn sleeping_state_plan_due_error_tolerated() {
        let connection = make_connection("test-3");
        let connection_id = connection.connection_id.clone();
        let mut runtime = make_runtime();

        runtime
            .scheduler_mut()
            .register(connection_id.clone(), 100, timestamp(50))
            .unwrap();

        // Put runtime to sleep.
        runtime.sleep().unwrap();
        assert!(matches!(
            runtime.state(),
            next_infra_runtime::RuntimeState::Sleeping(_)
        ));

        let resolve = |id: &ConnectionId| {
            if id == &connection_id {
                Some(connection.clone())
            } else {
                None
            }
        };
        let enqueue = |_: Connection, _: SyncTrigger| {
            panic!("enqueue should not be called while sleeping");
        };

        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        let mut pending = VecDeque::new();

        // Tick while sleeping: should not dispatch.
        driver.tick(&mut runtime, &mut pending, timestamp(100));

        // Wake: should dispatch the pending plan.
        let wake_plans = runtime.wake(timestamp(100)).unwrap();
        assert_eq!(wake_plans.len(), 1);
        pending.extend(wake_plans);

        let woken_dispatched: Arc<std::sync::Mutex<Vec<_>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let woken_for_closure = woken_dispatched.clone();
        let enqueue2 = move |conn: Connection, _: SyncTrigger| {
            woken_for_closure
                .lock()
                .unwrap()
                .push(conn.connection_id.clone());
            Ok(())
        };

        let mut driver2 = ScheduledSyncDriver::new(resolve, enqueue2);
        driver2.tick(&mut runtime, &mut pending, timestamp(100));

        assert_eq!(woken_dispatched.lock().unwrap().len(), 1);
    }

    /// Test: wake catch-up plans each dispatched at most once.
    #[test]
    fn wake_catch_up_plans_dispatched_at_most_once() {
        let connection = make_connection("test-4");
        let connection_id = connection.connection_id.clone();
        let mut runtime = make_runtime();

        runtime
            .scheduler_mut()
            .register(connection_id.clone(), 100, timestamp(50))
            .unwrap();

        // Sleep then wake: produces catch-up plan.
        runtime.sleep().unwrap();
        let catch_up_plans = runtime.wake(timestamp(100)).unwrap();
        assert_eq!(catch_up_plans.len(), 1);
        assert!(catch_up_plans[0].catch_up);

        let resolve = |id: &ConnectionId| {
            if id == &connection_id {
                Some(connection.clone())
            } else {
                None
            }
        };

        let dispatch_count = Arc::new(Mutex::new(0));
        let enqueue = {
            let dispatch_count_for_enqueue = dispatch_count.clone();
            move |_: Connection, _: SyncTrigger| {
                let mut n = dispatch_count_for_enqueue.lock().unwrap();
                *n += 1;
                Ok(())
            }
        };

        let mut pending: VecDeque<ScheduledSync> = catch_up_plans.into();

        // First tick: should dispatch.
        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        driver.tick(&mut runtime, &mut pending, timestamp(100));
        assert_eq!(*dispatch_count.lock().unwrap(), 1);

        // Second tick before the next due time (wake advanced next_due_at to
        // 200): nothing due and the catch-up plan was already consumed, so no
        // phantom re-dispatch.
        let resolve2 = |id: &ConnectionId| {
            if id == &connection_id {
                Some(connection.clone())
            } else {
                None
            }
        };
        let enqueue2 = {
            let dispatch_count_for_enqueue = dispatch_count.clone();
            move |_: Connection, _: SyncTrigger| {
                let mut n = dispatch_count_for_enqueue.lock().unwrap();
                *n += 1;
                Ok(())
            }
        };
        let mut driver2 = ScheduledSyncDriver::new(resolve2, enqueue2);
        driver2.tick(&mut runtime, &mut pending, timestamp(150));
        assert_eq!(*dispatch_count.lock().unwrap(), 1);
    }

    /// Test: register_connection helper works correctly.
    #[test]
    fn register_connection_sets_next_due_at_correctly() {
        let connection = make_connection("test-5");
        let connection_id = connection.connection_id.clone();
        let mut runtime = make_runtime();

        let resolve = |_: &ConnectionId| Some(connection.clone());
        let enqueue = |_: Connection, _: SyncTrigger| Ok(());

        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        driver
            .register_connection(&mut runtime, &connection, timestamp(500))
            .unwrap();

        let entry = runtime.scheduler().entry(&connection_id).unwrap();
        // next_due_at = now + interval = 500 + 900_000 = 900_500.
        let expected_next = Timestamp::from_unix_millis(900_500).unwrap();
        assert_eq!(entry.next_due_at, expected_next);
        assert_eq!(entry.interval_millis, 900_000);
    }

    /// Test: remove_connection removes scheduler entry.
    #[test]
    fn remove_connection_removes_entry() {
        let connection = make_connection("test-6");
        let connection_id = connection.connection_id.clone();
        let mut runtime = make_runtime();

        runtime
            .scheduler_mut()
            .register(connection_id.clone(), 100, timestamp(100))
            .unwrap();

        let resolve = |_: &ConnectionId| Some(connection.clone());
        let enqueue = |_: Connection, _: SyncTrigger| Ok(());

        let mut driver = ScheduledSyncDriver::new(resolve, enqueue);
        driver.remove_connection(&mut runtime, &connection_id);

        assert!(runtime.scheduler().entry(&connection_id).is_none());
    }

    /// Test: has_live_sync_path returns true only for github.
    #[test]
    fn has_live_sync_path_github_and_ssh() {
        let github = ConnectorType::new("github").unwrap();
        let ssh = ConnectorType::new("ssh").unwrap();
        let dokploy = ConnectorType::new("dokploy").unwrap();

        assert!(crate::composition::has_live_sync_path(&github));
        assert!(crate::composition::has_live_sync_path(&ssh));
        assert!(!crate::composition::has_live_sync_path(&dokploy));
    }
}
