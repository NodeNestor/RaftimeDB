# End-to-End Tests

These tests verify RaftTimeDB works as a complete system.

## Prerequisites

- Docker and Docker Compose
- `spacetime` CLI (for publishing test modules)

## Running

### Docker Cluster Test

Starts a 3-node cluster, verifies health, tests failover:

```bash
chmod +x tests/e2e/docker_cluster_test.sh
./tests/e2e/docker_cluster_test.sh
```

### What It Tests

1. All 3 nodes start and are reachable
2. WebSocket ports are open on all nodes
3. SpacetimeDB instances are healthy
4. Killing a node doesn't kill the cluster
5. Dead node can rejoin

### What It Will Test (once Raft transport is implemented)

1. Publish a SpacetimeDB module through one node
2. Call a reducer through node 1
3. Query state through node 2 (verify replication)
4. Kill the leader node
5. Verify writes still work through a new leader
6. Restart the dead node, verify it catches up
