use crate::raft::types::{ReducerCallResponse, TypeConfig};
use openraft::entry::EntryPayload;
use openraft::storage::RaftStateMachine;
use openraft::{BasicNode, Entry, ErrorSubject, ErrorVerb, LogId, RaftLogId, RaftSnapshotBuilder, Snapshot, SnapshotMeta, StorageError, StoredMembership};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::debug;

type C = TypeConfig;

/// Serializable snapshot of the state machine's metadata.
/// SpacetimeDB is the source of truth for actual data; we only snapshot
/// Raft bookkeeping (last_applied, membership, applied_count).
#[derive(Serialize, Deserialize)]
struct SnapshotData {
    last_applied: Option<LogId<u64>>,
    last_membership: StoredMembership<u64, BasicNode>,
    applied_count: u64,
}

/// The Raft state machine for RaftTimeDB.
///
/// When a reducer call is committed by Raft consensus, this state machine
/// forwards the raw WebSocket message to the local SpacetimeDB instance.
/// Since SpacetimeDB reducers are pure and deterministic, all replicas
/// executing the same ordered sequence of reducer calls produce identical state.
#[derive(Clone)]
pub struct StateMachineStore {
    inner: Arc<Mutex<StateMachineInner>>,
    /// Channel sends (origin_node_id, raw_message). The forwarder task uses
    /// origin_node_id to skip writes that the local handler already forwarded.
    forwarder: Arc<Mutex<Option<mpsc::UnboundedSender<(u64, Vec<u8>)>>>>,
}

struct StateMachineInner {
    last_applied: Option<LogId<u64>>,
    last_membership: StoredMembership<u64, BasicNode>,
    applied_count: u64,
}

impl Default for StateMachineStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StateMachineStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateMachineInner {
                last_applied: None,
                last_membership: StoredMembership::default(),
                applied_count: 0,
            })),
            forwarder: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the channel used to forward committed reducer calls to SpacetimeDB.
    pub fn set_forwarder(&self, tx: mpsc::UnboundedSender<(u64, Vec<u8>)>) {
        *self.forwarder.lock().unwrap() = Some(tx);
    }

    /// Get the last applied log ID.
    pub fn last_applied(&self) -> Option<LogId<u64>> {
        self.inner.lock().unwrap().last_applied
    }

    /// Get the count of applied application entries.
    pub fn applied_count(&self) -> u64 {
        self.inner.lock().unwrap().applied_count
    }
}

impl RaftStateMachine<C> for StateMachineStore {
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<
        (Option<LogId<u64>>, StoredMembership<u64, BasicNode>),
        StorageError<u64>,
    > {
        let inner = self.inner.lock().unwrap();
        Ok((inner.last_applied, inner.last_membership.clone()))
    }

    async fn apply<I>(
        &mut self,
        entries: I,
    ) -> Result<Vec<ReducerCallResponse>, StorageError<u64>>
    where
        I: IntoIterator<Item = Entry<C>> + Send,
        I::IntoIter: Send,
    {
        let mut results = Vec::new();
        let forwarder = self.forwarder.lock().unwrap().clone();

        for entry in entries {
            let log_id = *entry.get_log_id();

            {
                let mut inner = self.inner.lock().unwrap();
                inner.last_applied = Some(log_id);

                if let EntryPayload::Membership(ref membership) = entry.payload {
                    inner.last_membership =
                        StoredMembership::new(Some(log_id), membership.clone());
                }
            }

            if let EntryPayload::Normal(request) = entry.payload {
                debug!(
                    log_index = log_id.index,
                    msg_len = request.raw_message.len(),
                    "Forwarding committed reducer call to SpacetimeDB"
                );

                {
                    let mut inner = self.inner.lock().unwrap();
                    inner.applied_count += 1;
                }

                if let Some(ref tx) = forwarder {
                    let _ = tx.send((request.origin_node_id, request.raw_message));
                }

                results.push(ReducerCallResponse { success: true });
            }
        }

        Ok(results)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        _meta: &SnapshotMeta<u64, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        let data: SnapshotData = serde_json::from_slice(snapshot.get_ref())
            .map_err(|e| StorageError::from_io_error(
                ErrorSubject::StateMachine,
                ErrorVerb::Read,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            ))?;

        let mut inner = self.inner.lock().unwrap();
        inner.last_applied = data.last_applied;
        inner.last_membership = data.last_membership;
        inner.applied_count = data.applied_count;

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<C>>, StorageError<u64>> {
        Ok(None)
    }
}

impl RaftSnapshotBuilder<C> for StateMachineStore {
    async fn build_snapshot(
        &mut self,
    ) -> Result<Snapshot<C>, StorageError<u64>> {
        let inner = self.inner.lock().unwrap();

        let data = SnapshotData {
            last_applied: inner.last_applied,
            last_membership: inner.last_membership.clone(),
            applied_count: inner.applied_count,
        };

        let json = serde_json::to_vec(&data)
            .map_err(|e| StorageError::from_io_error(
                ErrorSubject::StateMachine,
                ErrorVerb::Read,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            ))?;

        let snapshot_id = inner
            .last_applied
            .map(|id| format!("{}-{}", id.leader_id, id.index))
            .unwrap_or_else(|| "0-0".to_string());

        let meta = SnapshotMeta {
            last_log_id: inner.last_applied,
            last_membership: inner.last_membership.clone(),
            snapshot_id,
        };

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(json)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state_machine_has_no_applied_state() {
        let sm = StateMachineStore::new();
        assert!(sm.last_applied().is_none());
        assert_eq!(sm.applied_count(), 0);
    }

    #[tokio::test]
    async fn test_forwarder_receives_committed_writes() {
        let sm = StateMachineStore::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        sm.set_forwarder(tx);

        {
            let forwarder = sm.forwarder.lock().unwrap();
            forwarder
                .as_ref()
                .unwrap()
                .send((1, vec![3, 10, 20]))
                .unwrap();
        }

        let (origin, received) = rx.recv().await.unwrap();
        assert_eq!(origin, 1);
        assert_eq!(received, vec![3, 10, 20]);
    }

    #[test]
    fn test_forwarder_is_none_by_default() {
        let sm = StateMachineStore::new();
        let forwarder = sm.forwarder.lock().unwrap();
        assert!(forwarder.is_none());
    }

    #[test]
    fn test_snapshot_data_roundtrip() {
        use openraft::CommittedLeaderId;
        let data = SnapshotData {
            last_applied: Some(LogId::new(CommittedLeaderId::new(1, 0), 42)),
            last_membership: StoredMembership::default(),
            applied_count: 10,
        };
        let json = serde_json::to_vec(&data).unwrap();
        let restored: SnapshotData = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.last_applied, data.last_applied);
        assert_eq!(restored.applied_count, data.applied_count);
    }
}
