# RaftTimeDB

## What This Is

RaftTimeDB is a **transparent replication proxy** for SpacetimeDB. It wraps SpacetimeDB in a Raft consensus layer so that every write (reducer call) is replicated across multiple nodes. If any node dies, the cluster keeps running. Clients connect to RaftTimeDB exactly like they would connect to SpacetimeDB — same WebSocket protocol, same messages, no code changes.

**Think of it like this:** SpacetimeDB is your database. RaftTimeDB makes it indestructible by running 3+ copies that stay perfectly in sync.

```
Your app (unchanged)  →  RaftTimeDB (any node)  →  SpacetimeDB
                              ↕ Raft consensus ↕
                         RaftTimeDB node 2  →  SpacetimeDB
                              ↕ Raft consensus ↕
                         RaftTimeDB node 3  →  SpacetimeDB
```

## For Users: How To Set Up and Use

### What You Need

- **3 machines** (or containers, or VMs) — Raft needs a majority, so 3 nodes tolerates 1 failure
- **SpacetimeDB** running on each machine (the same version)
- **Your module** published to ALL SpacetimeDB instances (see "Publishing Modules" below)
- **RaftTimeDB binary** on each machine (build with `cargo build --release`)

### Step 1: Start SpacetimeDB on Each Node

Each node needs its own SpacetimeDB instance:

```bash
# On each machine
spacetimedb-standalone start --listen-addr 0.0.0.0:3000
```

### Step 2: Publish Your Module to ALL Nodes

This is important: **publish the same module to every SpacetimeDB instance.**

```bash
spacetime publish --server http://node1:3000 my_module
spacetime publish --server http://node2:3000 my_module
spacetime publish --server http://node3:3000 my_module
```

RaftTimeDB replicates reducer calls (WebSocket writes), not module deployments (HTTP). Each SpacetimeDB instance needs the module installed independently. Once they all have the same module, Raft ensures they execute the same writes in the same order = identical state.

### Step 3: Start RaftTimeDB on Each Node

```bash
# Node 1
rafttimedb --node-id 1 --stdb-url ws://localhost:3000 \
  --peers "2=node2:4001,3=node3:4001"

# Node 2
rafttimedb --node-id 2 --stdb-url ws://localhost:3000 \
  --peers "1=node1:4001,3=node3:4001"

# Node 3
rafttimedb --node-id 3 --stdb-url ws://localhost:3000 \
  --peers "1=node1:4001,2=node2:4001"
```

Each node runs two servers:
- **:3001** — Client WebSocket (what your app connects to)
- **:4001** — Raft management API (inter-node + CLI)

### Step 4: Bootstrap the Cluster

Run this **once** to initialize Raft:

```bash
rtdb init --nodes 1=node1:4001 2=node2:4001 3=node3:4001
```

This triggers leader election. Within a few seconds, one node becomes the leader.

### Step 5: Verify

```bash
rtdb status --addr http://node1:4001
rtdb status --addr http://node2:4001
rtdb status --addr http://node3:4001
```

You should see one Leader and two Followers, all on the same term.

### Step 6: Connect Your App

Point your SpacetimeDB client at **any** RaftTimeDB node:

```
ws://node1:3001    # or node2:3001, or node3:3001 — any works
```

That's it. Your app works exactly like before, but now it's replicated.

### Docker Compose (Local Dev)

For local development, everything is preconfigured:

```bash
cd deploy/docker
docker compose up -d --build

# Wait for startup, then bootstrap:
curl -X POST http://localhost:4001/cluster/init \
  -H 'Content-Type: application/json' \
  -d '{"1":{"addr":"node-1:4001"},"2":{"addr":"node-2:4001"},"3":{"addr":"node-3:4001"}}'

# Check status:
curl http://localhost:4001/cluster/status
curl http://localhost:4002/cluster/status
curl http://localhost:4003/cluster/status

# Connect your client to ws://localhost:3001 (or :3002, :3003)
```

## Scaling the Cluster

RaftTimeDB supports **dynamic scaling** — add or remove nodes while the cluster is live. No downtime required.

### Adding Nodes

1. Start a new SpacetimeDB instance + RaftTimeDB proxy (with a unique `RTDB_NODE_ID`)
2. Publish the same module to the new SpacetimeDB instance
3. Add it to the Raft cluster:
```bash
rtdb add-node --node-id 4 --addr new-host:4001 --cluster http://existing-node:4001
```

The new node automatically syncs: it receives the Raft log from the leader, catches up, and becomes a voting member.

### Removing Nodes

**Always remove from Raft before stopping the container.** Never remove the current leader — remove followers first.

```bash
rtdb remove-node --node-id 4 --cluster http://existing-node:4001
# Then stop the node
```

### Quorum Rules

Always run an **odd number** of nodes for proper majority quorum:

| Nodes | Quorum needed | Tolerates failures | Use case |
|-------|--------------|-------------------|----------|
| 3     | 2            | 1                 | Dev / small prod |
| 5     | 3            | 2                 | Production |
| 7     | 4            | 3                 | High availability |

### Docker Compose Scaling

For local dev, each new node needs two docker-compose services added:
- `stdb-N` (SpacetimeDB on port `5000+N`)
- `node-N` (RaftTimeDB proxy, WS on `3000+N`, Raft on `4000+N`)

Start only the new containers with `docker compose up -d stdb-N node-N` — existing nodes stay running.

See `/scale` Claude skill for step-by-step automation.

## Can I Do Everything I Can With Normal SpacetimeDB?

**Short answer: yes, for runtime operations.**

| Operation | Works Through RaftTimeDB? | Notes |
|-----------|--------------------------|-------|
| Subscribe to tables | Yes, directly | Reads go straight to local SpacetimeDB |
| One-off queries | Yes, directly | Same — no consensus overhead |
| Call reducers | Yes, through Raft | ~1-5ms added for consensus |
| Call procedures | Yes, through Raft | Same as reducers |
| Publish modules | **No** — do it on each SpacetimeDB directly | HTTP operation, not WebSocket |
| Manage databases | **No** — do it on each SpacetimeDB directly | HTTP operation, not WebSocket |
| SpacetimeDB CLI (`spacetime` commands) | Talk to SpacetimeDB directly (:3000) | These use HTTP, not the proxy |

### How Writes Replicate

1. Client sends `CallReducer` to any node (say node-2, a follower)
2. RaftTimeDB reads tag byte `0x03` → classifies as Write
3. Proposes write through Raft consensus
4. If this node is a follower, forwards to the leader internally
5. Leader commits the write, replicates to all nodes
6. **Originating node**: handler forwards the write to its local SpacetimeDB (client gets the response)
7. **Other nodes**: forwarder task sends the committed write to their local SpacetimeDB
8. All 3 SpacetimeDB instances now have identical state

### How Reads Work

Reads (Subscribe, Unsubscribe, OneOffQuery) go **directly** to the local SpacetimeDB with zero overhead. This is safe because all replicas have identical state thanks to Raft-ordered writes.

### What "Decentralized" Means Here

RaftTimeDB is **distributed and fault-tolerant**, not "decentralized" in the blockchain sense:
- There's always one **leader** at a time (elected automatically by Raft)
- Writes go through the leader for ordering
- If the leader dies, a new one is elected in <5 seconds
- This is the same model as etcd, CockroachDB, and TiKV

## Configuration Reference

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--node-id` | `RTDB_NODE_ID` | required | Unique node ID (1, 2, 3, ...) |
| `--listen-addr` | `RTDB_LISTEN_ADDR` | `0.0.0.0:3001` | Client WebSocket address |
| `--raft-addr` | `RTDB_RAFT_ADDR` | `0.0.0.0:4001` | Raft + management API address |
| `--stdb-url` | `RTDB_STDB_URL` | `ws://127.0.0.1:3000` | Local SpacetimeDB WebSocket URL |
| `--peers` | `RTDB_PEERS` | required | Comma-separated: `"2=host:4001,3=host:4001"` |
| `--data-dir` | `RTDB_DATA_DIR` | `./data` | Persistent data directory |

### CLI Commands

```bash
rtdb init --nodes 1=host:4001 2=host:4001 3=host:4001   # Bootstrap cluster (once)
rtdb status --addr http://host:4001                       # Show node status
rtdb add-node --node-id 4 --addr host:4001 --cluster http://leader:4001  # Add node
rtdb remove-node --node-id 2 --cluster http://leader:4001               # Remove node
```

### Management API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/cluster/status` | GET | Node status (state, leader, term) |
| `/cluster/init` | POST | Bootstrap cluster with member map |
| `/cluster/add-node` | POST | Add node (learner → voter) |
| `/cluster/remove-node` | POST | Remove node from cluster |
| `/cluster/write` | POST | Internal: leader receives forwarded writes |
| `/raft/append` | POST | Internal: Raft AppendEntries RPC |
| `/raft/vote` | POST | Internal: Raft RequestVote RPC |
| `/raft/snapshot` | POST | Internal: Raft InstallSnapshot RPC |

## Project Structure

```
crates/
  proxy/src/
    main.rs              ← Entry point: starts HTTP API + WebSocket proxy + forwarder
    api.rs               ← axum HTTP server (Raft RPCs + cluster management)
    config.rs            ← NodeConfig struct
    raft/
      mod.rs             ← RaftNode: init, propose_write (with leader forwarding), parse_peers
      types.rs           ← ReducerCallRequest/Response, TypeConfig
      network.rs         ← HTTP transport (reqwest → POST /raft/*)
      state_machine.rs   ← Apply committed entries, snapshots, forwarder channel
      log_store.rs       ← In-memory Raft log (TODO: RocksDB for production)
    websocket/
      mod.rs             ← Proxy: accepts client connections
      handler.rs         ← Message classification (1-byte tag), client↔SpacetimeDB routing
      upstream.rs        ← Placeholder for connection management
    router/
      mod.rs             ← ShardRouter (Phase 2: multi-shard, currently single-shard)
  cli/src/
    main.rs              ← rtdb CLI: status, init, add-node, remove-node
deploy/
  docker/
    Dockerfile           ← Multi-stage Rust build
    docker-compose.yml   ← 3-node local cluster
  helm/                  ← Kubernetes Helm chart (placeholder)
  k3s/                   ← K3s manifests (placeholder)
tests/
  e2e/
    docker_cluster_test.sh  ← Full cluster E2E test
```

## Key Architecture Decisions

- **HTTP transport** for Raft RPCs (reqwest + axum) — simple and debuggable
- **Explicit bootstrap** via `rtdb init` — deterministic, like etcd
- **Server-side leader forwarding** — followers forward writes to leader internally; clients never see topology
- **One-byte classification** — reads tag byte 0 of each WebSocket message to route reads vs writes
- **Opaque blob replication** — entire WebSocket message replicated, no BSATN parsing needed
- **origin_node_id dedup** — each Raft entry records which node proposed it; the forwarder skips entries from the local node (handler already forwarded them)
- **Metadata-only snapshots** — SpacetimeDB is the source of truth; snapshots store last_applied + membership

## Development

```bash
cargo build                    # Build both crates
cargo test                     # Run all 42 tests
cargo build --release          # Release build
cargo install --path crates/cli   # Install rtdb CLI
RUST_LOG=rafttimedb=debug cargo run --bin rafttimedb -- --node-id 1 ...  # Debug logging
```

### Code Conventions

- Rust 2024 edition
- openraft 0.9 with `serde` + `storage-v2` features
- tokio async runtime throughout
- `anyhow::Result` for application errors, `thiserror` for library errors
- tracing for structured logging (info/debug/warn/error)
- Tests: unit tests in `#[cfg(test)] mod tests` inside each file, integration tests in `tests/`

### Running the E2E Test

```bash
# Requires Docker
./tests/e2e/docker_cluster_test.sh
```

Tests: containers running, API reachable, cluster bootstrap, leader election, write forwarding, fault tolerance (kill node, re-election), node rejoin.

## Known Limitations / Next Steps

- **In-memory log store** — Raft log is lost on restart. Need RocksDB for production persistence.
- **No module deployment replication** — you must `spacetime publish` to each instance separately. A future version could proxy the SpacetimeDB HTTP API too.
- **No client reconnection** — if a node dies, connected clients get disconnected. They need to reconnect to another node. A load balancer in front solves this.
- **No Prometheus metrics** — monitoring is via `rtdb status` only.
- **Single shard** — all writes go through one Raft group. Phase 3 will add multi-shard (one Raft group per module).
