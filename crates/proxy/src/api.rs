use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use openraft::error::InstallSnapshotError;
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{BasicNode, Raft};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::metrics;
use crate::raft::types::{ReducerCallRequest, TypeConfig};

/// Shared application state for the HTTP management API.
pub struct AppState {
    pub raft: Raft<TypeConfig>,
    pub node_id: u64,
    /// Peer node addresses for leader lookup.
    pub peers: BTreeMap<u64, BasicNode>,
    /// URL scheme for inter-node communication.
    pub http_scheme: String,
}

/// Build the axum router with all Raft RPC and cluster management endpoints.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // Raft RPC endpoints (receiving side of inter-node communication)
        .route("/raft/append", post(raft_append))
        .route("/raft/vote", post(raft_vote))
        .route("/raft/snapshot", post(raft_snapshot))
        // Cluster management endpoints
        .route("/cluster/status", get(cluster_status))
        .route("/cluster/init", post(cluster_init))
        .route("/cluster/add-node", post(cluster_add_node))
        .route("/cluster/remove-node", post(cluster_remove_node))
        // Write forwarding endpoint (leader receives forwarded writes from followers)
        .route("/cluster/write", post(cluster_write))
        // Discovery / monitoring endpoints
        .route("/cluster/leader", get(cluster_leader))
        .route("/cluster/health", get(cluster_health))
        .route("/metrics", get(prometheus_metrics))
        .with_state(state)
}

// ============================================================================
// Raft RPC Endpoints
// ============================================================================

/// Handle AppendEntries RPC from another Raft node.
async fn raft_append(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AppendEntriesRequest<TypeConfig>>,
) -> Json<Result<AppendEntriesResponse<u64>, openraft::error::RaftError<u64>>> {
    Json(state.raft.append_entries(req).await)
}

/// Handle RequestVote RPC from another Raft node.
async fn raft_vote(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VoteRequest<u64>>,
) -> Json<Result<VoteResponse<u64>, openraft::error::RaftError<u64>>> {
    Json(state.raft.vote(req).await)
}

/// Handle InstallSnapshot RPC from another Raft node.
async fn raft_snapshot(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InstallSnapshotRequest<TypeConfig>>,
) -> Json<Result<InstallSnapshotResponse<u64>, openraft::error::RaftError<u64, InstallSnapshotError>>>
{
    Json(state.raft.install_snapshot(req).await)
}

// ============================================================================
// Cluster Management Endpoints
// ============================================================================

#[derive(Serialize)]
struct ClusterStatus {
    node_id: u64,
    state: String,
    current_leader: Option<u64>,
    current_term: u64,
    last_applied: Option<String>,
    membership: String,
}

/// Return cluster status for this node.
async fn cluster_status(State(state): State<Arc<AppState>>) -> Json<ClusterStatus> {
    let metrics = state.raft.metrics().borrow().clone();
    Json(ClusterStatus {
        node_id: state.node_id,
        state: format!("{:?}", metrics.state),
        current_leader: metrics.current_leader,
        current_term: metrics.vote.leader_id().term,
        last_applied: metrics.last_applied.map(|id| format!("{}", id)),
        membership: format!("{:?}", metrics.membership_config),
    })
}

/// Initialize the Raft cluster with the given set of members.
/// This must be called exactly once on one node to bootstrap the cluster.
async fn cluster_init(
    State(state): State<Arc<AppState>>,
    Json(members): Json<BTreeMap<u64, BasicNode>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .raft
        .initialize(members)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::OK, "Cluster initialized".to_string()))
}

#[derive(Deserialize)]
struct AddNodeRequest {
    node_id: u64,
    addr: String,
}

/// Add a new node to the cluster (first as learner, then as voter).
async fn cluster_add_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddNodeRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let node = BasicNode {
        addr: req.addr.clone(),
    };

    // First add as learner
    state
        .raft
        .add_learner(req.node_id, node, true)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Then promote to voter by adding to membership
    let metrics = state.raft.metrics().borrow().clone();
    let mut voter_ids: BTreeSet<u64> = metrics
        .membership_config
        .membership()
        .voter_ids()
        .collect();
    voter_ids.insert(req.node_id);

    state
        .raft
        .change_membership(voter_ids, false)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::OK,
        format!("Node {} added to cluster", req.node_id),
    ))
}

#[derive(Deserialize)]
struct RemoveNodeRequest {
    node_id: u64,
}

/// Remove a node from the cluster.
async fn cluster_remove_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoveNodeRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let metrics = state.raft.metrics().borrow().clone();
    let voter_ids: BTreeSet<u64> = metrics
        .membership_config
        .membership()
        .voter_ids()
        .filter(|id| *id != req.node_id)
        .collect();

    state
        .raft
        .change_membership(voter_ids, false)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::OK,
        format!("Node {} removed from cluster", req.node_id),
    ))
}

/// Handle a write forwarded from a follower node.
/// The leader executes client_write and returns the result.
async fn cluster_write(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReducerCallRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .raft
        .client_write(req)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::OK, "Write committed".to_string()))
}

// ============================================================================
// Discovery & Monitoring Endpoints
// ============================================================================

#[derive(Serialize)]
struct LeaderInfo {
    leader_id: Option<u64>,
    leader_addr: Option<String>,
    this_node_is_leader: bool,
}

/// Return the current Raft leader's address.
/// Clients use this for reconnection after failover.
async fn cluster_leader(State(state): State<Arc<AppState>>) -> Json<LeaderInfo> {
    let metrics = state.raft.metrics().borrow().clone();
    let leader_id = metrics.current_leader;
    let this_node_is_leader = leader_id == Some(state.node_id);

    let leader_addr = leader_id.and_then(|id| {
        if id == state.node_id {
            None // Return None for self — client is already connected here
        } else {
            state.peers.get(&id).map(|n| n.addr.clone())
        }
    });

    Json(LeaderInfo {
        leader_id,
        leader_addr,
        this_node_is_leader,
    })
}

/// Health check endpoint. Returns 200 if the node is healthy, 503 if not.
async fn cluster_health(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let metrics = state.raft.metrics().borrow().clone();

    // A node is healthy if it knows who the leader is
    if metrics.current_leader.is_some() {
        Ok((StatusCode::OK, "healthy".to_string()))
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, "no leader elected".to_string()))
    }
}

/// Prometheus metrics endpoint.
async fn prometheus_metrics() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        metrics::encode_metrics(),
    )
}
