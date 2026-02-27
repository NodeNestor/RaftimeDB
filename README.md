<div align="center">

# RaftTimeDB

### Make SpacetimeDB indestructible.

Distributed, fault-tolerant clustering for [SpacetimeDB](https://spacetimedb.com) — automatic replication, failover, and horizontal scaling without modifying SpacetimeDB itself.

[![CI](https://github.com/NodeNestor/RaftimeDB/actions/workflows/ci.yml/badge.svg)](https://github.com/NodeNestor/RaftimeDB/actions)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)

[Quick Start](#quick-start) | [How It Works](#how-it-works) | [Deploy Anywhere](#deploy-anywhere) | [Roadmap](#roadmap)

</div>

---

SpacetimeDB collapses your entire backend into the database — no servers, no microservices, just WASM modules running at memory speed. But it runs on a single node. If that node dies, everything dies.

**RaftTimeDB fixes that.** It wraps SpacetimeDB in a [Raft consensus](https://raft.github.io/) layer that replicates every write across multiple nodes. Kill any node — the cluster keeps running. No data loss. No downtime. Clients don't even notice.

```
Your app (unchanged)
       │
       ▼
  RaftTimeDB ──── Raft ──── RaftTimeDB ──── Raft ──── RaftTimeDB
       │                        │                        │
  SpacetimeDB              SpacetimeDB              SpacetimeDB
  (identical)              (identical)              (identical)
```

## Quick Start

### Option 1: Docker Compose (easiest)

Spins up a 3-node cluster locally in seconds.

```bash
git clone https://github.com/NodeNestor/RaftimeDB.git
cd RaftTimeDB/deploy/docker
docker compose up
```

Connect your SpacetimeDB client to `ws://localhost:3001` (or `:3002`, `:3003` — any node works).

### Option 2: Pre-built Binary

Download from [Releases](https://github.com/NodeNestor/RaftimeDB/releases) for your platform:

| Platform | Binary |
|----------|--------|
| Linux x86_64 | `rafttimedb-x86_64-unknown-linux-gnu` |
| Windows x86_64 | `rafttimedb-x86_64-pc-windows-msvc.exe` |
| macOS ARM | `rafttimedb-aarch64-apple-darwin` |

```bash
# Start 3 nodes (each alongside a SpacetimeDB instance)
rafttimedb --node-id 1 --peers node2:4001,node3:4001 --stdb-url ws://localhost:3000
rafttimedb --node-id 2 --peers node1:4001,node3:4001 --stdb-url ws://localhost:3010
rafttimedb --node-id 3 --peers node1:4001,node2:4001 --stdb-url ws://localhost:3020
```

### Option 3: Build from Source

```bash
# Requires Rust 1.85+
cargo install --path crates/proxy
rafttimedb --help
```

## How It Works

SpacetimeDB reducers are **pure, deterministic, and replayable** — same inputs always produce same outputs. This is the key insight: if you feed the same ordered sequence of reducer calls to multiple SpacetimeDB instances, they all end up with identical state.

RaftTimeDB uses [Raft consensus](https://raft.github.io/) to create that ordered sequence across a cluster.

### The One-Byte Trick

The proxy reads **a single byte** from each WebSocket message to classify it:

```
message[0] = BSATN tag byte (SpacetimeDB v2 protocol)

Tag 0: Subscribe        → READ  → forward directly (no consensus)
Tag 1: Unsubscribe      → READ  → forward directly
Tag 2: OneOffQuery       → READ  → forward directly
Tag 3: CallReducer       → WRITE → replicate through Raft, then forward
Tag 4: CallProcedure     → WRITE → replicate through Raft, then forward
```

- **Writes** go through Raft consensus first (adds ~1-5ms), then to SpacetimeDB
- **Reads** go straight to the local SpacetimeDB (zero overhead — state is identical on all replicas)
- The entire message is replicated as an **opaque blob** — no BSATN deserialization needed

### What Happens When a Node Dies

```
Before:  Node 1 (LEADER) ←→ Node 2 ←→ Node 3
                                         │
                                     [Node 3 dies]

After:   Node 1 (LEADER) ←→ Node 2
         (still serving, zero downtime)

Later:   Node 1 (LEADER) ←→ Node 2 ←→ Node 3 (rejoins, catches up)
```

1. Raft detects the failure (missed heartbeats, ~1-2 seconds)
2. If the leader died, remaining nodes elect a new leader (<5 seconds)
3. Cluster continues serving with the remaining majority
4. When the dead node comes back, it catches up from the Raft log
5. **Zero data loss** — Raft only acknowledges writes after majority commit

## Deploy Anywhere

RaftTimeDB is a single binary. Run it wherever you want.

### Docker Compose (local dev / small deployments)

```bash
cd deploy/docker
docker compose up
```

3 SpacetimeDB instances + 3 RaftTimeDB proxies. Ports 3001-3003 exposed.

### Docker (single node)

```bash
docker run -p 3001:3001 -p 4001:4001 \
  -e RTDB_NODE_ID=1 \
  -e RTDB_STDB_URL=ws://host.docker.internal:3000 \
  -e RTDB_PEERS=node2:4001,node3:4001 \
  ghcr.io/nodenestor/rafttimedb:latest
```

### Windows

Download `rafttimedb.exe` from [Releases](https://github.com/NodeNestor/RaftimeDB/releases) or build from source:

```powershell
cargo build --release
.\target\release\rafttimedb.exe --node-id 1 --stdb-url ws://localhost:3000 --peers node2:4001,node3:4001
```

### Linux / macOS

```bash
cargo build --release
./target/release/rafttimedb --node-id 1 --stdb-url ws://localhost:3000 --peers node2:4001,node3:4001
```

### Kubernetes (K8s / K3s)

```bash
helm install rafttimedb deploy/helm/rafttimedb \
  --set replicas=3 \
  --set spacetimedb.image=clockworklabs/spacetime:latest
```

Or apply the raw manifests:

```bash
kubectl apply -f deploy/k3s/
```

<details>
<summary>Example K8s manifest</summary>

```yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: rafttimedb
spec:
  serviceName: rafttimedb-raft
  replicas: 3
  selector:
    matchLabels:
      app: rafttimedb
  template:
    metadata:
      labels:
        app: rafttimedb
    spec:
      containers:
        - name: rafttimedb
          image: ghcr.io/nodenestor/rafttimedb:latest
          ports:
            - containerPort: 3001  # Client WebSocket
              name: ws
            - containerPort: 4001  # Raft inter-node
              name: raft
          env:
            - name: RTDB_NODE_ID
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name  # rafttimedb-0, rafttimedb-1, ...
            - name: RTDB_STDB_URL
              value: "ws://localhost:3000"
            - name: RTDB_PEERS
              value: "rafttimedb-0.rafttimedb-raft:4001,rafttimedb-1.rafttimedb-raft:4001,rafttimedb-2.rafttimedb-raft:4001"
        - name: spacetimedb
          image: clockworklabs/spacetime:latest
          command: ["spacetimedb-standalone", "start", "--listen-addr", "0.0.0.0:3000"]
          ports:
            - containerPort: 3000
          volumeMounts:
            - name: stdb-data
              mountPath: /app/data
  volumeClaimTemplates:
    - metadata:
        name: stdb-data
      spec:
        accessModes: ["ReadWriteOnce"]
        resources:
          requests:
            storage: 10Gi
---
# Headless service for Raft peer discovery
apiVersion: v1
kind: Service
metadata:
  name: rafttimedb-raft
spec:
  clusterIP: None
  selector:
    app: rafttimedb
  ports:
    - port: 4001
      name: raft
---
# Client-facing service
apiVersion: v1
kind: Service
metadata:
  name: rafttimedb
spec:
  type: LoadBalancer
  selector:
    app: rafttimedb
  ports:
    - port: 3001
      name: ws
```

</details>

### Bare Metal (3 machines, no orchestrator)

```bash
# Machine 1 (192.168.1.10)
rafttimedb --node-id 1 --listen-addr 0.0.0.0:3001 --raft-addr 0.0.0.0:4001 \
  --stdb-url ws://127.0.0.1:3000 --peers 192.168.1.11:4001,192.168.1.12:4001

# Machine 2 (192.168.1.11)
rafttimedb --node-id 2 --listen-addr 0.0.0.0:3001 --raft-addr 0.0.0.0:4001 \
  --stdb-url ws://127.0.0.1:3000 --peers 192.168.1.10:4001,192.168.1.12:4001

# Machine 3 (192.168.1.12)
rafttimedb --node-id 3 --listen-addr 0.0.0.0:3001 --raft-addr 0.0.0.0:4001 \
  --stdb-url ws://127.0.0.1:3000 --peers 192.168.1.10:4001,192.168.1.11:4001
```

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    RaftTimeDB Node                        │
│                                                          │
│  ┌────────────────────────────────────────────────────┐  │
│  │              WebSocket Proxy (:3001)                │  │
│  │                                                    │  │
│  │  Client msg ──► Read tag byte ──► Write? ──────┐   │  │
│  │                      │                         │   │  │
│  │                      │ Read                    │   │  │
│  │                      ▼                         ▼   │  │
│  │              Forward directly          Raft consensus  │
│  │              to SpacetimeDB       (replicate to peers) │
│  │                      │                         │   │  │
│  │                      │            After commit │   │  │
│  │                      ▼                    ▼    │   │  │
│  │              ┌──────────────────────────────┐  │   │  │
│  │              │  Local SpacetimeDB (:3000)   │  │   │  │
│  │              │  (identical on all nodes)    │  │   │  │
│  │              └──────────────────────────────┘  │   │  │
│  └────────────────────────────────────────────────┘  │  │
│                                                      │  │
│  ┌────────────────────────────────────────────────┐  │  │
│  │           Raft Transport (:4001)               │  │  │
│  │  Communicates with peer nodes for consensus    │  │  │
│  └────────────────────────────────────────────────┘  │  │
└──────────────────────────────────────────────────────┘  │
                                                          │
        ◄──── Raft protocol ────►                         │
                                                          │
┌──────────────────┐          ┌──────────────────┐        │
│  RaftTimeDB      │          │  RaftTimeDB      │        │
│  Node 2          │          │  Node 3          │        │
│  (same layout)   │          │  (same layout)   │        │
└──────────────────┘          └──────────────────┘        │
```

## Use Cases

### MMO Game Servers
Thousands of players across sharded game zones. Each zone is a SpacetimeDB module backed by its own Raft group. Kill a node — players barely notice. Add nodes — the world gets bigger.

### AI Agent Swarms
Hundreds of autonomous agents with shared state. Agents call reducers to update shared knowledge, subscribe to tables for real-time updates. The swarm is indestructible — pull the plug on any node, agents reconnect and keep going.

### Real-Time Collaboration
Shared documents, cursors, presence. SpacetimeDB's subscription system pushes deltas to all connected clients. RaftTimeDB ensures the backing state survives failures.

### Any SpacetimeDB App That Can't Afford Downtime
If your app matters, it should be replicated. RaftTimeDB makes that a config change, not a rewrite.

## CLI Tool

```bash
# Cluster management
rtdb init --nodes 1=node1:4001 2=node2:4001 3=node3:4001   # Bootstrap cluster (once)
rtdb status --addr http://localhost:4001                      # Show node status
rtdb add-node --node-id 4 --addr new-node:4001               # Add a node
rtdb remove-node --node-id 2                                  # Remove a node

# Shard management (multi-raft)
rtdb create-shard --shard-id 1 --cluster http://localhost:4001
rtdb add-route --module my_game --shard-id 1 --cluster http://localhost:4001
rtdb remove-route --module my_game --cluster http://localhost:4001
rtdb list-shards --addr http://localhost:4001
rtdb shard-status --shard-id 1 --addr http://localhost:4001
```

## Configuration

All options can be set via CLI flags or environment variables:

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--node-id` | `RTDB_NODE_ID` | *required* | Unique node identifier |
| `--listen-addr` | `RTDB_LISTEN_ADDR` | `0.0.0.0:3001` | Client WebSocket address |
| `--raft-addr` | `RTDB_RAFT_ADDR` | `0.0.0.0:4001` | Raft inter-node address |
| `--stdb-url` | `RTDB_STDB_URL` | `ws://127.0.0.1:3000` | Local SpacetimeDB URL |
| `--peers` | `RTDB_PEERS` | *required* | Comma-separated peer Raft addresses |
| `--data-dir` | `RTDB_DATA_DIR` | `./data` | Persistent data directory |
| `--tls-cert` | `RTDB_TLS_CERT` | *none* | Path to TLS certificate (PEM) |
| `--tls-key` | `RTDB_TLS_KEY` | *none* | Path to TLS private key (PEM) |
| `--tls-ca-cert` | `RTDB_TLS_CA_CERT` | *none* | CA cert for peer verification (PEM) |

## Production Features

RaftTimeDB includes everything needed for production deployments:

- **TLS encryption** — inter-node Raft RPCs and management API served over HTTPS (`--tls-cert` + `--tls-key`)
- **Prometheus metrics** — `GET /metrics` exposes Raft term, leader status, connections, write latency, and more
- **Health checks** — `GET /cluster/health` returns 200 when leader is known, 503 otherwise
- **Leader discovery** — `GET /cluster/leader` returns the current leader's address for client routing
- **Client reconnection hints** — on graceful shutdown, WebSocket close frames include `leader=<id>:<addr>` so clients know where to reconnect
- **Graceful shutdown** — Ctrl+C / SIGTERM triggers coordinated shutdown: drain connections, send close frames, 5-second drain period
- **Persistent Raft log** — redb (pure Rust, ACID) stores Raft log + vote on disk; nodes survive restarts
- **Snapshot support** — fast catch-up for new or restarted nodes
- **Multi-shard (Multi-Raft)** — each SpacetimeDB module can get its own Raft group for parallel write throughput

## Roadmap

### Phase 1: Single-Shard Replication
- [x] WebSocket proxy with BSATN single-byte tag inspection
- [x] Raft consensus integration (openraft)
- [x] Docker Compose (3-node local cluster)
- [x] GitHub Actions CI (Linux, Windows, macOS)
- [x] Cluster bootstrap and leader election
- [x] Persistent log store (redb — pure Rust, ACID)
- [x] End-to-end working demo

### Phase 2: Production Ready
- [x] TLS encryption for inter-node and client communication
- [x] Prometheus metrics (Raft health, connections, latency)
- [x] Snapshot support (new node catch-up)
- [x] Client reconnection hints on failover
- [x] Graceful shutdown and connection draining
- [x] Health check and leader discovery endpoints
- [x] Helm chart for K8s/K3s

### Phase 3: Horizontal Scaling (Multi-Raft)
- [x] Each SpacetimeDB module gets its own Raft group
- [x] Shard router (route clients to correct Raft group by module name)
- [x] Shard management API + CLI (create shards, add/remove routes)
- [x] Shard config replicated via Raft consensus (no external store)
- [x] Per-shard persistent log stores with auto-migration
- [x] Backwards compatible — existing single-shard clusters work unchanged
- [ ] Shard splitting when memory threshold exceeded
- [ ] Coalesced heartbeats across Raft groups (CockroachDB pattern)
- [ ] Batched persistence writes (TiKV pattern)
- [ ] Cross-shard communication (saga pattern)

### Phase 4: Kubernetes Operator
- [ ] `SpacetimeCluster` Custom Resource Definition
- [ ] Auto-scaling based on connection count / memory
- [ ] Rolling upgrades with zero downtime
- [ ] Automatic shard rebalancing

## Tech Stack

| Component | Choice | Why |
|-----------|--------|-----|
| Language | [Rust](https://www.rust-lang.org) | Same as SpacetimeDB. Memory safe. Fast. |
| Consensus | [openraft](https://github.com/databendlabs/openraft) | Async/Tokio, event-driven, 3.5M+ writes/sec |
| WebSocket | [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) | Mature, full message inspection, zero-copy |
| Runtime | [Tokio](https://tokio.rs) | Industry standard async runtime |
| Protocol | BSATN (SpacetimeDB native) | One-byte classification, opaque blob replication |
| TLS | [rustls](https://github.com/rustls/rustls) + [tokio-rustls](https://github.com/rustls/tokio-rustls) | Pure Rust TLS, no OpenSSL dependency |
| Metrics | [prometheus](https://github.com/tikv/rust-prometheus) | Same crate TiKV uses. Text format for Grafana/etc. |
| Persistence | [redb](https://github.com/cberner/redb) | Pure Rust embedded ACID database for Raft log |
| K8s *(planned)* | [kube-rs](https://kube.rs) | Rust-native K8s operator framework, CNCF Sandbox |

## Design Decisions

<details>
<summary><b>Why a proxy instead of forking SpacetimeDB?</b></summary>

We don't modify SpacetimeDB at all. New SpacetimeDB versions work automatically. The proxy reads one byte per message — it's invisible to both clients and SpacetimeDB. Your existing modules work unchanged.
</details>

<details>
<summary><b>Why openraft over raft-rs (TiKV)?</b></summary>

openraft is event-driven (not tick-based), so operations proceed at network speed instead of polling intervals. It's natively async with Tokio. raft-rs is more battle-tested (powers TiKV) but requires building the entire event loop yourself.
</details>

<details>
<summary><b>Why no shared storage (Longhorn/Ceph)?</b></summary>

Raft replication handles durability. Adding distributed storage underneath is a redundant replication layer with added latency. Each node uses local disk — same approach CockroachDB and TiKV use.
</details>

<details>
<summary><b>Why not just wait for SpacetimeDB to add clustering?</b></summary>

They might! But SpacetimeDB is focused on their game (BitCraft) and single-node performance. Clustering is not on their near-term roadmap. RaftTimeDB fills the gap now, and if SpacetimeDB adds native clustering later, the patterns here will inform that work.
</details>

## Contributing

We'd love your help! See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

This project is in **early development** — perfect time to get involved and shape the direction.

## License

[Apache 2.0](LICENSE)
