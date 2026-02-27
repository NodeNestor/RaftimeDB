use crate::raft::types::TypeConfig;
use openraft::error::{InstallSnapshotError, RPCError, RaftError, RemoteError, Unreachable};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::BasicNode;

type C = TypeConfig;

/// Network transport for Raft inter-node communication via HTTP(S).
pub struct NetworkFactory {
    client: reqwest::Client,
    use_tls: bool,
}

impl NetworkFactory {
    pub fn new(use_tls: bool, ca_cert_pem: Option<&[u8]>) -> Self {
        let mut builder = reqwest::Client::builder();

        if let Some(ca_pem) = ca_cert_pem {
            if let Ok(cert) = reqwest::Certificate::from_pem(ca_pem) {
                builder = builder.add_root_certificate(cert);
            }
        }

        Self {
            client: builder.build().unwrap_or_else(|_| reqwest::Client::new()),
            use_tls,
        }
    }
}

impl Default for NetworkFactory {
    fn default() -> Self {
        Self::new(false, None)
    }
}

pub struct NetworkConnection {
    target: u64,
    addr: String,
    client: reqwest::Client,
    use_tls: bool,
}

impl NetworkConnection {
    fn url(&self, path: &str) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}{}", scheme, self.addr, path)
    }
}

impl RaftNetworkFactory<C> for NetworkFactory {
    type Network = NetworkConnection;

    async fn new_client(&mut self, target: u64, node: &BasicNode) -> Self::Network {
        NetworkConnection {
            target,
            addr: node.addr.clone(),
            client: self.client.clone(),
            use_tls: self.use_tls,
        }
    }
}

impl RaftNetwork<C> for NetworkConnection {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<C>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<u64>, RPCError<u64, BasicNode, RaftError<u64>>> {
        let url = self.url("/raft/append");
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
        let url = self.url("/raft/vote");
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
        let url = self.url("/raft/snapshot");
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
