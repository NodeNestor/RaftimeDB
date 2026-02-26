use crate::raft::types::TypeConfig;
use openraft::error::{InstallSnapshotError, RPCError, RaftError, RemoteError, Unreachable};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::BasicNode;

type C = TypeConfig;

/// Network transport for Raft inter-node communication via HTTP.
pub struct NetworkFactory {
    client: reqwest::Client,
}

impl NetworkFactory {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for NetworkFactory {
    fn default() -> Self {
        Self::new()
    }
}

pub struct NetworkConnection {
    target: u64,
    addr: String,
    client: reqwest::Client,
}

impl RaftNetworkFactory<C> for NetworkFactory {
    type Network = NetworkConnection;

    async fn new_client(&mut self, target: u64, node: &BasicNode) -> Self::Network {
        NetworkConnection {
            target,
            addr: node.addr.clone(),
            client: self.client.clone(),
        }
    }
}

impl RaftNetwork<C> for NetworkConnection {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<C>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<u64>, RPCError<u64, BasicNode, RaftError<u64>>> {
        let url = format!("http://{}/raft/append", self.addr);
        let resp: Result<AppendEntriesResponse<u64>, RaftError<u64>> = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&e)))?
            .json()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&e)))?;

        resp.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<u64>,
        _option: RPCOption,
    ) -> Result<VoteResponse<u64>, RPCError<u64, BasicNode, RaftError<u64>>> {
        let url = format!("http://{}/raft/vote", self.addr);
        let resp: Result<VoteResponse<u64>, RaftError<u64>> = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&e)))?
            .json()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&e)))?;

        resp.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<C>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<u64>,
        RPCError<u64, BasicNode, RaftError<u64, InstallSnapshotError>>,
    > {
        let url = format!("http://{}/raft/snapshot", self.addr);
        let resp: Result<InstallSnapshotResponse<u64>, RaftError<u64, InstallSnapshotError>> = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&e)))?
            .json()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::new(&e)))?;

        resp.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }
}
