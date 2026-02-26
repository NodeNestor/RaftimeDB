use openraft::BasicNode;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// A reducer call to be replicated through Raft.
/// We treat the WebSocket message as an opaque blob — no BSATN deserialization needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReducerCallRequest {
    pub raw_message: Vec<u8>,
}

/// Response after a reducer call is committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReducerCallResponse {
    pub success: bool,
}

openraft::declare_raft_types!(
    pub TypeConfig:
        D = ReducerCallRequest,
        R = ReducerCallResponse,
        Node = BasicNode,
        SnapshotData = Cursor<Vec<u8>>,
);
