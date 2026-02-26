# Contributing to RaftTimeDB

Thanks for your interest in contributing! RaftTimeDB is early-stage and there's a lot to build.

## Getting Started

### Prerequisites

- **Rust** (latest stable): https://rustup.rs
- **Docker** (for running SpacetimeDB locally): https://docker.com
- **SpacetimeDB CLI** (optional, for publishing modules): https://spacetimedb.com/install

### Building

```bash
git clone https://github.com/YOUR_ORG/RaftTimeDB.git
cd RaftTimeDB
cargo build
```

### Running the Tests

```bash
cargo test
```

### Running Locally (Docker Compose)

```bash
cd deploy/docker
docker compose up
```

This starts a 3-node cluster. Connect to `ws://localhost:3001`.

## Project Structure

```
crates/
├── proxy/          # Core proxy binary (Raft + WebSocket)
│   └── src/
│       ├── raft/       # Raft consensus (openraft)
│       ├── websocket/  # WebSocket proxy (client ↔ SpacetimeDB)
│       └── router/     # Shard routing (multi-raft, future)
├── cli/            # CLI tool (rtdb)
deploy/
├── docker/         # Docker Compose for local dev
├── helm/           # Helm chart for K8s/K3s
└── k3s/            # K3s-specific examples
```

## What Needs Work

Check the [GitHub Issues](https://github.com/YOUR_ORG/RaftTimeDB/issues) for current priorities. The big areas:

### Phase 1: Make It Work
- TCP transport for Raft inter-node communication
- Cluster bootstrap (initial leader election)
- RocksDB-backed persistent log store
- Snapshot support for node catch-up

### Phase 2: Make It Solid
- Prometheus metrics
- Client failover (redirect to new leader)
- Graceful shutdown
- Integration tests

### Phase 3: Make It Scale
- Multi-Raft (one Raft group per SpacetimeDB module)
- Shard splitting and rebalancing
- K8s operator

## Pull Request Process

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Ensure `cargo test` and `cargo clippy` pass
4. Write a clear PR description explaining what and why
5. Submit!

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings
- Keep functions focused and small
- Add comments for non-obvious logic (but don't over-comment)
- Error handling: use `anyhow` for applications, `thiserror` for libraries

## Communication

- **GitHub Issues**: Bug reports, feature requests, questions
- **Pull Requests**: Code contributions
- **Discussions**: Architecture decisions, design proposals

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.
