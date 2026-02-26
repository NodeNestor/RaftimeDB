# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project structure with Cargo workspace
- WebSocket proxy with BSATN tag byte inspection (read/write classification)
- Raft consensus integration via openraft
- In-memory log store for development
- State machine that forwards committed writes to SpacetimeDB
- CLI tool (`rtdb`) with cluster management commands
- Docker Compose setup for local 3-node cluster
- Dockerfile with multi-stage Rust build
