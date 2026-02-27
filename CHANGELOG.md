# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project structure with Cargo workspace
- WebSocket proxy with BSATN tag byte inspection (read/write classification)
- Raft consensus integration via openraft
- State machine that forwards committed writes to SpacetimeDB
- CLI tool (`rtdb`) with cluster management commands
- Docker Compose setup for local 3-node cluster
- Dockerfile with multi-stage Rust build
- Persistent Raft log store using redb (pure Rust, ACID) — survives restarts
- TLS encryption for inter-node Raft RPCs and management API (`--tls-cert`, `--tls-key`, `--tls-ca-cert`)
- Prometheus metrics endpoint (`GET /metrics`) with Raft term, leader status, connections, write latency, entries applied
- Health check endpoint (`GET /cluster/health`) — returns 200 when leader known, 503 otherwise
- Leader discovery endpoint (`GET /cluster/leader`) — returns current leader ID and address
- Client reconnection hints — WebSocket close frames include `leader=<id>:<addr>` on graceful shutdown
- Graceful shutdown with Ctrl+C / SIGTERM handling, coordinated task shutdown, and 5-second drain period
- Snapshot caching for fast new-node catch-up
- Helm chart and K3s manifests for Kubernetes deployment
- End-to-end Docker cluster test script
- GitHub Actions CI for Linux, Windows, and macOS
