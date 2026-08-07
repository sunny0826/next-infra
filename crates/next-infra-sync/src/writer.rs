use next_infra_core::{CommitResult, StoreWriter, SyncCommit};
use std::collections::VecDeque;

/// The only write boundary used by the sync engine.
///
/// Connector and normalizer code never receives the underlying store. Jobs are
/// queued and drained in insertion order by this single owner, which keeps the
/// commit order deterministic even when observation work is performed in
/// parallel by a caller.
pub struct WriterQueue<S> {
    store: S,
    pending: VecDeque<SyncCommit>,
}

impl<S> WriterQueue<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            pending: VecDeque::new(),
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn into_inner(self) -> S {
        self.store
    }

    pub fn enqueue(&mut self, commit: SyncCommit) {
        self.pending.push_back(commit);
    }
}

impl<S, E> WriterQueue<S>
where
    S: StoreWriter<Error = E>,
{
    /// Drain all currently queued commits in FIFO order.
    pub fn flush(&mut self) -> Result<Vec<CommitResult>, E> {
        let mut results = Vec::with_capacity(self.pending.len());
        while let Some(commit) = self.pending.pop_front() {
            match self.store.commit_sync(commit.clone()) {
                Ok(result) => results.push(result),
                Err(error) => {
                    self.pending.push_front(commit);
                    return Err(error);
                }
            }
        }
        Ok(results)
    }

    /// Queue and immediately drain one commit through the single writer.
    pub fn submit(&mut self, commit: SyncCommit) -> Result<CommitResult, E> {
        self.enqueue(commit);
        self.flush()?
            .pop()
            .ok_or_else(|| unreachable!("submit enqueues exactly one commit"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_core::{StoreWriter, SyncCommit};

    #[derive(Default)]
    struct FakeWriter {
        ids: Vec<String>,
        fail_once: bool,
    }

    impl StoreWriter for FakeWriter {
        type Error = String;

        fn upsert_connection(
            &mut self,
            _connection: next_infra_core::Connection,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn start_sync_run(
            &mut self,
            _sync_run: next_infra_core::SyncRun,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn commit_sync(&mut self, commit: SyncCommit) -> Result<CommitResult, Self::Error> {
            if self.fail_once {
                self.fail_once = false;
                return Err("fixture commit failure".into());
            }
            self.ids.push(commit.sync_run.sync_run_id.into_inner());
            Ok(CommitResult::default())
        }

        fn mark_running_syncs_interrupted(
            &mut self,
            _at: next_infra_core::Timestamp,
        ) -> Result<usize, Self::Error> {
            Ok(0)
        }
    }

    fn commit(id: &str) -> SyncCommit {
        let connection_id = next_infra_core::ConnectionId::new("fixture-connection").unwrap();
        let sync_run_id = next_infra_core::SyncRunId::new(id).unwrap();
        SyncCommit {
            sync_run: next_infra_core::SyncRun {
                sync_run_id,
                connection_id,
                mode: next_infra_core::SyncMode::Full,
                trigger: next_infra_core::SyncTrigger::User,
                started_at: next_infra_core::Timestamp::from_unix_millis(1).unwrap(),
                finished_at: Some(next_infra_core::Timestamp::from_unix_millis(2).unwrap()),
                status: next_infra_core::SyncRunStatus::Succeeded,
                coverage: next_infra_core::SyncCoverage::AuthoritativeFull {
                    scope: next_infra_core::Scope::new("fixture-scope").unwrap(),
                },
                cursor_before: None,
                cursor_after: None,
                counts: next_infra_core::SyncRunCounts::default(),
                errors: Vec::new(),
                warnings: Vec::new(),
            },
            resources: Vec::new(),
            resource_versions: Vec::new(),
            relations: Vec::new(),
            relation_versions: Vec::new(),
            changes: Vec::new(),
            cursor_after: None,
            missing_evidence: None,
        }
    }

    #[test]
    fn queue_flushes_commits_in_fifo_order() {
        let mut queue = WriterQueue::new(FakeWriter::default());
        queue.enqueue(commit("fixture-run-a"));
        queue.enqueue(commit("fixture-run-b"));
        assert_eq!(queue.pending_len(), 2);

        let results = queue.flush().unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(queue.pending_len(), 0);
        assert_eq!(queue.store().ids, ["fixture-run-a", "fixture-run-b"]);
    }

    #[test]
    fn failed_commit_stays_at_the_front_for_retry() {
        let mut queue = WriterQueue::new(FakeWriter {
            fail_once: true,
            ..FakeWriter::default()
        });
        queue.enqueue(commit("fixture-run-a"));
        queue.enqueue(commit("fixture-run-b"));

        assert_eq!(queue.flush(), Err("fixture commit failure".to_owned()));
        assert_eq!(queue.pending_len(), 2);
        assert!(queue.store().ids.is_empty());

        assert_eq!(queue.flush().unwrap().len(), 2);
        assert_eq!(queue.pending_len(), 0);
        assert_eq!(queue.store().ids, ["fixture-run-a", "fixture-run-b"]);
    }
}
